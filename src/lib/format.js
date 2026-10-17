import { locale, t } from '../i18n'
export function bytes(n) {
  if (n == null || Number.isNaN(n)) return '–'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`
}

export function percent(used, total) {
  if (!total) return 0
  return Math.min(100, Math.max(0, (used / total) * 100))
}

export function uptime(secs) {
  if (secs == null) return '–'
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (d > 0) return t('{d}일 {h}시간 {m}분', { d, h, m })
  if (h > 0) return t('{h}시간 {m}분', { h, m })
  return t('{m}분', { m })
}

/** "3분 전" 형태 */
export function ago(iso, now = Date.now()) {
  if (!iso) return '–'
  const diff = Math.round((now - new Date(iso).getTime()) / 1000)
  if (diff < 10) return t('방금')
  if (diff < 60) return t('{diff}초 전', { diff })
  if (diff < 3600) return t('{v}분 전', { v: Math.floor(diff / 60) })
  if (diff < 86400) return t('{v}시간 전', { v: Math.floor(diff / 3600) })
  return t('{v}일 전', { v: Math.floor(diff / 86400) })
}

export function duration(startIso, endIso) {
  if (!startIso || !endIso) return '–'
  const s = Math.max(0, Math.round((new Date(endIso) - new Date(startIso)) / 1000))
  if (s < 60) return t('{s}초', { s })
  return t('{v}분 {v2}초', { v: Math.floor(s / 60), v2: s % 60 })
}

export function shortSha(sha) {
  return sha ? sha.slice(0, 8) : '–'
}

export function dateTime(iso) {
  if (!iso) return '–'
  return new Date(iso).toLocaleString(locale(), { hour12: false })
}

/** .env 텍스트 → [{key,value}] (주석/빈 줄 무시, 따옴표 제거, export 접두어 허용) */
export function parseDotenv(text) {
  const out = []
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    if (!line || line.startsWith('#')) continue
    const m = line.match(/^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/)
    if (!m) continue
    let value = m[2]
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1)
    }
    out.push({ key: m[1], value })
  }
  return out
}

/** 기존 목록에 병합 (같은 키는 덮어씀, 순서 유지) */
export function mergeEnv(current, incoming) {
  const map = new Map(current.map((v) => [v.key, v.value]))
  for (const { key, value } of incoming) map.set(key, value)
  return [...map.entries()].map(([key, value]) => ({ key, value }))
}

/** HTTP 상태코드 → 색 클래스 */
export function httpClass(code) {
  if (code == null) return 'text-fail'
  if (code < 300) return 'text-signal'
  if (code < 400) return 'text-sky-300'
  if (code < 500) return 'text-warn'
  return 'text-fail'
}

export const CRON_PRESETS = [
  { label: '5분마다', value: '*:0/5' },
  { label: '매시간', value: 'hourly' },
  { label: '매일 03:00', value: '*-*-* 03:00:00' },
  { label: '평일 09:00', value: 'Mon..Fri *-*-* 09:00:00' },
  { label: '매주 월 04:00', value: 'Mon *-*-* 04:00:00' },
  { label: '매월 1일 05:00', value: '*-*-01 05:00:00' },
]
