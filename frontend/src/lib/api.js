// API 클라이언트
// - 액세스 토큰(JWT)은 메모리에만 보관 (XSS로 localStorage 탈취 방지)
// - 401이면 HttpOnly 리프레시 쿠키로 1회 갱신 후 재시도 (동시 요청은 갱신 1번만)

export class ApiError extends Error {
  constructor(status, message, code) {
    super(message)
    this.status = status
    this.code = code
  }
}

let accessToken = null
let refreshing = null
const listeners = new Set()

export function getAccessToken() {
  return accessToken
}

export function setSession(token, user) {
  accessToken = token
  listeners.forEach((fn) => fn(token ? user ?? null : null))
}

/** 세션 변경 구독 (로그인/로그아웃/만료). 반환값은 해제 함수 */
export function onSessionChange(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

const CSRF = { 'X-Requested-With': 'rpanel' }

async function parse(res) {
  if (res.status === 204) return null
  const text = await res.text()
  if (!text) return null
  try {
    return JSON.parse(text)
  } catch {
    return { message: text }
  }
}

/** 리프레시 쿠키로 새 액세스 토큰. 성공 시 사용자 객체, 실패 시 null */
export function refreshSession() {
  if (!refreshing) {
    refreshing = fetch('/api/auth/refresh', { method: 'POST', headers: CSRF, credentials: 'same-origin' })
      .then(async (res) => {
        if (!res.ok) {
          setSession(null)
          return null
        }
        const data = await parse(res)
        setSession(data.access_token, data.user)
        return data.user
      })
      .catch(() => {
        setSession(null)
        return null
      })
      .finally(() => {
        refreshing = null
      })
  }
  return refreshing
}

export async function api(path, { method = 'GET', body, retry = true } = {}) {
  const run = () => {
    const headers = { ...CSRF }
    if (body !== undefined) headers['Content-Type'] = 'application/json'
    if (accessToken) headers.Authorization = `Bearer ${accessToken}`
    return fetch(`/api${path}`, {
      method,
      headers,
      credentials: 'same-origin',
      body: body !== undefined ? JSON.stringify(body) : undefined,
    })
  }

  let res = await run()
  if (res.status === 401 && retry && !path.startsWith('/auth/')) {
    const user = await refreshSession()
    if (user) res = await run()
  }
  const data = await parse(res)
  if (!res.ok) {
    throw new ApiError(res.status, data?.message || `요청 실패 (${res.status})`, data?.error)
  }
  return data
}

export const get = (p) => api(p)
export const post = (p, body = {}) => api(p, { method: 'POST', body })
export const put = (p, body) => api(p, { method: 'PUT', body })
export const del = (p) => api(p, { method: 'DELETE' })

export async function login(email, password) {
  const data = await api('/auth/login', { method: 'POST', body: { email, password }, retry: false })
  setSession(data.access_token, data.user)
  return data.user
}

export async function setup(email, password) {
  const data = await api('/auth/setup', { method: 'POST', body: { email, password }, retry: false })
  setSession(data.access_token, data.user)
  return data.user
}

export async function logout() {
  try {
    await fetch('/api/auth/logout', { method: 'POST', headers: CSRF, credentials: 'same-origin' })
  } finally {
    setSession(null)
  }
}

/** 30초 1회성 티켓으로 WebSocket 열기 */
export async function openSocket(path) {
  const { ticket } = await post('/auth/ws-ticket')
  const proto = window.location.protocol === 'https:' ? 'wss' : 'ws'
  return new WebSocket(`${proto}://${window.location.host}/api${path}?ticket=${encodeURIComponent(ticket)}`)
}

/** 인증이 필요한 파일 다운로드 (백업 등) */
export async function download(path, filename) {
  const run = () =>
    fetch(`/api${path}`, {
      headers: { ...CSRF, ...(accessToken ? { Authorization: `Bearer ${accessToken}` } : {}) },
      credentials: 'same-origin',
    })
  let res = await run()
  if (res.status === 401 && (await refreshSession())) res = await run()
  if (!res.ok) {
    const data = await parse(res)
    throw new ApiError(res.status, data?.message || `다운로드 실패 (${res.status})`)
  }
  const blob = await res.blob()
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
