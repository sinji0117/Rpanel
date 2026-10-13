import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

function res(status, body, headers = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers,
    text: async () => (body === undefined ? '' : JSON.stringify(body)),
  }
}

describe('api client', () => {
  let api
  beforeEach(async () => {
    vi.resetModules()
    api = await import('./api')
  })
  afterEach(() => vi.restoreAllMocks())

  it('sends bearer + CSRF header and parses JSON', async () => {
    const fetchMock = vi.fn().mockResolvedValue(res(200, { ok: 1 }))
    vi.stubGlobal('fetch', fetchMock)
    api.setSession('tok', { id: 1 })
    expect(await api.get('/projects')).toEqual({ ok: 1 })
    const [url, init] = fetchMock.mock.calls[0]
    expect(url).toBe('/api/projects')
    expect(init.headers.Authorization).toBe('Bearer tok')
    expect(init.headers['X-Requested-With']).toBe('rpanel')
    expect(init.credentials).toBe('same-origin')
  })

  it('on 401 refreshes once (single-flight) and retries', async () => {
    let refreshCalls = 0
    const fetchMock = vi.fn(async (url, init) => {
      if (url === '/api/auth/refresh') {
        refreshCalls++
        await new Promise((r) => setTimeout(r, 10))
        return res(200, { access_token: 'new', user: { id: 1, email: 'a@b.c' } })
      }
      return init.headers.Authorization === 'Bearer new' ? res(200, { url }) : res(401, { message: 'no' })
    })
    vi.stubGlobal('fetch', fetchMock)
    api.setSession('old', { id: 1 })
    const [a, b] = await Promise.all([api.get('/a'), api.get('/b')])
    expect(a).toEqual({ url: '/api/a' })
    expect(b).toEqual({ url: '/api/b' })
    expect(refreshCalls).toBe(1)
    expect(api.getAccessToken()).toBe('new')
  })

  it('refresh failure clears session and throws ApiError', async () => {
    const seen = []
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url) => (url === '/api/auth/refresh' ? res(401, { message: 'expired' }) : res(401, { error: 'unauthorized', message: '인증이 필요합니다' }))),
    )
    api.onSessionChange((u) => seen.push(u))
    api.setSession('old', { id: 1 })
    await expect(api.get('/x')).rejects.toMatchObject({ status: 401, message: '인증이 필요합니다' })
    expect(api.getAccessToken()).toBe(null)
    expect(seen.at(-1)).toBe(null)
  })

  it('does not try to refresh for /auth/* endpoints', async () => {
    const fetchMock = vi.fn().mockResolvedValue(res(400, { message: '이메일 또는 비밀번호가 올바르지 않습니다' }))
    vi.stubGlobal('fetch', fetchMock)
    await expect(api.login('a@b.c', 'x')).rejects.toThrow('올바르지 않습니다')
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('handles 204', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(res(204)))
    expect(await api.del('/apps/1')).toBe(null)
  })
})
