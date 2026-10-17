// API 클라이언트
import { t } from '../i18n'
// - 액세스 토큰(JWT)은 메모리에만 보관 (XSS로 localStorage 탈취 방지)
// - 401이면 HttpOnly 리프레시 쿠키로 1회 갱신 후 재시도 (동시 요청은 갱신 1번만)

export class ApiError extends Error {
  constructor(status, message, code) {
    super(t(message)) // 서버 메시지도 사전에 있으면 선택한 언어로
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
  listeners.forEach((fn) => fn(token ? (user ?? null) : null))
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
    throw new ApiError(res.status, data?.message || t('요청 실패 ({status})', { status: res.status }), data?.error)
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
    throw new ApiError(res.status, data?.message || t('다운로드 실패 ({status})', { status: res.status }))
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

/** 파일 업로드 (진행률). path 는 사이트 유저 홈 기준 "폴더/파일명" */
export function uploadFile(appId, path, file, onProgress) {
  const send = () =>
    new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest()
      xhr.open('POST', `/api/apps/${appId}/files/upload?path=${encodeURIComponent(path)}`)
      xhr.setRequestHeader('X-Requested-With', 'rpanel')
      if (accessToken) xhr.setRequestHeader('Authorization', `Bearer ${accessToken}`)
      xhr.upload.onprogress = (e) => e.lengthComputable && onProgress?.(e.loaded / e.total)
      xhr.onload = () => {
        let data = null
        try {
          data = JSON.parse(xhr.responseText)
        } catch {
          /* ignore */
        }
        if (xhr.status >= 200 && xhr.status < 300) resolve(data)
        else reject(new ApiError(xhr.status, data?.message || t('업로드 실패 ({status})', { status: xhr.status })))
      }
      xhr.onerror = () => reject(new ApiError(0, t('업로드 중 연결이 끊겼습니다')))
      xhr.send(file)
    })
  return send().catch(async (e) => {
    if (e.status === 401 && (await refreshSession())) return send()
    throw e
  })
}
