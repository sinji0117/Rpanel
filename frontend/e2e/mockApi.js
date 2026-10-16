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
    jobs: [],
    dbUsers: [],
    redisUsers: [],
    crons: [],
    files: {},
    ssl: {},
    accounts: [],
    admins: [],
    notifications: [],
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
    version: { current: '0.4.0', latest: '0.5.0', update_available: true, repo_url: 'https://github.com/sinji0117/Rpanel', checked_at: now(), error: null },
    runtimes: [
      { id: 901, lang: 'rust', version: 'stable', status: 'ready', is_default: true, is_system: true, detail: '1.90.0', log: null, updated_at: now() },
      { id: 902, lang: 'python', version: 'system', status: 'ready', is_default: true, is_system: true, detail: '3.12.3', log: null, updated_at: now() },
      { id: 903, lang: 'node', version: 'system', status: 'ready', is_default: true, is_system: true, detail: '22.11.0', log: null, updated_at: now() },
    ],
    seq: 1,
  }
  const id = () => db.seq++

  const json = (route, status, body) => route.fulfill({ status, contentType: 'application/json', body: body === undefined ? '' : JSON.stringify(body) })
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
      if (body.email !== db.user?.email || body.password !== db.password) return json(route, 400, { error: 'bad_request', message: '이메일 또는 비밀번호가 올바르지 않습니다' })
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
        boot_time: Math.floor(Date.now() / 1000) - (3 * 86400 + 4 * 3600 + 5 * 60),
        panel_started_at: Math.floor(Date.now() / 1000) - 7200,
      })
    if (path === '/system/overview')
      return json(route, 200, {
        projects: db.projects.length,
        apps: { total: db.apps.length, by_status: {} },
        deployments_24h: { total: db.deployments.length, failed: 0 },
        recent_deployments: db.deployments.map((d) => ({ ...d, app_name: db.apps.find((a) => a.id === d.app_id)?.name, app_slug: '' })),
      })
    if (path === '/audit') return json(route, 200, [])
    if (path === '/system/version') return json(route, 200, db.version)
    // ── notifications ──
    if (path === '/notifications' && method === 'GET') {
      const unreadOnly = url.searchParams.get('unread') === 'true'
      const items = [...db.notifications]
        .reverse()
        .filter((n) => !unreadOnly || !n.read_at)
        .slice(0, Number(url.searchParams.get('limit') || 50))
      return json(route, 200, { items, unread: db.notifications.filter((n) => !n.read_at).length })
    }
    if (path === '/notifications' && method === 'DELETE') {
      db.notifications = []
      return json(route, 204)
    }
    if (path === '/notifications/read') {
      for (const n of db.notifications) if (body.all || body.ids?.includes(n.id)) n.read_at ??= now()
      return json(route, 200, { unread: db.notifications.filter((n) => !n.read_at).length })
    }
    if (path === '/notifications/settings') {
      const events = Object.keys(db.notifySettings.events).map((key) => ({
        key,
        label: {
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
        }[key],
        default: true,
      }))
      if (method === 'PUT') {
        if (body.channels.discord_url && !body.channels.discord_url.startsWith('https://discord.com/api/webhooks/'))
          return json(route, 400, { error: 'bad_request', message: 'Discord 웹훅 주소는 https://discord.com/api/webhooks/ 로 시작합니다' })
        db.notifySettings = body
      }
      return json(route, 200, { settings: db.notifySettings, events })
    }
    if (path === '/notifications/test') {
      db.notifications.push({
        id: id(),
        kind: 'test',
        level: 'info',
        title: '시험 알림입니다',
        params: {},
        body: 'Rpanel 알림이 잘 도착했습니다.',
        link: null,
        read_at: null,
        created_at: now(),
      })
      return json(route, 200, { results: db.notifySettings.channels.discord_url ? [{ channel: 'discord', ok: true, error: null }] : [] })
    }
    if (path === '/system/metrics') {
      const n = { '30m': 30, '1h': 60, '6h': 360, '24h': 1440, '7d': 10080 }[url.searchParams.get('range') || '30m']
      const nowS = Math.floor(Date.now() / 1000)
      const step = Math.max(1, Math.ceil(n / 120))
      const points = []
      for (let i = n - 1; i >= 0; i -= step)
        points.push({ t: nowS - i * 60, c: 5 + (i % 7), m: 4 * 2 ** 30, mt: 47 * 2 ** 30, l: [0.3, 0.2, 0.15], d: [['/', 41 * 2 ** 30, 371 * 2 ** 30]] })
      return json(route, 200, { range: url.searchParams.get('range') || '30m', interval: 60, cpu_count: 6, points })
    }
    if (path === '/logs/deployments')
      return json(
        route,
        200,
        db.deployments.map((d) => ({ ...d, app_name: db.apps.find((a) => a.id === d.app_id)?.name, app_slug: '' })),
      )
    if (path === '/logs/system') {
      const kind = url.searchParams.get('kind')
      if (!['panel', 'nginx', 'ssh', 'mariadb', 'redis'].includes(kind)) return json(route, 400, { error: 'bad_request', message: '로그 종류가 올바르지 않습니다' })
      const lines =
        {
          panel: ['2026-10-16T12:00:00+0900 rpanel[1]: starting rpanel 0.4.0', '2026-10-16T12:00:01+0900 rpanel[1]: WARN slow request'],
          ssh: ['Accepted publickey for shop from 1.2.3.4', 'Failed password for root from 5.6.7.8'],
        }[kind] ?? []
      return json(route, 200, { kind, lines })
    }

    // ── runtimes ── (설치는 GET 두 번째에 끝난 것으로)
    if (path === '/runtimes' && method === 'GET') {
      for (const r of db.runtimes)
        if (r.status === 'installing' && (r.polls = (r.polls ?? 0) + 1) > 1) {
          Object.assign(r, {
            status: 'ready',
            detail: r.lang === 'python' ? `${r.version}.9` : r.lang === 'node' ? `${r.version}.3.0` : `${r.version}.0`,
            log: `$ install ${r.lang} ${r.version}\nOK`,
          })
          if (r.makeDefault) for (const x of db.runtimes) if (x.lang === r.lang) x.is_default = x === r
        }
      const usage = {}
      for (const a of db.apps) {
        const lang = a.kind === 'python' ? 'python' : a.source === 'git' && a.kind === 'rust' ? 'rust' : a.source === 'git' && a.kind === 'static' ? 'node' : null
        if (!lang) continue
        const v = a.runtime_version ?? db.runtimes.find((r) => r.lang === lang && r.is_default)?.version
        usage[`${lang}:${v}`] = (usage[`${lang}:${v}`] ?? 0) + 1
      }
      return json(route, 200, {
        runtimes: db.runtimes,
        candidates: { rust: ['stable', 'beta', 'nightly', '1.90', '1.89'], python: ['system', '3.14', '3.13', '3.12', '3.11'], node: ['system', '24', '22', '20'] },
        usage,
      })
    }
    if (path === '/runtimes' && method === 'POST') {
      if (body.lang === 'python' && !/^(system|3\.\d{1,2})$/.test(body.version))
        return json(route, 400, { error: 'bad_request', message: 'Python 버전은 3.8 ~ 3.20 형식입니다 (예: 3.12)' })
      const ex = db.runtimes.find((r) => r.lang === body.lang && r.version === body.version)
      if (ex?.status === 'ready') {
        if (body.make_default) for (const x of db.runtimes) if (x.lang === ex.lang) x.is_default = x === ex
        return json(route, 200, ex)
      }
      const r = ex ?? { id: id(), lang: body.lang, version: body.version, is_default: false, is_system: false, detail: '', log: null, updated_at: now() }
      Object.assign(r, { status: 'installing', polls: 0, makeDefault: !!body.make_default })
      if (!ex) db.runtimes.push(r)
      return json(route, 202, r)
    }
    let rm = path.match(/^\/runtimes\/(\d+)(\/default)?$/)
    if (rm) {
      const r = db.runtimes.find((x) => x.id === Number(rm[1]))
      if (!r) return json(route, 404, { error: 'not_found', message: '찾을 수 없습니다' })
      if (rm[2]) {
        for (const x of db.runtimes) if (x.lang === r.lang) x.is_default = x === r
        return json(route, 200, r)
      }
      if (r.is_default || r.is_system) return json(route, 400, { error: 'bad_request', message: '기본 버전은 지울 수 없습니다 — 다른 버전을 기본으로 정한 뒤 지우세요' })
      if (db.apps.some((a) => a.runtime_version === r.version))
        return json(route, 409, { error: 'conflict', message: '앱 1개가 이 버전을 쓰고 있습니다 — 앱 설정에서 버전을 바꾼 뒤 지우세요' })
      db.runtimes = db.runtimes.filter((x) => x !== r)
      return json(route, 204)
    }

    // ── admins ──
    const admins = () => [db.user, ...db.admins]
    if (path === '/users' && method === 'GET') return json(route, 200, { users: admins().map((u) => ({ last_login_at: null, ...u })), me: db.user.id })
    if (path === '/users' && method === 'POST') {
      if (!/^[^@\s]+@[^@\s]+$/.test(body.email ?? '')) return json(route, 400, { error: 'bad_request', message: '이메일 형식이 올바르지 않습니다' })
      if ((body.password ?? '').length < 10) return json(route, 400, { error: 'bad_request', message: '비밀번호는 10자 이상이어야 합니다' })
      if (admins().some((u) => u.email === body.email.toLowerCase())) return json(route, 409, { error: 'conflict', message: '이미 존재하는 값입니다' })
      const u = { id: 1000 + id(), email: body.email.toLowerCase(), role: 'admin', created_at: now(), last_login_at: null }
      db.admins.push(u)
      return json(route, 201, u)
    }
    let um = path.match(/^\/users\/(\d+)$/)
    if (um) {
      const u = admins().find((x) => x.id === Number(um[1]))
      if (!u) return json(route, 404, { error: 'not_found', message: '찾을 수 없습니다' })
      if (method === 'PUT') {
        if (body.email) u.email = body.email.toLowerCase()
        return json(route, 200, u)
      }
      if (method === 'DELETE') {
        if (u.id === db.user.id) return json(route, 400, { error: 'bad_request', message: '자기 자신은 삭제할 수 없습니다' })
        db.admins = db.admins.filter((x) => x !== u)
        return json(route, 204)
      }
    }

    // ── projects ──
    if (path === '/projects' && method === 'GET')
      return json(
        route,
        200,
        db.projects.map((p) => ({ ...p, app_count: db.apps.filter((a) => a.project_id === p.id).length, running_count: 0 })),
      )
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

    if (path === '/system/edge') return json(route, 200, { edge_mode: 'direct', trusted_proxy: '', ssl_available: true })
    if (path === '/sites' && method === 'POST') {
      const domains = body.domains
        .trim()
        .toLowerCase()
        .split(/[\s,]+/)
      if (db.apps.some((a) => (a.domains || '').split(' ').some((d) => domains.includes(d))))
        return json(route, 409, { error: 'conflict', message: `도메인 ${domains[0]} 은(는) 이미 사용 중입니다` })
      const kind = body.type === 'html' ? 'static' : body.type
      const user = body.type === 'proxy' ? null : body.site_user || domains[0].split('.')[0]
      const a = {
        id: id(),
        project_id: null,
        name: domains[0],
        slug: domains[0].replace(/\./g, '-'),
        kind,
        source: body.type === 'proxy' ? 'none' : 'files',
        git_url: null,
        git_branch: 'main',
        domains: domains.join(' '),
        site_user: user,
        site_root: user ? `/home/${user}/htdocs/${domains[0]}` : null,
        web_root: '',
        proxy_target: body.proxy_target || null,
        start_cmd: body.start_cmd || null,
        build_cmd: null,
        health_path: '/',
        port: kind === 'rust' ? 8150 : null,
        status: kind === 'rust' ? 'idle' : 'running',
        ssl_mode: 'none',
        force_https: false,
        client_max_body_mb: 64,
        current_release: 1,
        redis_db: null,
        backup_schedule: 'off',
        backup_keep: 7,
      }
      db.apps.push(a)
      db.env[a.id] = []
      db.files[a.id] = { [`htdocs/${domains[0]}`]: kind === 'rust' ? { 'README.txt': 'cargo build --release' } : { 'index.html': '<h1>hi</h1>' } }
      return json(route, 201, { app: a, deployment_id: null, site_user: user, generated_password: body.password ? null : 'Gen3ratedPassw0rd' })
    }

    // ── apps ──
    if (path === '/apps') {
      const pid = Number(url.searchParams.get('project_id'))
      return json(
        route,
        200,
        db.apps.filter((a) => !pid || a.project_id === pid),
      )
    }
    m = path.match(/^\/apps\/(\d+)\/runtime$/)
    if (m && method === 'PUT') {
      const a = db.apps.find((x) => x.id === Number(m[1]))
      a.runtime_version = body.version ?? null
      return json(route, 200, a)
    }

    // ── site accounts ──
    m = path.match(/^\/apps\/(\d+)\/accounts(?:\/(\d+)(\/password)?)?$/)
    if (m) {
      const a = db.apps.find((x) => x.id === Number(m[1]))
      if (!a?.site_user) return json(route, 400, { error: 'bad_request', message: '파일 기반 사이트에서만 쓸 수 있습니다' })
      if (!db.accounts.some((x) => x.app_id === a.id && x.is_primary))
        db.accounts.unshift({ id: id(), app_id: a.id, username: a.site_user, is_primary: true, access: 'ssh', enabled: true, key_count: 0, note: null, created_at: now() })
      const mine = () => db.accounts.filter((x) => x.app_id === a.id)
      if (!m[2] && method === 'GET') return json(route, 200, { accounts: mine(), site_user: a.site_user, site_root: a.site_root })
      if (!m[2] && method === 'POST') {
        if (!/^[a-z][a-z0-9-]{2,31}$/.test(body.username ?? '')) return json(route, 400, { error: 'bad_request', message: '계정 이름 형식이 올바르지 않습니다' })
        if (db.accounts.some((x) => x.username === body.username)) return json(route, 409, { error: 'conflict', message: '이미 존재하는 값입니다' })
        const acc = {
          id: id(),
          app_id: a.id,
          username: body.username,
          is_primary: false,
          access: body.access || 'sftp',
          enabled: true,
          key_count: (body.ssh_keys || '').split('\n').filter(Boolean).length,
          note: body.note ?? null,
          created_at: now(),
        }
        db.accounts.push(acc)
        return json(route, 201, { account: acc, password: body.password || (body.ssh_keys ? null : 'AutoGen3rated-pw-01') })
      }
      const acc = mine().find((x) => x.id === Number(m[2]))
      if (!acc) return json(route, 404, { error: 'not_found', message: '찾을 수 없습니다' })
      if (m[3]) {
        if (body.password && body.password.length < 10) return json(route, 400, { error: 'bad_request', message: '비밀번호는 10자 이상이어야 합니다' })
        return json(route, 200, { ok: true, password: body.password || 'AutoGen3rated-pw-02' })
      }
      if (method === 'PUT') {
        for (const k of ['access', 'enabled', 'note']) if (body[k] !== undefined) acc[k] = body[k]
        return json(route, 200, acc)
      }
      if (method === 'DELETE') {
        if (acc.is_primary) return json(route, 400, { error: 'bad_request', message: '기본 계정은 삭제할 수 없습니다' })
        db.accounts = db.accounts.filter((x) => x !== acc)
        return json(route, 204)
      }
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
        ...db.dbs
          .filter((d) => d.app_id === a.id)
          .map((d) => ({ key: d.env_key, value: `mysql://${d.username}:secretpw@127.0.0.1:3306/${d.name}`, source: `MariaDB · ${d.name}` })),
        ...(a.redis_db != null ? [{ key: 'REDIS_URL', value: `redis://:pw@127.0.0.1:6379/${a.redis_db}`, source: `Redis · DB ${a.redis_db}` }] : []),
      ]
      if (sub === '/files') {
        const dir = url.searchParams.get('path') || ''
        const tree = db.files[a.id] || {}
        const entries = Object.entries(tree[dir] || {}).map(([name, content]) => ({ name, kind: 'file', size: content.length, mode: '0644', mtime: 1792100000 }))
        for (const k of Object.keys(tree))
          if (k.startsWith(dir + '/') && !k.slice(dir.length + 1).includes('/'))
            entries.unshift({ name: k.slice(dir.length + 1), kind: 'dir', size: 0, mode: '0750', mtime: 1792100000 })
        return json(route, 200, { path: dir, entries, root: `htdocs/${a.domains.split(' ')[0]}`, user: a.site_user })
      }
      if (sub === '/content' || sub === '/files-content') return json(route, 404, {})
      if (sub === '/ssl' && method === 'GET')
        return json(route, 200, {
          mode: a.ssl_mode,
          force_https: a.force_https,
          certificates: a.ssl_mode === 'none' ? [] : [{ path: 'x', not_after: 'Jan 14 00:00:00 2027 GMT', issuer: 'CN = Test CA', domains: a.domains.split(' ') }],
          edge_mode: 'direct',
          domains: a.domains.split(' '),
        })
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
          db.backups.unshift({
            id: id(),
            app_id: a.id,
            app_name: a.name,
            app_slug: a.slug,
            kind: 'db',
            target: d.name,
            file: `${stamp}-db-${d.name}.sql.gz`,
            size_bytes: 20480,
            status: 'success',
            error: null,
            trigger_by: 'manual',
            created_at: now(),
            finished_at: now(),
          })
        db.backups.unshift({
          id: id(),
          app_id: a.id,
          app_name: a.name,
          app_slug: a.slug,
          kind: 'data',
          target: 'data',
          file: `${stamp}-data.tar.gz`,
          size_bytes: 1024,
          status: 'success',
          error: null,
          trigger_by: 'manual',
          created_at: now(),
          finished_at: now(),
        })
        return json(route, 202, { backup_ids: [] })
      }
      if (sub === '/cron' && method === 'GET')
        return json(
          route,
          200,
          db.crons.filter((c) => c.app_id === a.id),
        )
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
          stats: {
            total: 4,
            by_class: { '2xx': 3, '5xx': 1 },
            top_paths: [
              ['/', 3],
              ['/api', 1],
            ],
            top_ips: [['203.0.113.5', 4]],
            bytes: 2048,
          },
        })
      if (sub === '/env' && method === 'PUT') {
        const bad = body.vars.find((v) => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(v.key))
        if (bad) return json(route, 400, { error: 'bad_request', message: `환경변수 이름이 올바르지 않습니다: ${bad.key}` })
        db.env[a.id] = [...body.vars].sort((x, y) => x.key.localeCompare(y.key))
        return json(route, 200, { vars: db.env[a.id] })
      }
      if (sub === '/service' && method === 'GET') return json(route, 200, { active: a.svc ?? (a.current_release ? 'active' : 'inactive'), unit: `rpanel-app-${a.slug}.service` })
      if (sub === '/service' && method === 'POST') {
        a.svc = body.action === 'stop' ? 'inactive' : 'active'
        a.status = body.action === 'stop' ? 'stopped' : 'running'
        return json(route, 200, { ok: true })
      }
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
      return json(
        route,
        200,
        db.backups.filter((b) => !appId || b.app_id === appId),
      )
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
              domain: d,
              app_id: a.id,
              app_name: a.name,
              app_slug: a.slug,
              kind: a.kind,
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
    // ── 데이터베이스 · 계정 ──
    const dbUrl = (name, user, pw = 'secretpw') => `mysql://${user}:${pw}@127.0.0.1:3306/${name}`
    const redisUrl = (u) => `redis://${u.username}:${u.password}@127.0.0.1:6379/${u.db_index}`
    const pw = () => 'Gen' + Math.random().toString(36).slice(2, 14) + 'x9'
    if (path === '/databases' && method === 'GET')
      return json(route, 200, {
        databases: db.dbs.map((d) => ({
          ...d,
          app_id: d.app_id ?? null,
          app_name: db.apps.find((a) => a.id === d.app_id)?.name ?? null,
          users: [
            { id: null, username: d.username, privileges: 'all', primary: true, url: dbUrl(d.name, d.username) },
            ...db.dbUsers.filter((u) => u.database_id === d.id).map((u) => ({ ...u, primary: false, url: dbUrl(d.name, u.username, u.password) })),
          ],
        })),
        redis: db.apps.filter((a) => a.redis_db != null).map((a) => ({ app_id: a.id, app_name: a.name, app_slug: a.slug, db: a.redis_db, keys: 0 })),
        redis_users: db.redisUsers.map((u) => ({ ...u, password: undefined, url: redisUrl(u) })),
        redis_range: [4, 15],
        panel_redis_db: 10,
      })
    if (path === '/databases' && method === 'POST') {
      if (!/^[a-z][a-z0-9_]{1,63}$/.test(body.name || '')) return json(route, 400, { error: 'bad_request', message: 'DB 이름은 소문자로 시작하는 소문자·숫자·_ 2~64자여야 합니다' })
      if (db.dbs.some((d) => d.name === body.name)) return json(route, 409, { error: 'conflict', message: '이미 사용 중인 DB 이름입니다' })
      const d = { id: id(), app_id: body.app_id ?? null, name: body.name, username: body.username || body.name, env_key: body.env_key || 'DATABASE_URL', created_at: now() }
      db.dbs.push(d)
      return json(route, 201, { ...d, url: dbUrl(d.name, d.username) })
    }
    m = path.match(/^\/databases\/(\d+)(\/.*)?$/)
    if (m) {
      const d = db.dbs.find((x) => x.id === Number(m[1]))
      if (!d) return json(route, 404, { error: 'not_found', message: '찾을 수 없습니다' })
      const sub = m[2] || ''
      if (!sub && method === 'PUT') {
        Object.assign(d, { app_id: body.app_id, env_key: body.env_key || d.env_key })
        return json(route, 200, { ok: true })
      }
      if (!sub && method === 'DELETE') {
        db.dbs = db.dbs.filter((x) => x !== d)
        db.dbUsers = db.dbUsers.filter((u) => u.database_id !== d.id)
        return json(route, 204)
      }
      if (sub === '/password') return json(route, 200, { ok: true, url: dbUrl(d.name, d.username, 'NewPrimary123') })
      if (sub === '/backups') {
        db.backups.unshift({
          id: id(),
          app_id: d.app_id,
          app_name: null,
          app_slug: null,
          database_id: d.id,
          kind: 'db',
          target: d.name,
          file: `20261013-030000-db-${d.name}.sql.gz`,
          size_bytes: 4096,
          status: 'success',
          error: null,
          trigger_by: 'manual',
          created_at: now(),
          finished_at: now(),
        })
        return json(route, 202, { backup_ids: [] })
      }
      if (sub === '/users' && method === 'POST') {
        if (!/^[a-z][a-z0-9_]{1,31}$/.test(body.username || ''))
          return json(route, 400, { error: 'bad_request', message: '사용자 이름은 소문자로 시작하는 소문자·숫자·_ 2~32자여야 합니다' })
        if ([...db.dbs.map((x) => x.username), ...db.dbUsers.map((u) => u.username)].includes(body.username))
          return json(route, 409, { error: 'conflict', message: `사용자 이름 ${body.username} 은(는) 이미 있습니다` })
        const u = { id: id(), database_id: d.id, username: body.username, privileges: body.privileges || 'all', password: body.password || pw(), created_at: now() }
        db.dbUsers.push(u)
        return json(route, 201, { ...u, url: dbUrl(d.name, u.username, u.password) })
      }
      const um = sub.match(/^\/users\/(\d+)(\/password)?$/)
      if (um) {
        const u = db.dbUsers.find((x) => x.id === Number(um[1]))
        if (um[2]) {
          u.password = body?.password || pw()
          return json(route, 200, { ok: true, password: u.password, url: dbUrl(d.name, u.username, u.password) })
        }
        db.dbUsers = db.dbUsers.filter((x) => x !== u)
        return json(route, 204)
      }
    }
    if (path === '/redis-users' && method === 'POST') {
      if (!/^[a-z][a-z0-9_-]{2,31}$/.test(body.username || '') || body.username === 'default')
        return json(route, 400, { error: 'bad_request', message: 'Redis 계정 이름은 소문자로 시작하는 소문자·숫자·_- 3~32자여야 합니다' })
      const u = {
        id: id(),
        username: body.username,
        password: body.password || pw(),
        key_pattern: body.key_pattern || `${body.username}:*`,
        readonly: !!body.readonly,
        db_index: body.db_index ?? 0,
        note: body.note || null,
        created_at: now(),
      }
      db.redisUsers.push(u)
      return json(route, 201, { id: u.id, username: u.username, password: u.password, url: redisUrl(u) })
    }
    m = path.match(/^\/redis-users\/(\d+)(\/password)?$/)
    if (m) {
      const u = db.redisUsers.find((x) => x.id === Number(m[1]))
      if (m[2]) {
        u.password = pw()
        return json(route, 200, { ok: true, password: u.password, url: redisUrl(u) })
      }
      if (method === 'PUT') {
        Object.assign(u, { key_pattern: body.key_pattern || u.key_pattern, readonly: !!body.readonly, db_index: body.db_index, note: body.note || null })
        return json(route, 200, { ok: true, url: redisUrl(u) })
      }
      db.redisUsers = db.redisUsers.filter((x) => x !== u)
      return json(route, 204)
    }

    // ── 백업 예약 ──
    const pad = (n) => String(n).padStart(2, '0')
    const jobView = (j) => ({
      ...j,
      target_name:
        j.target === 'app' ? db.apps.find((a) => a.id === j.app_id)?.name : j.target === 'database' ? db.dbs.find((d) => d.id === j.database_id)?.name : '모든 앱·데이터베이스',
      next_run_at: j.enabled ? `2026-10-17T${pad(j.hour)}:${pad(j.minute)}:00Z` : null,
      last_status: db.backups.find((b) => b.job_id === j.id)?.status ?? null,
      backup_count: db.backups.filter((b) => b.job_id === j.id).length,
    })
    if (path === '/backup-jobs' && method === 'GET') {
      const appId = Number(url.searchParams.get('app_id'))
      return json(route, 200, { jobs: db.jobs.filter((j) => !appId || j.app_id === appId || j.target === 'all').map(jobView), timezone: '+09:00' })
    }
    if ((path === '/backup-jobs' && method === 'POST') || (/^\/backup-jobs\/\d+$/.test(path) && method === 'PUT')) {
      if (!['hourly', 'daily', 'weekly'].includes(body.schedule)) return json(route, 400, { error: 'bad_request', message: '주기는 hourly / daily / weekly 중 하나입니다' })
      if (body.target === 'app' && !body.app_id) return json(route, 400, { error: 'bad_request', message: '백업할 앱을 고르세요' })
      const name = body.name || (body.target === 'all' ? '전체 백업' : `${jobView({ ...body, id: 0 }).target_name} 백업`)
      if (method === 'POST') {
        const j = { id: id(), ...body, name, last_run_at: null, created_at: now() }
        db.jobs.push(j)
        return json(route, 201, jobView(j))
      }
      const j = db.jobs.find((x) => x.id === Number(path.split('/')[2]))
      Object.assign(j, body, { name })
      return json(route, 200, jobView(j))
    }
    m = path.match(/^\/backup-jobs\/(\d+)(\/run)?$/)
    if (m) {
      const j = db.jobs.find((x) => x.id === Number(m[1]))
      if (m[2]) {
        const apps = j.target === 'app' ? db.apps.filter((a) => a.id === j.app_id) : db.apps
        for (const a of apps)
          db.backups.unshift({
            id: id(),
            app_id: a.id,
            app_name: a.name,
            app_slug: a.slug,
            job_id: j.id,
            job_name: j.name,
            kind: 'data',
            target: 'data',
            file: '20261013-040000-data.tar.gz',
            size_bytes: 2048,
            status: 'success',
            error: null,
            trigger_by: 'manual',
            created_at: now(),
            finished_at: now(),
          })
        j.last_run_at = now()
        return json(route, 202, { started: apps.length })
      }
      if (body?.delete_files) db.backups = db.backups.filter((b) => b.job_id !== j.id)
      db.jobs = db.jobs.filter((x) => x !== j)
      return json(route, 204)
    }
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
