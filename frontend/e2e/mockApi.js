// Playwright용 가짜 백엔드. 실제 API와 같은 모양의 응답을 메모리에서 돌려줍니다.

export function createMockBackend({ needsSetup = false } = {}) {
  const now = () => new Date().toISOString()
  const db = {
    needsSetup,
    user: needsSetup ? null : { id: 1, email: 'admin@example.com', role: 'admin', created_at: now() },
    password: 'correct-horse-1',
    loggedIn: false,
    projects: [],
    apps: [],
    env: {},
    deployments: [],
    dbs: [],
    backups: [],
    crons: [],
    files: {},
    ssl: {},
    seq: 1,
  }
  const id = () => db.seq++

  const json = (route, status, body) =>
    route.fulfill({ status, contentType: 'application/json', body: body === undefined ? '' : JSON.stringify(body) })
  const token = () => ({ access_token: 'test-access-token', token_type: 'Bearer', expires_at: Date.now() / 1000 + 900, user: db.user })

  async function handle(route) {
    const req = route.request()
    const url = new URL(req.url())
    const path = url.pathname.replace(/^\/api/, '')
    const method = req.method()
    let body
    try {
      body = req.postData() ? JSON.parse(req.postData()) : undefined
    } catch {
      body = undefined // 파일 업로드 (원본 바이트)
    }
    const authed = req.headers()['authorization'] === 'Bearer test-access-token'

    // ── auth ──
    if (path === '/auth/status') return json(route, 200, { needs_setup: db.needsSetup })
    if (path === '/auth/setup' && method === 'POST') {
      if (!db.needsSetup) return json(route, 403, { error: 'forbidden', message: '이미 설정이 완료되었습니다' })
      if ((body.password ?? '').length < 10) return json(route, 400, { error: 'bad_request', message: '비밀번호는 10자 이상이어야 합니다' })
      db.needsSetup = false
      db.user = { id: 1, email: body.email.toLowerCase(), role: 'admin', created_at: now() }
      db.password = body.password
      db.loggedIn = true
      return json(route, 201, token())
    }
    if (path === '/auth/login' && method === 'POST') {
      if (body.email !== db.user?.email || body.password !== db.password)
        return json(route, 400, { error: 'bad_request', message: '이메일 또는 비밀번호가 올바르지 않습니다' })
      db.loggedIn = true
      return json(route, 200, token())
    }
    if (path === '/auth/refresh') return db.loggedIn ? json(route, 200, token()) : json(route, 401, { error: 'unauthorized', message: '세션이 만료되었습니다' })
    if (path === '/auth/logout') {
      db.loggedIn = false
      return json(route, 204)
    }
    if (!authed) return json(route, 401, { error: 'unauthorized', message: '인증이 필요합니다' })
    if (path === '/auth/ws-ticket') return json(route, 200, { ticket: 'a'.repeat(48), expires_in: 30 })

    // ── system ──
    if (path === '/system/info')
      return json(route, 200, {
        version: '0.1.0',
        exec_mode: 'dry-run',
        host: { hostname: 'app-server', os: 'Linux 24.04 Ubuntu 24.04.3 LTS', kernel: '6.8.0', cpu_brand: 'Intel Xeon E5-2690', cpu_count: 8 },
        app_port_range: [8100, 8999],
      })
    if (path === '/system/overview')
      return json(route, 200, {
        projects: db.projects.length,
        apps: { total: db.apps.length, by_status: {} },
        deployments_24h: { total: db.deployments.length, failed: 0 },
        recent_deployments: db.deployments.map((d) => ({ ...d, app_name: db.apps.find((a) => a.id === d.app_id)?.name, app_slug: '' })),
      })
    if (path === '/audit') return json(route, 200, [])

    // ── projects ──
    if (path === '/projects' && method === 'GET')
      return json(route, 200, db.projects.map((p) => ({ ...p, app_count: db.apps.filter((a) => a.project_id === p.id).length, running_count: 0 })))
    if (path === '/projects' && method === 'POST') {
      if (db.projects.some((p) => p.name === body.name)) return json(route, 409, { error: 'conflict', message: '이미 존재하는 값입니다' })
      const p = { id: id(), name: body.name, description: body.description ?? '', created_at: now(), updated_at: now() }
      db.projects.push(p)
      return json(route, 201, p)
    }
    let m = path.match(/^\/projects\/(\d+)$/)
    if (m) {
      const p = db.projects.find((x) => x.id === Number(m[1]))
      return p ? json(route, 200, p) : json(route, 404, { error: 'not_found', message: '찾을 수 없습니다' })
    }
    m = path.match(/^\/projects\/(\d+)\/apps$/)
    if (m && method === 'POST') {
      if (!/^(https?|ssh):\/\/|^[\w.-]+@/.test(body.git_url)) return json(route, 400, { error: 'bad_request', message: 'Git URL 형식이 올바르지 않습니다' })
      const a = {
        id: id(),
        project_id: Number(m[1]),
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
      return json(route, 201, a)
    }

    if (path === '/system/php') return json(route, 200, { versions: ['8.2', '8.3'] })
    if (path === '/system/edge') return json(route, 200, { edge_mode: 'direct', trusted_proxy: '', ssl_available: true })
    if (path === '/sites' && method === 'POST') {
      const domains = body.domains.trim().toLowerCase().split(/[\s,]+/)
      if (db.apps.some((a) => (a.domains || '').split(' ').some((d) => domains.includes(d))))
        return json(route, 409, { error: 'conflict', message: `도메인 ${domains[0]} 은(는) 이미 사용 중입니다` })
      const kind = body.type === 'html' ? 'static' : body.type
      const user = body.type === 'proxy' ? null : body.site_user || domains[0].split('.')[0]
      const a = {
        id: id(), project_id: null, name: domains[0], slug: domains[0].replace(/\./g, '-'), kind,
        source: body.type === 'proxy' ? 'none' : 'files', git_url: null, git_branch: 'main', domains: domains.join(' '),
        site_user: user, site_root: user ? `/home/${user}/htdocs/${domains[0]}` : null, web_root: '',
        php_version: body.php_version || null, php_memory_mb: 256, php_upload_mb: 64, php_max_exec: 60, php_max_children: 10,
        proxy_target: body.proxy_target || null, start_cmd: body.start_cmd || null, build_cmd: null, health_path: '/',
        port: kind === 'node' ? 8150 : null, status: 'running', ssl_mode: 'none', force_https: false, client_max_body_mb: 64,
        current_release: 1, redis_db: null, backup_schedule: 'off', backup_keep: 7,
      }
      db.apps.push(a)
      db.env[a.id] = []
      db.files[a.id] = { [`htdocs/${domains[0]}`]: { 'index.php': '<?php echo "hi";' } }
      return json(route, 201, { app: a, deployment_id: null, site_user: user, generated_password: body.password ? null : 'Gen3ratedPassw0rd' })
    }

    // ── apps ──
    if (path === '/apps') {
      const pid = Number(url.searchParams.get('project_id'))
      return json(route, 200, db.apps.filter((a) => !pid || a.project_id === pid))
    }
    m = path.match(/^\/apps\/(\d+)\/(files\/content|files\/op|files\/upload|ssl\/custom|ssl\/letsencrypt|ssl\/force|site-user\/password)$/)
    if (m) {
      const a = db.apps.find((x) => x.id === Number(m[1]))
      const tree = (db.files[a.id] ||= {})
      const split = (p) => [p.slice(0, p.lastIndexOf('/')), p.slice(p.lastIndexOf('/') + 1)]
      if (m[2] === 'files/content' && method === 'GET') {
        const [d, n] = split(url.searchParams.get('path'))
        const c = tree[d]?.[n]
        return c == null ? json(route, 400, { message: '없는 경로입니다' }) : json(route, 200, { path: url.searchParams.get('path'), content: c, mode: '0644' })
      }
      if (m[2] === 'files/content' && method === 'PUT') {
        const [d, n] = split(body.path)
        ;(tree[d] ||= {})[n] = body.content
        return json(route, 200, { ok: true, bytes: String(body.content.length) })
      }
      if (m[2] === 'files/upload') {
        const [d, n] = split(url.searchParams.get('path'))
        ;(tree[d] ||= {})[n] = req.postData() || ''
        return json(route, 200, { ok: true, bytes: (req.postData() || '').length })
      }
      if (m[2] === 'files/op') {
        if (body.op === 'mkdir') tree[body.path] = {}
        if (body.op === 'rm') {
          const [d, n] = split(body.path)
          delete tree[d]?.[n]
          delete tree[body.path]
        }
        return json(route, 200, { ok: true })
      }
      if (m[2] === 'ssl/custom') {
        if (!body.certificate.includes('BEGIN CERTIFICATE')) return json(route, 400, { message: 'PEM 형식의 인증서와 개인키를 붙여넣으세요' })
        Object.assign(a, { ssl_mode: 'custom', force_https: body.force_https })
        return json(route, 200, { ok: true })
      }
      if (m[2] === 'ssl/letsencrypt') return json(route, 400, { message: "Let's Encrypt 발급 실패 — 도메인 DNS 가 이 서버를 가리키는지 확인하세요" })
      if (m[2] === 'site-user/password') return body.password.length < 10 ? json(route, 400, { message: '비밀번호는 10자 이상이어야 합니다' }) : json(route, 200, { ok: true })
      return json(route, 200, { ok: true })
    }
    m = path.match(/^\/apps\/(\d+)(\/[\w-]+)?$/)
    if (m) {
      const a = db.apps.find((x) => x.id === Number(m[1]))
      if (!a) return json(route, 404, { error: 'not_found', message: '찾을 수 없습니다' })
      const sub = m[2]
      if (!sub && method === 'GET') return json(route, 200, a)
      if (!sub && method === 'PUT') {
        Object.assign(a, body, { slug: a.slug, kind: a.kind })
        return json(route, 200, a)
      }
      const managed = () => [
        ...db.dbs.filter((d) => d.app_id === a.id).map((d) => ({ key: d.env_key, value: `mysql://${d.username}:secretpw@127.0.0.1:3306/${d.name}`, source: `MariaDB · ${d.name}` })),
        ...(a.redis_db != null ? [{ key: 'REDIS_URL', value: `redis://:pw@127.0.0.1:6379/${a.redis_db}`, source: `Redis · DB ${a.redis_db}` }] : []),
      ]
      if (sub === '/files') {
        const dir = url.searchParams.get('path') || ''
        const tree = db.files[a.id] || {}
        const entries = Object.entries(tree[dir] || {}).map(([name, content]) => ({ name, kind: 'file', size: content.length, mode: '0644', mtime: 1792100000 }))
        for (const k of Object.keys(tree)) if (k.startsWith(dir + '/') && !k.slice(dir.length + 1).includes('/')) entries.unshift({ name: k.slice(dir.length + 1), kind: 'dir', size: 0, mode: '0750', mtime: 1792100000 })
        return json(route, 200, { path: dir, entries, root: `htdocs/${a.domains.split(' ')[0]}`, user: a.site_user })
      }
      if (sub === '/content' || sub === '/files-content') return json(route, 404, {})
      if (sub === '/ssl' && method === 'GET') return json(route, 200, { mode: a.ssl_mode, force_https: a.force_https, certificates: a.ssl_mode === 'none' ? [] : [{ path: 'x', not_after: 'Jan 14 00:00:00 2027 GMT', issuer: 'CN = Test CA', domains: a.domains.split(' ') }], edge_mode: 'direct', domains: a.domains.split(' ') })
      if (sub === '/env' && method === 'GET') return json(route, 200, { vars: db.env[a.id], managed: managed() })
      if (sub === '/databases' && method === 'GET')
        return json(route, 200, {
          databases: db.dbs.filter((d) => d.app_id === a.id).map((d) => ({ ...d, url: `mysql://${d.username}:secretpw@127.0.0.1:3306/${d.name}` })),
          redis: a.redis_db != null ? { db: a.redis_db, url: `redis://:pw@127.0.0.1:6379/${a.redis_db}`, keys: 0 } : null,
          suggested_name: a.slug.replace(/-/g, '_'),
        })
      if (sub === '/databases' && method === 'POST') {
        const name = body.name || a.slug.replace(/-/g, '_')
        if (db.dbs.some((d) => d.name === name)) return json(route, 409, { error: 'conflict', message: '이미 사용 중인 DB 이름입니다' })
        const d = { id: id(), app_id: a.id, name, username: body.username || name, env_key: body.env_key || 'DATABASE_URL', created_at: now() }
        db.dbs.push(d)
        return json(route, 201, d)
      }
      if (sub === '/redis' && method === 'POST') {
        a.redis_db = 4
        return json(route, 200, { db: 4 })
      }
      if (sub === '/backups' && method === 'POST') {
        const stamp = '20261013-030000'
        for (const d of db.dbs.filter((x) => x.app_id === a.id))
          db.backups.unshift({ id: id(), app_id: a.id, app_name: a.name, app_slug: a.slug, kind: 'db', target: d.name, file: `${stamp}-db-${d.name}.sql.gz`, size_bytes: 20480, status: 'success', error: null, trigger_by: 'manual', created_at: now(), finished_at: now() })
        db.backups.unshift({ id: id(), app_id: a.id, app_name: a.name, app_slug: a.slug, kind: 'data', target: 'data', file: `${stamp}-data.tar.gz`, size_bytes: 1024, status: 'success', error: null, trigger_by: 'manual', created_at: now(), finished_at: now() })
        return json(route, 202, { backup_ids: [] })
      }
      if (sub === '/backup-settings') {
        Object.assign(a, { backup_schedule: body.schedule, backup_keep: body.keep })
        return json(route, 200, { ...body, hour: 3 })
      }
      if (sub === '/cron' && method === 'GET') return json(route, 200, db.crons.filter((c) => c.app_id === a.id))
      if (sub === '/cron' && method === 'POST') {
        if (/[;$`]/.test(body.schedule)) return json(route, 400, { error: 'bad_request', message: '일정 형식이 올바르지 않습니다' })
        const c = { id: id(), app_id: a.id, ...body, unit: `rpanel-cron-${a.slug}-x`, status: null }
        db.crons.push(c)
        return json(route, 201, { id: c.id })
      }
      if (sub === '/nginx-logs')
        return json(route, 200, {
          kind: 'access',
          lines: ['203.0.113.5 - - [13/Oct/2026:11:00:00 +0900] "GET / HTTP/1.1" 200 512 "-" "Mozilla"'],
          stats: { total: 4, by_class: { '2xx': 3, '5xx': 1 }, top_paths: [['/', 3], ['/api', 1]], top_ips: [['203.0.113.5', 4]], bytes: 2048 },
        })
      if (sub === '/env' && method === 'PUT') {
        const bad = body.vars.find((v) => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(v.key))
        if (bad) return json(route, 400, { error: 'bad_request', message: `환경변수 이름이 올바르지 않습니다: ${bad.key}` })
        db.env[a.id] = [...body.vars].sort((x, y) => x.key.localeCompare(y.key))
        return json(route, 200, { vars: db.env[a.id] })
      }
      if (sub === '/service' && method === 'GET') return json(route, 200, { active: 'inactive', unit: `rpanel-app-${a.slug}.service` })
      if (sub === '/deploy-key' && method === 'GET') return json(route, 200, { public_key: null })
      if (sub === '/deploy-key' && method === 'POST') return json(route, 200, { public_key: `ssh-ed25519 AAAAC3NzaTEST rpanel-${a.slug}` })
      if (sub === '/webhook') return json(route, 200, { path: `/api/hooks/${a.slug}`, token: 'hooktoken123', branch: a.git_branch })
      if (sub === '/deployments') return json(route, 200, db.deployments.filter((d) => d.app_id === a.id).reverse())
      if (sub === '/deploy' && method === 'POST') {
        const d = {
          id: id(),
          app_id: a.id,
          status: 'running',
          triggered_by: 1,
          commit_sha: null,
          error: null,
          started_at: now(),
          finished_at: null,
          created_at: now(),
        }
        db.deployments.push(d)
        return json(route, 202, { deployment_id: d.id })
      }
    }
    if (path === '/backups') {
      const appId = Number(url.searchParams.get('app_id'))
      return json(route, 200, db.backups.filter((b) => !appId || b.app_id === appId))
    }
    if (path === '/cron/preview') return json(route, 200, { valid: !/[;$`]/.test(body.schedule), next: 'Wed 2026-10-14 03:00:00 KST', error: '일정 형식이 올바르지 않습니다' })
    if (path === '/sites')
      return json(
        route,
        200,
        db.apps
          .filter((a) => a.domains)
          .flatMap((a) =>
            a.domains.split(' ').map((d, i) => ({
              domain: d, app_id: a.id, app_name: a.name, app_slug: a.slug, kind: a.kind, app_status: a.status, project_id: a.project_id,
              http_status: i === 0 ? 200 : 502, latency_ms: i === 0 ? 12 : null, error: i === 0 ? null : 'Bad Gateway', last_deploy_status: null, last_deploy_at: null,
            })),
          ),
      )
    if (path === '/databases')
      return json(route, 200, {
        databases: db.dbs.map((d) => ({ ...d, app_name: db.apps.find((a) => a.id === d.app_id)?.name })),
        redis: db.apps.filter((a) => a.redis_db != null).map((a) => ({ app_id: a.id, app_name: a.name, app_slug: a.slug, db: a.redis_db, keys: 0 })),
        redis_range: [4, 15],
        panel_redis_db: 10,
      })
    m = path.match(/^\/deployments\/(\d+)$/)
    if (m) {
      const d = db.deployments.find((x) => x.id === Number(m[1]))
      const a = db.apps.find((x) => x.id === d?.app_id)
      return d ? json(route, 200, { ...d, log: null, app_name: a.name, app_slug: a.slug }) : json(route, 404, { message: '찾을 수 없습니다' })
    }
    return json(route, 404, { error: 'not_found', message: `mock: ${method} ${path}` })
  }

  /** 배포 로그 WebSocket: 줄을 보내고 성공 상태로 종료 */
  function handleWs(ws) {
    const m = ws.url().match(/\/ws\/deployments\/(\d+)/)
    if (m) {
      const d = db.deployments.find((x) => x.id === Number(m[1]))
      const lines = [
        'Rpanel 배포 #' + m[1],
        '▶ 소스 가져오기',
        '$ git clone --depth 1 --branch main -- ssh://git@100.100.199.123:2222/beri/backend.git /var/lib/rpanel/builds/x/repo',
        '▶ 빌드',
        '$ bash -c cargo build --release',
        '   Compiling hello-api v0.1.0',
        '▶ 헬스체크',
        'GET /api/health → 200 (1회차)',
        '✔ 배포 완료 (0000000d)',
      ]
      let i = 0
      const timer = setInterval(() => {
        if (i < lines.length) {
          // 중복 전송(재연결 백로그 시뮬레이션)도 섞어서 seq 중복 제거를 검증
          ws.send(JSON.stringify({ type: 'line', seq: i + 1, text: lines[i] }))
          if (i === 2) ws.send(JSON.stringify({ type: 'line', seq: 2, text: lines[1] }))
          i++
        } else {
          clearInterval(timer)
          if (d) {
            d.status = 'success'
            d.commit_sha = '0000000dryrun'
            d.finished_at = new Date().toISOString()
            const a = db.apps.find((x) => x.id === d.app_id)
            a.status = 'running'
            a.current_release = d.id
            a.current_commit = d.commit_sha
          }
          ws.send(JSON.stringify({ type: 'status', status: 'success' }))
          ws.close()
        }
      }, 40)
      return
    }
    if (ws.url().includes('/ws/metrics')) {
      const send = () =>
        ws.send(
          JSON.stringify({
            type: 'metrics',
            data: {
              ts: Date.now(),
              cpu_percent: 23.5,
              cpu_per_core: [20, 27],
              mem_total: 16 * 1024 ** 3,
              mem_used: 6 * 1024 ** 3,
              swap_total: 0,
              swap_used: 0,
              load: [0.42, 0.38, 0.3],
              uptime: 93784,
              disks: [{ mount: '/', total: 200 * 1024 ** 3, used: 41 * 1024 ** 3 }],
            },
          }),
        )
      send()
      const t = setInterval(send, 500)
      ws.onClose(() => clearInterval(t))
    }
  }

  return {
    db,
    async install(page) {
      await page.route('**/api/**', handle)
      await page.routeWebSocket(/\/api\/ws\//, handleWs)
    },
  }
}
