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
  if (d > 0) return `${d}일 ${h}시간`
  if (h > 0) return `${h}시간 ${m}분`
  return `${m}분`
}

/** "3분 전" 형태 */
export function ago(iso, now = Date.now()) {
  if (!iso) return '–'
  const diff = Math.round((now - new Date(iso).getTime()) / 1000)
  if (diff < 10) return '방금'
  if (diff < 60) return `${diff}초 전`
  if (diff < 3600) return `${Math.floor(diff / 60)}분 전`
  if (diff < 86400) return `${Math.floor(diff / 3600)}시간 전`
  return `${Math.floor(diff / 86400)}일 전`
}

export function duration(startIso, endIso) {
  if (!startIso || !endIso) return '–'
  const s = Math.max(0, Math.round((new Date(endIso) - new Date(startIso)) / 1000))
  if (s < 60) return `${s}초`
  return `${Math.floor(s / 60)}분 ${s % 60}초`
}

export function shortSha(sha) {
  return sha ? sha.slice(0, 8) : '–'
}

export function dateTime(iso) {
  if (!iso) return '–'
  return new Date(iso).toLocaleString('ko-KR', { hour12: false })
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
