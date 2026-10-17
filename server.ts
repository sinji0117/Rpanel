import http from 'node:http'
import path from 'node:path'
import fs from 'node:fs'
import { fileURLToPath } from 'node:url'
import express, { Request, Response } from 'express'
import cookieParser from 'cookie-parser'
import cors from 'cors'
import { WebSocketServer, WebSocket } from 'ws'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

function getArg(flag: string) {
  const idx = process.argv.indexOf(flag)
  return idx >= 0 && idx + 1 < process.argv.length ? process.argv[idx + 1] : undefined
}

const PKG_VERSION = '0.10.0'
const PORT = Number(getArg('--port') || process.env.PORT) || 3000
const HOST = getArg('--host') || process.env.HOST || '0.0.0.0'
const isProd = process.env.NODE_ENV === 'production'

const now = () => new Date().toISOString()

// In-Memory Database store initialized with realistic state
const db = {
  needsSetup: false,
  user: { id: 1, email: 'admin@example.com', role: 'admin', created_at: now() },
  password: 'correct-horse-1',
  loggedIn: true,
  projects: [
    { id: 1, name: 'Beri Service', description: '메인 전자상거래 및 API 서비스', created_at: now(), updated_at: now() },
  ],
  apps: [
    {
      id: 1,
      project_id: 1,
      name: 'api.example.com',
      slug: 'api-example-com',
      kind: 'rust',
      source: 'git',
      git_url: 'https://github.com/example/api.git',
      git_branch: 'main',
      domains: 'api.example.com',
      site_user: 'api_user',
      site_root: '/home/api_user/htdocs/api.example.com',
      web_root: '',
      proxy_target: null,
      start_cmd: 'cargo run --release',
      build_cmd: 'cargo build --release',
      health_path: '/health',
      port: 8101,
      status: 'running',
      ssl_mode: 'none',
      force_https: false,
      client_max_body_mb: 64,
      current_release: 1,
      current_commit: 'a1b2c3d',
      redis_db: 4,
      backup_schedule: 'daily',
      backup_keep: 7,
      created_at: now(),
      updated_at: now(),
    },
    {
      id: 2,
      project_id: 1,
      name: 'web.example.com',
      slug: 'web-example-com',
      kind: 'static',
      source: 'files',
      git_url: null,
      git_branch: 'main',
      domains: 'web.example.com',
      site_user: 'web_user',
      site_root: '/home/web_user/htdocs/web.example.com',
      web_root: '',
      proxy_target: null,
      start_cmd: null,
      build_cmd: null,
      health_path: '/',
      port: null,
      status: 'running',
      ssl_mode: 'none',
      force_https: false,
      client_max_body_mb: 32,
      current_release: 1,
      current_commit: null,
      redis_db: null,
      backup_schedule: 'off',
      backup_keep: 7,
      created_at: now(),
      updated_at: now(),
    },
  ] as any[],
  env: {
    1: [
      { key: 'PORT', value: '8101' },
      { key: 'RUST_LOG', value: 'info' },
    ],
    2: [],
  } as Record<number, { key: string; value: string }[]>,
  deployments: [
    {
      id: 1,
      app_id: 1,
      status: 'success',
      triggered_by: 1,
      commit_sha: 'a1b2c3d',
      error: null,
      started_at: new Date(Date.now() - 3600000).toISOString(),
      finished_at: new Date(Date.now() - 3550000).toISOString(),
      created_at: new Date(Date.now() - 3600000).toISOString(),
    },
  ],
  dbs: [
    { id: 1, app_id: 1, name: 'api_db', username: 'api_user', env_key: 'DATABASE_URL', created_at: now() },
  ],
  backups: [
    {
      id: 1,
      app_id: 1,
      app_name: 'api.example.com',
      app_slug: 'api-example-com',
      kind: 'db',
      target: 'api_db',
      file: '20261017-030000-db-api_db.sql.gz',
      size_bytes: 45056,
      status: 'success',
      error: null,
      trigger_by: 'schedule',
      created_at: new Date(Date.now() - 7200000).toISOString(),
      finished_at: new Date(Date.now() - 7190000).toISOString(),
    },
  ],
  jobs: [
    {
      id: 1,
      name: '매일 새벽 DB 백업',
      schedule: 'daily',
      hour: 3,
      minute: 0,
      target: 'all',
      app_id: null,
      database_id: null,
      enabled: true,
      last_run_at: new Date(Date.now() - 7200000).toISOString(),
      created_at: now(),
    },
  ],
  dbUsers: [] as any[],
  redisUsers: [] as any[],
  crons: [] as any[],
  files: {
    1: {
      'htdocs/api.example.com': {
        'Cargo.toml': '[package]\nname = "api"\nversion = "0.1.0"\nedition = "2021"\n',
        'README.md': '# API Service\nRunning on Rpanel\n',
      },
    },
    2: {
      'htdocs/web.example.com': {
        'index.html': '<!DOCTYPE html><html><head><title>Welcome</title></head><body><h1>Welcome to Rpanel!</h1></body></html>',
      },
    },
  } as Record<number, Record<string, Record<string, string>>>,
  ssl: {} as Record<string, any>,
  accounts: [
    { id: 1, app_id: 1, username: 'api_user', is_primary: true, access: 'ssh', enabled: true, key_count: 0, note: null, created_at: now() },
    { id: 2, app_id: 2, username: 'web_user', is_primary: true, access: 'sftp', enabled: true, key_count: 0, note: null, created_at: now() },
  ],
  admins: [] as any[],
  mongos: [] as any[],
  panelDomain: null,
  panelAllow: [] as string[],
  engines: [
    {
      engine: 'mariadb',
      label: 'MariaDB',
      installed: true,
      version: '10.11.13-MariaDB',
      series: '10.11',
      list: [
        ['10.11', 'LTS · Ubuntu 24.04 기본'],
        ['11.4', 'LTS'],
        ['11.8', 'LTS'],
      ],
      job: null,
    },
    {
      engine: 'mongodb',
      label: 'MongoDB',
      installed: true,
      version: '8.0.4',
      series: '8.0',
      list: [
        ['8.0', '기본'],
        ['8.2', ''],
      ],
      job: null,
    },
    {
      engine: 'redis',
      label: 'Redis',
      installed: true,
      version: '7.0.15',
      series: '7.0',
      list: [
        ['7.0', 'Ubuntu 24.04 기본'],
        ['8', 'packages.redis.io 최신 8.x'],
      ],
      job: null,
    },
  ],
  dbweb: {
    columns: ['id', 'name', 'price', 'created_at'],
    rows: [
      [1, '프리미엄 원두', '18000.00', '2026-10-15 10:00:00'],
      [2, '콜드브루 보틀', '6500.00', '2026-10-16 11:30:00'],
      [3, '핸드드립 세트', '42000.00', '2026-10-17 09:15:00'],
    ],
    next: 4,
  },
  dbSessions: {} as Record<string, boolean>,
  notifications: [
    {
      id: 1,
      kind: 'deploy_success',
      level: 'info',
      title: '배포가 성공적으로 완료되었습니다',
      params: { app: 'api.example.com' },
      body: 'api.example.com 버전이 정상 배포되었습니다.',
      link: '/apps/1',
      read_at: null,
      created_at: new Date(Date.now() - 3550000).toISOString(),
    },
    {
      id: 2,
      kind: 'backup_success',
      level: 'info',
      title: '자동 백업이 완료되었습니다',
      params: { target: 'api_db' },
      body: '새벽 정기 백업(api_db)이 성공적으로 생성되었습니다.',
      link: '/backups',
      read_at: null,
      created_at: new Date(Date.now() - 7190000).toISOString(),
    },
  ] as any[],
  notifySettings: {
    events: {
      deploy_failed: true,
      deploy_success: false,
      backup_failed: true,
      backup_success: false,
      site_down: true,
      disk_high: true,
      mem_high: true,
      update_available: true,
      runtime_install: true,
      admin_login: false,
    },
    disk_threshold: 90,
    mem_threshold: 90,
    channels: { discord_url: '', slack_url: '', telegram_token: '', telegram_chat_id: '', webhook_url: '' },
  },
  version: {
    current: PKG_VERSION,
    latest: '0.10.0',
    update_available: false,
    repo_url: 'https://github.com/sinji0117/Rpanel',
    checked_at: now(),
    error: null,
  },
  runtimes: [
    { id: 901, lang: 'rust', version: 'stable', status: 'ready', is_default: true, is_system: true, detail: '1.90.0', log: null, updated_at: now() },
    { id: 902, lang: 'python', version: 'system', status: 'ready', is_default: true, is_system: true, detail: '3.12.3', log: null, updated_at: now() },
    { id: 903, lang: 'node', version: 'system', status: 'ready', is_default: true, is_system: true, detail: '22.11.0', log: null, updated_at: now() },
  ],
  seq: 10,
  storage: { backups: null, projects: null } as any,
  storageJob: null as any,
  mongoDocs: {
    orders: [
      { _id: { $oid: '65a000000000000000000001' }, item: '에스프레소 머신', qty: 1, status: 'completed' },
      { _id: { $oid: '65a000000000000000000002' }, item: '그라인더', qty: 2, status: 'pending' },
    ],
  } as Record<string, any[]>,
  redisKeys: {
    'session:admin': '{"user_id":1,"role":"admin"}',
    'cache:stats': '{"views":1240,"visitors":380}',
  } as Record<string, string>,
  systemEvents: [
    { id: 1, timestamp: new Date(Date.now() - 180000).toISOString(), level: 'info', source: 'rpanel', message: 'Rpanel daemon supervisor active on http://0.0.0.0:3000' },
    { id: 2, timestamp: new Date(Date.now() - 150000).toISOString(), level: 'info', source: 'nginx', message: 'Nginx reverse proxy health check 200 OK (latency: 1.2ms)' },
    { id: 3, timestamp: new Date(Date.now() - 120000).toISOString(), level: 'info', source: 'mariadb', message: 'MariaDB InnoDB buffer pool warm; 4 active database connections' },
    { id: 4, timestamp: new Date(Date.now() - 90000).toISOString(), level: 'warn', source: 'system', message: 'I/O wait threshold reached 12% on mount / during daily snapshot preparation' },
    { id: 5, timestamp: new Date(Date.now() - 60000).toISOString(), level: 'info', source: 'redis', message: 'Redis background save: 16 keys persisted to dump.rdb in 4ms' },
    { id: 6, timestamp: new Date(Date.now() - 30000).toISOString(), level: 'error', source: 'nginx', message: 'Failed to bind upstream worker port 8105 (Connection refused - app restarting)' },
    { id: 7, timestamp: new Date(Date.now() - 15000).toISOString(), level: 'info', source: 'cron', message: 'Cron job run #104 finished in 420ms (exit code 0)' },
    { id: 8, timestamp: new Date(Date.now() - 5000).toISOString(), level: 'info', source: 'rpanel', message: 'Real-time WebSocket telemetry stream established' },
  ] as Array<{ id: number; timestamp: string; level: 'info' | 'warn' | 'error'; source: string; message: string }>,
}

const id = () => db.seq++
const token = () => ({
  access_token: 'rpanel-auth-access-token-' + Date.now(),
  token_type: 'Bearer',
  expires_at: Math.floor(Date.now() / 1000) + 86400,
  user: db.user,
})

const app = express()
app.use(cors())
app.use(cookieParser())
app.use(express.json())
app.use(express.text({ type: ['text/*', 'application/x-www-form-urlencoded'] }))

const router = express.Router()

// ── Auth ──
router.get('/auth/status', (_req, res) => {
  res.json({ needs_setup: db.needsSetup })
})

router.post('/auth/setup', (req, res) => {
  if (!db.needsSetup) {
    return res.status(403).json({ error: 'forbidden', message: '이미 설정이 완료되었습니다' })
  }
  const body = req.body || {}
  if ((body.password ?? '').length < 10) {
    return res.status(400).json({ error: 'bad_request', message: '비밀번호는 10자 이상이어야 합니다' })
  }
  db.needsSetup = false
  db.user = { id: 1, email: (body.email || 'admin@example.com').toLowerCase(), role: 'admin', created_at: now() }
  db.password = body.password
  db.loggedIn = true
  return res.status(201).json(token())
})

router.post('/auth/login', (req, res) => {
  const body = req.body || {}
  if (body.email !== db.user?.email || body.password !== db.password) {
    return res.status(400).json({ error: 'bad_request', message: '이메일 또는 비밀번호가 올바르지 않습니다' })
  }
  db.loggedIn = true
  return res.json(token())
})

router.post('/auth/refresh', (_req, res) => {
  if (db.loggedIn && db.user) {
    return res.json(token())
  }
  return res.status(401).json({ error: 'unauthorized', message: '세션이 만료되었습니다' })
})

router.post('/auth/logout', (_req, res) => {
  db.loggedIn = false
  return res.status(204).end()
})

router.post('/auth/ws-ticket', (_req, res) => {
  return res.json({ ticket: 'a'.repeat(48), expires_in: 30 })
})

// ── System ──
router.get('/system/info', (_req, res) => {
  res.json({
    version: PKG_VERSION,
    exec_mode: 'dry-run',
    host: { hostname: 'rpanel-cloud', os: 'Linux 24.04 Ubuntu LTS', kernel: '6.8.0', cpu_brand: 'Intel Xeon Platinum', cpu_count: 8 },
    app_port_range: [8100, 8999],
    boot_time: Math.floor(Date.now() / 1000) - 86400 * 5,
    panel_started_at: Math.floor(Date.now() / 1000) - 7200,
  })
})

router.get('/system/overview', (_req, res) => {
  res.json({
    projects: db.projects.length,
    apps: {
      total: db.apps.length,
      by_status: { running: db.apps.filter((a) => a.status === 'running').length },
    },
    deployments_24h: { total: db.deployments.length, failed: 0 },
    recent_deployments: db.deployments.map((d) => ({
      ...d,
      app_name: db.apps.find((a) => a.id === d.app_id)?.name,
      app_slug: db.apps.find((a) => a.id === d.app_id)?.slug || '',
    })),
  })
})

router.get('/audit', (_req, res) => {
  res.json([])
})

router.get('/system/version', (_req, res) => {
  res.json(db.version)
})

router.get('/system/edge', (_req, res) => {
  res.json({ edge_mode: 'direct', trusted_proxy: '', ssl_available: true })
})

router.get('/system/metrics', (req, res) => {
  const range = (req.query.range as string) || '30m'
  const n = ({ '30m': 30, '1h': 60, '6h': 360, '24h': 1440, '7d': 10080 } as Record<string, number>)[range] || 30
  const nowS = Math.floor(Date.now() / 1000)
  const step = Math.max(1, Math.ceil(n / 120))
  const points = []
  for (let i = n - 1; i >= 0; i -= step) {
    points.push({
      t: nowS - i * 60,
      c: 12 + ((i * 3) % 25),
      m: 5.5 * 2 ** 30,
      mt: 16 * 2 ** 30,
      l: [0.35, 0.28, 0.22],
      d: [['/', 38 * 2 ** 30, 200 * 2 ** 30]],
    })
  }
  res.json({ range, interval: 60, cpu_count: 8, points })
})

router.get('/logs/deployments', (_req, res) => {
  res.json(
    db.deployments.map((d) => ({
      ...d,
      app_name: db.apps.find((a) => a.id === d.app_id)?.name,
      app_slug: db.apps.find((a) => a.id === d.app_id)?.slug || '',
    })),
  )
})

router.get('/logs/system', (req, res) => {
  const kind = req.query.kind as string
  const lines =
    {
      panel: [
        `${new Date(Date.now() - 3600000).toISOString()} rpanel: starting rpanel ${PKG_VERSION}`,
        `${new Date(Date.now() - 3000000).toISOString()} rpanel: listening on http://0.0.0.0:${PORT}`,
        `${new Date().toISOString()} rpanel: all services operating normally`,
      ],
      nginx: [
        '127.0.0.1 - - [17/Oct/2026:12:00:00 +0000] "GET / HTTP/1.1" 200 412 "-" "Browser"',
      ],
      ssh: ['Accepted publickey for admin from 10.0.0.1 port 52341 ssh2'],
      mariadb: ['mysqld: ready for connections. Version: 10.11.13-MariaDB socket: /run/mysqld/mysqld.sock'],
      redis: ['Server initialized, ready to accept connections on port 6379'],
    }[kind] || []
  res.json({ kind, lines })
})

// ── System Events & Real-time Logs ──
router.get('/system/events', (req, res) => {
  let events = [...db.systemEvents]
  const level = req.query.level as string
  const source = req.query.source as string
  const q = (req.query.q as string || '').toLowerCase()
  const limit = Number(req.query.limit) || 100

  if (level && level !== 'all') {
    events = events.filter((e) => e.level === level)
  }
  if (source && source !== 'all') {
    events = events.filter((e) => e.source === source)
  }
  if (q) {
    events = events.filter((e) => e.message.toLowerCase().includes(q) || e.source.toLowerCase().includes(q))
  }
  res.json({
    events: events.slice(-limit),
    summary: {
      total: db.systemEvents.length,
      errors: db.systemEvents.filter((e) => e.level === 'error').length,
      warnings: db.systemEvents.filter((e) => e.level === 'warn').length,
      info: db.systemEvents.filter((e) => e.level === 'info').length,
    },
  })
})

router.post('/system/events', (req, res) => {
  const { level = 'info', source = 'system', message = 'Custom system test event' } = req.body || {}
  const event = {
    id: (db.systemEvents[db.systemEvents.length - 1]?.id || 0) + 1,
    timestamp: new Date().toISOString(),
    level,
    source,
    message,
  }
  db.systemEvents.push(event)
  if (db.systemEvents.length > 500) db.systemEvents.shift()
  res.status(201).json(event)
})

router.delete('/system/events', (_req, res) => {
  db.systemEvents = []
  res.status(204).end()
})

router.get('/system/processes', (_req, res) => {
  res.json({
    processes: [
      { pid: 1, name: 'rpanel', user: 'root', cpu: 0.8, mem_mb: 58.2, status: 'running', command: 'node server.ts --port 3000' },
      { pid: 14, name: 'nginx: master', user: 'root', cpu: 0.1, mem_mb: 14.5, status: 'running', command: 'nginx: master process /usr/sbin/nginx' },
      { pid: 15, name: 'nginx: worker', user: 'www-data', cpu: 0.3, mem_mb: 18.2, status: 'running', command: 'nginx: worker process' },
      { pid: 88, name: 'mariadbd', user: 'mysql', cpu: 0.4, mem_mb: 148.0, status: 'running', command: '/usr/sbin/mariadbd --basedir=/usr' },
      { pid: 92, name: 'redis-server', user: 'redis', cpu: 0.1, mem_mb: 22.4, status: 'running', command: '/usr/bin/redis-server 127.0.0.1:6379' },
      { pid: 104, name: 'cron', user: 'root', cpu: 0.0, mem_mb: 4.2, status: 'running', command: '/usr/sbin/cron -f' },
      { pid: 142, name: 'api.example.com', user: 'api_user', cpu: 1.2, mem_mb: 84.6, status: 'running', command: './target/release/api --port 8105' },
    ],
    summary: {
      total: 7,
      running: 7,
      sleeping: 0,
      zombie: 0,
    },
  })
})

// ── Notifications ──
router.get('/notifications', (req, res) => {
  const unreadOnly = req.query.unread === 'true'
  const items = [...db.notifications]
    .reverse()
    .filter((n) => !unreadOnly || !n.read_at)
    .slice(0, Number(req.query.limit || 50))
  res.json({ items, unread: db.notifications.filter((n) => !n.read_at).length })
})

router.delete('/notifications', (_req, res) => {
  db.notifications = []
  res.status(204).end()
})

router.post('/notifications/read', (req, res) => {
  const { all, ids } = req.body || {}
  for (const n of db.notifications) {
    if (all || ids?.includes(n.id)) n.read_at ??= now()
  }
  res.json({ unread: db.notifications.filter((n) => !n.read_at).length })
})

router.get('/notifications/settings', (_req, res) => {
  const events = Object.keys(db.notifySettings.events).map((key) => ({
    key,
    label: (
      {
        deploy_failed: '배포·적용 실패',
        deploy_success: '배포·적용 성공',
        backup_failed: '백업 실패',
        backup_success: '백업 완료',
        site_down: '사이트 응답 없음 · 복구',
        disk_high: '디스크 사용량 높음',
        mem_high: '메모리 사용량 높음',
        update_available: '새 버전 알림',
        runtime_install: '런타임 설치 결과',
        admin_login: '관리자 로그인',
      } as Record<string, string>
    )[key] || key,
    default: true,
  }))
  res.json({ settings: db.notifySettings, events })
})

router.put('/notifications/settings', (req, res) => {
  db.notifySettings = req.body
  const events = Object.keys(db.notifySettings.events).map((key) => ({ key, label: key, default: true }))
  res.json({ settings: db.notifySettings, events })
})

router.post('/notifications/test', (_req, res) => {
  db.notifications.push({
    id: id(),
    kind: 'test',
    level: 'info',
    title: '시험 알림입니다',
    params: {},
    body: 'Rpanel 알림이 정상적으로 수신되었습니다.',
    link: null,
    read_at: null,
    created_at: now(),
  })
  res.json({ results: [{ channel: 'in-app', ok: true, error: null }] })
})

// ── Runtimes ──
router.get('/runtimes', (_req, res) => {
  const usage: Record<string, number> = {}
  for (const a of db.apps) {
    const lang = a.kind === 'python' ? 'python' : a.kind === 'rust' ? 'rust' : 'node'
    const v = a.runtime_version ?? db.runtimes.find((r) => r.lang === lang && r.is_default)?.version
    usage[`${lang}:${v}`] = (usage[`${lang}:${v}`] ?? 0) + 1
  }
  res.json({
    runtimes: db.runtimes,
    candidates: {
      rust: ['stable', 'beta', 'nightly', '1.90', '1.89'],
      python: ['system', '3.14', '3.13', '3.12', '3.11'],
      node: ['system', '24', '22', '20'],
    },
    usage,
  })
})

router.post('/runtimes', (req, res) => {
  const body = req.body || {}
  const ex = db.runtimes.find((r) => r.lang === body.lang && r.version === body.version)
  if (ex) {
    if (body.make_default) {
      for (const x of db.runtimes) if (x.lang === ex.lang) x.is_default = x === ex
    }
    return res.json(ex)
  }
  const r = {
    id: id(),
    lang: body.lang,
    version: body.version,
    is_default: !!body.make_default,
    is_system: false,
    status: 'ready',
    detail: `${body.version}.0`,
    log: `$ install ${body.lang} ${body.version}\nOK`,
    updated_at: now(),
  }
  if (body.make_default) {
    for (const x of db.runtimes) if (x.lang === r.lang) x.is_default = false
  }
  db.runtimes.push(r)
  return res.status(202).json(r)
})

router.post('/runtimes/:id/default', (req, res) => {
  const r = db.runtimes.find((x) => x.id === Number(req.params.id))
  if (!r) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  for (const x of db.runtimes) if (x.lang === r.lang) x.is_default = x === r
  return res.json(r)
})

router.delete('/runtimes/:id', (req, res) => {
  const r = db.runtimes.find((x) => x.id === Number(req.params.id))
  if (!r) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  if (r.is_default || r.is_system) {
    return res.status(400).json({ error: 'bad_request', message: '기본 버전은 삭제할 수 없습니다' })
  }
  db.runtimes = db.runtimes.filter((x) => x !== r)
  return res.status(204).end()
})

// ── Users / Admins ──
router.get('/users', (_req, res) => {
  const list = [db.user, ...db.admins].map((u) => ({ last_login_at: null, ...u }))
  res.json({ users: list, me: db.user?.id })
})

router.post('/users', (req, res) => {
  const body = req.body || {}
  const u = {
    id: id(),
    email: (body.email || '').toLowerCase(),
    role: 'admin',
    created_at: now(),
    last_login_at: null,
  }
  db.admins.push(u)
  res.status(201).json(u)
})

router.delete('/users/:id', (req, res) => {
  const targetId = Number(req.params.id)
  if (targetId === db.user?.id) {
    return res.status(400).json({ error: 'bad_request', message: '자기 자신은 삭제할 수 없습니다' })
  }
  db.admins = db.admins.filter((x) => x.id !== targetId)
  res.status(204).end()
})

// ── Projects ──
router.get('/projects', (_req, res) => {
  res.json(
    db.projects.map((p) => ({
      ...p,
      app_count: db.apps.filter((a) => a.project_id === p.id).length,
      running_count: db.apps.filter((a) => a.project_id === p.id && a.status === 'running').length,
    })),
  )
})

router.post('/projects', (req, res) => {
  const body = req.body || {}
  const p = {
    id: id(),
    name: body.name || '새 프로젝트',
    description: body.description || '',
    created_at: now(),
    updated_at: now(),
  }
  db.projects.push(p)
  res.status(201).json(p)
})

router.get('/projects/:id', (req, res) => {
  const p = db.projects.find((x) => x.id === Number(req.params.id))
  if (!p) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  res.json(p)
})

router.post('/projects/:id/apps', (req, res) => {
  const body = req.body || {}
  const a = {
    id: id(),
    project_id: Number(req.params.id),
    ...body,
    slug: body.slug || body.name.toLowerCase().replace(/[^a-z0-9]+/g, '-'),
    port: body.kind === 'static' ? null : 8100 + db.apps.length,
    status: 'idle',
    source: 'git',
    ssl_mode: 'none',
    current_commit: null,
    current_release: null,
    created_at: now(),
    updated_at: now(),
  }
  db.apps.push(a)
  db.env[a.id] = []
  res.status(201).json(a)
})

// ── Sites ──
router.get('/sites', (_req, res) => {
  res.json(
    db.apps
      .filter((a) => a.domains)
      .flatMap((a) =>
        a.domains.split(' ').map((d, i) => ({
          domain: d,
          app_id: a.id,
          app_name: a.name,
          app_slug: a.slug,
          kind: a.kind,
          source: a.source ?? 'git',
          site_user: a.site_user ?? null,
          app_status: a.status,
          project_id: a.project_id,
          http_status: i === 0 ? 200 : 502,
          latency_ms: i === 0 ? 12 : null,
          error: i === 0 ? null : 'Bad Gateway',
          last_deploy_status: null,
          last_deploy_at: null,
        })),
      ),
  )
})

router.post('/sites', (req, res) => {
  const body = req.body || {}
  const domains = (body.domains || '').trim().toLowerCase().split(/[\s,]+/)
  const domain = domains[0] || 'site.example.com'
  const kind = body.type === 'html' ? 'static' : body.type || 'static'
  const user = body.type === 'proxy' ? null : body.site_user || domain.split('.')[0]
  const a = {
    id: id(),
    project_id: null,
    name: domain,
    slug: domain.replace(/\./g, '-'),
    kind,
    source: body.type === 'proxy' ? 'none' : 'files',
    git_url: null,
    git_branch: 'main',
    domains: domains.join(' '),
    site_user: user,
    site_root: user ? `/home/${user}/htdocs/${domain}` : null,
    web_root: '',
    proxy_target: body.proxy_target || null,
    start_cmd: body.start_cmd || null,
    build_cmd: null,
    health_path: '/',
    port: kind === 'rust' || kind === 'python' || kind === 'node' ? 8100 + db.apps.length : null,
    status: 'running',
    ssl_mode: 'none',
    force_https: false,
    client_max_body_mb: 64,
    current_release: 1,
    current_commit: null,
    redis_db: null,
    backup_schedule: 'off',
    backup_keep: 7,
    created_at: now(),
    updated_at: now(),
  }
  db.apps.push(a)
  db.env[a.id] = []
  db.files[a.id] = { [`htdocs/${domain}`]: { 'index.html': '<h1>안녕하세요! Rpanel 사이트입니다.</h1>' } }
  res.status(201).json({ app: a, deployment_id: null, site_user: user, generated_password: 'GeneratedPw123' })
})

// ── Apps & App sub-routes ──
router.get('/apps', (req, res) => {
  const pid = Number(req.query.project_id)
  res.json(db.apps.filter((a) => !pid || a.project_id === pid))
})

router.get('/apps/:id', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  res.json(a)
})

router.put('/apps/:id', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  Object.assign(a, req.body, { slug: a.slug, kind: a.kind })
  res.json(a)
})

router.put('/apps/:id/runtime', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  a.runtime_version = req.body?.version ?? null
  res.json(a)
})

router.get('/apps/:id/env', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  const managed = [
    ...db.dbs.filter((d) => d.app_id === a.id).map((d) => ({ key: d.env_key, value: `mysql://${d.username}:secretpw@127.0.0.1:3306/${d.name}`, source: `MariaDB · ${d.name}` })),
    ...(a.redis_db != null ? [{ key: 'REDIS_URL', value: `redis://:pw@127.0.0.1:6379/${a.redis_db}`, source: `Redis · DB ${a.redis_db}` }] : []),
  ]
  res.json({ vars: db.env[a.id] || [], managed })
})

router.put('/apps/:id/env', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  db.env[a.id] = req.body.vars || []
  res.json({ vars: db.env[a.id] })
})

router.get('/apps/:id/service', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  res.json({ active: a.status === 'running' ? 'active' : 'inactive', unit: `rpanel-app-${a.slug}.service` })
})

router.post('/apps/:id/service', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  const action = req.body?.action
  a.status = action === 'stop' ? 'stopped' : 'running'
  res.json({ ok: true })
})

router.get('/apps/:id/ssl', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (!a) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  res.json({
    mode: a.ssl_mode || 'none',
    force_https: a.force_https || false,
    certificates: [],
    edge_mode: 'direct',
    domains: (a.domains || '').split(' '),
  })
})

router.get('/apps/:id/deploy-key', (_req, res) => {
  res.json({ public_key: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExampleDeployKey rpanel-key' })
})

router.post('/apps/:id/deploy-key', (_req, res) => {
  res.json({ public_key: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExampleDeployKey rpanel-key' })
})

router.get('/apps/:id/webhook', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  res.json({ path: `/api/hooks/${a?.slug}`, token: 'hooktoken123', branch: a?.git_branch || 'main' })
})

router.get('/apps/:id/deployments', (req, res) => {
  const appId = Number(req.params.id)
  res.json(db.deployments.filter((d) => d.app_id === appId).reverse())
})

router.post('/apps/:id/deploy', (req, res) => {
  const appId = Number(req.params.id)
  const d = {
    id: id(),
    app_id: appId,
    status: 'running',
    triggered_by: 1,
    commit_sha: 'c0ff33a',
    error: null,
    started_at: now(),
    finished_at: null,
    created_at: now(),
  }
  db.deployments.push(d)
  res.status(202).json({ deployment_id: d.id })
})

router.get('/deployments/:id', (req, res) => {
  const d = db.deployments.find((x) => x.id === Number(req.params.id))
  if (!d) return res.status(404).json({ error: 'not_found', message: '찾을 수 없습니다' })
  const a = db.apps.find((x) => x.id === d.app_id)
  res.json({ ...d, log: null, app_name: a?.name, app_slug: a?.slug })
})

// ── Site Accounts ──
router.get('/apps/:id/accounts', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  const accounts = db.accounts.filter((x) => x.app_id === a?.id)
  res.json({ accounts, site_user: a?.site_user, site_root: a?.site_root })
})

router.post('/apps/:id/accounts', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  const body = req.body || {}
  const acc = {
    id: id(),
    app_id: a?.id,
    username: body.username,
    is_primary: false,
    access: body.access || 'sftp',
    enabled: true,
    key_count: (body.ssh_keys || '').split('\n').filter(Boolean).length,
    note: body.note ?? null,
    created_at: now(),
  }
  db.accounts.push(acc)
  res.status(201).json({ account: acc, password: body.password || 'AutoGen3rated-pw-01' })
})

// ── Files ──
router.get('/apps/:id/files', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  const dir = (req.query.path as string) || ''
  const tree = (a ? db.files[a.id] : {}) || {}
  const entries: any[] = Object.entries(tree[dir] || {}).map(([name, content]) => ({
    name,
    kind: 'file',
    size: content.length,
    mode: '0644',
    mtime: Math.floor(Date.now() / 1000),
  }))
  for (const k of Object.keys(tree)) {
    if (k.startsWith(dir ? dir + '/' : '') && !k.slice(dir ? dir.length + 1 : 0).includes('/')) {
      entries.unshift({ name: k.slice(dir ? dir.length + 1 : 0), kind: 'dir', size: 0, mode: '0750', mtime: Math.floor(Date.now() / 1000) })
    }
  }
  res.json({ path: dir, entries, root: `htdocs/${a?.domains.split(' ')[0]}`, user: a?.site_user })
})

router.get('/apps/:id/files/content', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  const filePath = (req.query.path as string) || ''
  const lastSlash = filePath.lastIndexOf('/')
  const d = lastSlash >= 0 ? filePath.slice(0, lastSlash) : ''
  const n = lastSlash >= 0 ? filePath.slice(lastSlash + 1) : filePath
  const content = a ? db.files[a.id]?.[d]?.[n] : null
  if (content == null) {
    return res.status(400).json({ message: '없는 파일입니다' })
  }
  res.json({ path: filePath, content, mode: '0644' })
})

router.put('/apps/:id/files/content', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  const { path: filePath, content } = req.body || {}
  const lastSlash = filePath.lastIndexOf('/')
  const d = lastSlash >= 0 ? filePath.slice(0, lastSlash) : ''
  const n = lastSlash >= 0 ? filePath.slice(lastSlash + 1) : filePath
  if (a) {
    db.files[a.id] ||= {}
    db.files[a.id][d] ||= {}
    db.files[a.id][d][n] = content
  }
  res.json({ ok: true, bytes: String(content?.length || 0) })
})

router.post('/apps/:id/files/op', (req, res) => {
  res.json({ ok: true })
})

router.post('/apps/:id/files/upload', (req, res) => {
  res.json({ ok: true, bytes: 1024 })
})

// ── Databases ──
const dbUrl = (name: string, user: string, pw = 'secretpw') => `mysql://${user}:${pw}@127.0.0.1:3306/${name}`
const redisUrl = (u: any) => `redis://${u.username}:${u.password}@127.0.0.1:6379/${u.db_index}`

router.get('/databases', (_req, res) => {
  res.json({
    databases: db.dbs.map((d) => ({
      ...d,
      app_name: db.apps.find((a) => a.id === d.app_id)?.name ?? null,
      users: [
        { id: null, username: d.username, privileges: 'all', primary: true, url: dbUrl(d.name, d.username) },
        ...db.dbUsers.filter((u) => u.database_id === d.id).map((u) => ({ ...u, primary: false, url: dbUrl(d.name, u.username, u.password) })),
      ],
    })),
    redis: db.apps.filter((a) => a.redis_db != null).map((a) => ({ app_id: a.id, app_name: a.name, app_slug: a.slug, db: a.redis_db, keys: 2 })),
    redis_users: db.redisUsers.map((u) => ({ ...u, password: undefined, url: redisUrl(u) })),
    redis_range: [4, 15],
    panel_redis_db: 10,
  })
})

router.post('/databases', (req, res) => {
  const body = req.body || {}
  const d = {
    id: id(),
    app_id: body.app_id ?? null,
    name: body.name,
    username: body.username || body.name,
    env_key: body.env_key || 'DATABASE_URL',
    created_at: now(),
  }
  db.dbs.push(d)
  res.status(201).json({ ...d, url: dbUrl(d.name, d.username) })
})

router.delete('/databases/:id', (req, res) => {
  db.dbs = db.dbs.filter((x) => x.id !== Number(req.params.id))
  res.status(204).end()
})

router.get('/apps/:id/databases', (req, res) => {
  const appId = Number(req.params.id)
  const a = db.apps.find((x) => x.id === appId)
  res.json({
    databases: db.dbs.filter((d) => d.app_id === appId).map((d) => ({ ...d, url: dbUrl(d.name, d.username) })),
    redis: a?.redis_db != null ? { db: a.redis_db, url: `redis://:pw@127.0.0.1:6379/${a.redis_db}`, keys: 2 } : null,
    suggested_name: a?.slug.replace(/-/g, '_') || 'app_db',
  })
})

router.post('/apps/:id/databases', (req, res) => {
  const appId = Number(req.params.id)
  const body = req.body || {}
  const d = {
    id: id(),
    app_id: appId,
    name: body.name,
    username: body.username || body.name,
    env_key: body.env_key || 'DATABASE_URL',
    created_at: now(),
  }
  db.dbs.push(d)
  res.status(201).json(d)
})

router.post('/apps/:id/redis', (req, res) => {
  const a = db.apps.find((x) => x.id === Number(req.params.id))
  if (a) a.redis_db = 4
  res.json({ db: 4 })
})

// ── Backups ──
router.get('/backups', (req, res) => {
  const appId = Number(req.query.app_id)
  res.json(db.backups.filter((b) => !appId || b.app_id === appId))
})

router.post('/apps/:id/backups', (req, res) => {
  const appId = Number(req.params.id)
  const a = db.apps.find((x) => x.id === appId)
  const b = {
    id: id(),
    app_id: appId,
    app_name: a?.name || null,
    app_slug: a?.slug || null,
    kind: 'data',
    target: 'htdocs',
    file: `20261017-${Date.now()}-data.tar.gz`,
    size_bytes: 12288,
    status: 'success',
    error: null,
    trigger_by: 'manual',
    created_at: now(),
    finished_at: now(),
  }
  db.backups.unshift(b)
  res.status(202).json({ backup_ids: [b.id] })
})

router.get('/backup-jobs', (req, res) => {
  const appId = Number(req.query.app_id)
  const jobView = (j: any) => ({
    ...j,
    target_name: j.target === 'app' ? db.apps.find((a) => a.id === j.app_id)?.name : '모든 앱·데이터베이스',
    next_run_at: j.enabled ? `2026-10-18T03:00:00Z` : null,
    last_status: 'success',
    backup_count: 1,
  })
  res.json({ jobs: db.jobs.filter((j) => !appId || j.app_id === appId || j.target === 'all').map(jobView), timezone: '+09:00' })
})

router.post('/backup-jobs', (req, res) => {
  const body = req.body || {}
  const j = { id: id(), ...body, name: body.name || '정기 백업', last_run_at: null, created_at: now() }
  db.jobs.push(j)
  res.status(201).json(j)
})

router.post('/backup-jobs/:id/run', (req, res) => {
  res.status(202).json({ started: 1 })
})

// ── Cron ──
router.get('/apps/:id/cron', (req, res) => {
  const appId = Number(req.params.id)
  res.json(db.crons.filter((c) => c.app_id === appId))
})

router.post('/apps/:id/cron', (req, res) => {
  const appId = Number(req.params.id)
  const a = db.apps.find((x) => x.id === appId)
  const c = { id: id(), app_id: appId, ...req.body, unit: `rpanel-cron-${a?.slug}-x`, status: 'active' }
  db.crons.push(c)
  res.status(201).json({ id: c.id })
})

router.post('/cron/preview', (req, res) => {
  res.json({ valid: true, next: 'Tomorrow at 03:00:00 KST', error: null })
})

// ── DB Engines ──
router.get('/db-engines', (_req, res) => {
  res.json({
    engines: db.engines.map((e) => {
      const cur = e.list.findIndex((x) => x[0] === e.series)
      return {
        user_dbs: 1,
        engine: e.engine,
        label: e.label,
        installed: e.installed,
        version: e.version,
        series: e.series,
        job: e.job,
        last: null,
        versions: e.list.map(([v, note], i) => ({
          version: v,
          note,
          current: i === cur,
          available: i !== cur,
          direction: i === cur ? null : i > cur ? 'up' : 'down',
          reason: i === cur ? '이미 이 버전입니다' : null,
        })),
      }
    }),
  })
})

// ── Storage ──
router.get('/system/storage', (_req, res) => {
  const item = (kind: string) => ({
    kind,
    label: kind === 'backups' ? '백업' : '프로젝트 파일',
    path: db.storage[kind] ?? (kind === 'backups' ? '/var/lib/rpanel/backups' : '/var/lib/rpanel'),
    custom: db.storage[kind] != null,
    default_path: kind === 'backups' ? '/var/lib/rpanel/backups' : '/var/lib/rpanel',
    total_bytes: 100e9,
    free_bytes: 42e9,
  })
  res.json({ items: [item('backups'), item('projects')], job: null, last: null, busy: null })
})

// ── Panel Access & Domain ──
router.get('/system/panel-access', (_req, res) => {
  res.json({ allow: db.panelAllow, my_ip: '203.0.113.9', restricted: db.panelAllow.length > 0 })
})

router.get('/system/panel-domain', (_req, res) => {
  res.json({ domain: db.panelDomain, email: null, url: db.panelDomain ? `https://${db.panelDomain}` : null })
})

// ── MongoDB ──
const mongoUrl = (d: any) => `mongodb://${d.username}:pw@127.0.0.1:27017/${d.name}?authSource=${d.name}`
router.get('/mongo', (_req, res) => {
  res.json({
    configured: true,
    available: true,
    version: '8.0.4',
    error: null,
    databases: db.mongos.map((d) => ({ ...d, app_name: db.apps.find((a) => a.id === d.app_id)?.name ?? null, url: mongoUrl(d), size_bytes: 8192, users: [] })),
  })
})

router.post('/mongo', (req, res) => {
  const body = req.body || {}
  const d = { id: id(), app_id: body.app_id ?? null, name: body.name, username: body.username || body.name, env_key: body.env_key || 'MONGODB_URL', created_at: now() }
  db.mongos.push(d)
  res.status(201).json({ database: d, url: mongoUrl(d), password: 'pw' })
})

// ── DbWeb Manager (MariaDB / Redis / Mongo) ──
router.get('/dbweb/redis', (_req, res) => {
  res.json({
    databases: db.apps.filter((a) => a.redis_db != null).map((a) => ({ db: a.redis_db, app: a.name, keys: Object.keys(db.redisKeys).length })),
  })
})

router.get('/dbweb/redis/:db/keys', (req, res) => {
  const pattern = (req.query.pattern as string) || '*'
  const re = new RegExp('^' + pattern.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '$')
  res.json({
    db: Number(req.params.db),
    cursor: 0,
    keys: Object.keys(db.redisKeys).filter((k) => re.test(k)).map((k) => ({ key: k, type: 'string', ttl: null })),
  })
})

router.get('/dbweb/redis/:db/key', (req, res) => {
  const k = req.query.k as string
  if (k in db.redisKeys) {
    res.json({ key: k, type: 'string', ttl: null, value: db.redisKeys[k] })
  } else {
    res.status(404).json({ message: '찾을 수 없습니다' })
  }
})

router.post('/dbweb/redis/:db/set', (req, res) => {
  db.redisKeys[req.body.key] = req.body.value ?? ''
  res.json({ ok: true })
})

router.post('/dbweb/redis/:db/delete', (req, res) => {
  delete db.redisKeys[req.body.key]
  res.json({ ok: true })
})

router.all('/dbweb/:scope/:id/query', (req, res) => {
  const sql = (req.body?.sql || '').trim()
  if (/^select/i.test(sql)) {
    res.json({ columns: db.dbweb.columns, rows: db.dbweb.rows, affected: 0, truncated: false, elapsed_ms: 2 })
  } else {
    res.json({ columns: [], rows: [], affected: 1, truncated: false, elapsed_ms: 3 })
  }
})

router.all('/dbweb/:scope/:id/table/:table', (_req, res) => {
  res.json({
    table: 'orders',
    columns: [
      { name: 'id', type: 'int(10) unsigned', nullable: false, key: 'PRI', default: null, extra: 'auto_increment', comment: '' },
      { name: 'name', type: 'varchar(40)', nullable: false, key: '', default: null, extra: '', comment: '' },
      { name: 'price', type: 'decimal(10,2)', nullable: true, key: '', default: null, extra: '', comment: '' },
      { name: 'created_at', type: 'datetime', nullable: true, key: '', default: null, extra: '', comment: '' },
    ],
    primary_key: ['id'],
    rows: db.dbweb.rows,
    total: db.dbweb.rows.length,
    page: 1,
    size: 50,
    order: 'id',
    desc: false,
  })
})

router.all('/dbweb/:scope/:id', (_req, res) => {
  res.json({
    database: 'api_db',
    user: 'api_user',
    tables: [{ name: 'orders', type: 'BASE TABLE', rows: db.dbweb.rows.length, size: 16384 }],
  })
})

// Mount router under /api
app.use('/api', router)

// Create HTTP server
const server = http.createServer(app)

// WebSocket setup
const wss = new WebSocketServer({ noServer: true })

server.on('upgrade', (request, socket, head) => {
  const url = new URL(request.url || '', `http://${request.headers.host || 'localhost'}`)
  if (url.pathname.startsWith('/api/ws') || url.pathname.startsWith('/ws')) {
    wss.handleUpgrade(request, socket, head, (ws) => {
      wss.emit('connection', ws, request)
    })
  }
})

wss.on('connection', (ws: WebSocket, request: http.IncomingMessage) => {
  const url = new URL(request.url || '', `http://${request.headers.host || 'localhost'}`)
  const pathname = url.pathname

  if (pathname.includes('/ws/metrics')) {
    const sendMetrics = () => {
      if (ws.readyState !== WebSocket.OPEN) return
      ws.send(
        JSON.stringify({
          type: 'metrics',
          data: {
            ts: Date.now(),
            cpu_percent: 18.5 + (Math.sin(Date.now() / 5000) * 12),
            cpu_per_core: [15, 22, 19, 24],
            mem_total: 16 * 1024 ** 3,
            mem_used: (5.2 + Math.random() * 0.4) * 1024 ** 3,
            swap_total: 0,
            swap_used: 0,
            load: [0.38, 0.32, 0.28],
            uptime: 95420,
            disks: [{ mount: '/', total: 200 * 1024 ** 3, used: 38 * 1024 ** 3 }],
          },
        }),
      )
    }
    sendMetrics()
    const timer = setInterval(sendMetrics, 1500)
    ws.on('close', () => clearInterval(timer))
    return
  }

  const deployMatch = pathname.match(/\/ws\/deployments\/(\d+)/)
  if (deployMatch) {
    const depId = Number(deployMatch[1])
    const lines = [
      `Rpanel 배포 #${depId}`,
      '▶ 저장소 소스 동기화 중...',
      '$ git clone --depth 1 --branch main https://github.com/example/repo /var/lib/rpanel/builds/repo',
      '✔ 소스 체크아웃 완료',
      '▶ 빌드 파이프라인 실행',
      '   Compiling crates v0.1.0',
      '   Finished release [optimized] target(s) in 3.42s',
      '▶ 헬스체크 검증 중',
      'GET /health -> 200 OK',
      '✔ 서비스 정상 가동 완료',
    ]
    let i = 0
    const timer = setInterval(() => {
      if (ws.readyState !== WebSocket.OPEN) {
        clearInterval(timer)
        return
      }
      if (i < lines.length) {
        ws.send(JSON.stringify({ type: 'line', seq: i + 1, text: lines[i] }))
        i++
      } else {
        clearInterval(timer)
        const d = db.deployments.find((x) => x.id === depId)
        if (d) {
          d.status = 'success'
          d.finished_at = now()
        }
        ws.send(JSON.stringify({ type: 'status', status: 'success' }))
        setTimeout(() => ws.close(), 500)
      }
    }, 200)
    return
  }

  ws.on('message', (_msg) => {})
})

// Setup Vite in Dev or Static serving in Production
async function startServer() {
  if (!isProd) {
    const { createServer: createViteServer } = await import('vite')
    const vite = await createViteServer({
      server: {
        middlewareMode: true,
        host: HOST,
        port: PORT,
        allowedHosts: true,
      },
      appType: 'spa',
    })
    app.use(vite.middlewares)
    app.use('*', async (req, res, next) => {
      const url = req.originalUrl
      try {
        let template = fs.readFileSync(path.resolve(__dirname, 'index.html'), 'utf-8')
        template = await vite.transformIndexHtml(url, template)
        res.status(200).set({ 'Content-Type': 'text/html' }).end(template)
      } catch (e) {
        next(e)
      }
    })
  } else {
    app.use(express.static(path.resolve(__dirname, 'dist')))
    app.get('*', (_req: Request, res: Response) => {
      res.sendFile(path.resolve(__dirname, 'dist', 'index.html'))
    })
  }

  server.listen(PORT, HOST, () => {
    console.log(`[Rpanel] Server started on http://${HOST}:${PORT} (mode: ${isProd ? 'production' : 'development'})`)
  })
}

startServer()
