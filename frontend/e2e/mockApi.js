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
    const body = req.postData() ? JSON.parse(req.postData()) : undefined
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
        current_commit: null,
        current_release: null,
        created_at: now(),
        updated_at: now(),
      }
      db.apps.push(a)
      db.env[a.id] = []
      return json(route, 201, a)
    }

    // ── apps ──
    if (path === '/apps') {
      const pid = Number(url.searchParams.get('project_id'))
      return json(route, 200, db.apps.filter((a) => !pid || a.project_id === pid))
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
      if (sub === '/env' && method === 'GET') return json(route, 200, { vars: db.env[a.id] })
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
