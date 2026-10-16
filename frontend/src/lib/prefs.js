// 화면 설정 (테마) — 이 브라우저에만 저장됩니다
const KEY_THEME = 'rpanel.theme'

function read(key, fallback) {
  try {
    return localStorage.getItem(key) ?? fallback
  } catch {
    return fallback
  }
}
function write(key, value) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // 저장할 수 없는 환경 (사생활 보호 모드 등) — 이번 방문 동안만 적용
  }
}

export const THEMES = ['system', 'light', 'dark']

let theme = read(KEY_THEME, 'system')
const media = typeof window !== 'undefined' && window.matchMedia ? window.matchMedia('(prefers-color-scheme: light)') : null

export function resolvedTheme(mode = theme) {
  if (mode === 'light' || mode === 'dark') return mode
  return media?.matches ? 'light' : 'dark'
}

export function applyTheme() {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.theme = resolvedTheme()
}

export function getTheme() {
  return theme
}

export function setTheme(mode) {
  theme = THEMES.includes(mode) ? mode : 'system'
  write(KEY_THEME, theme)
  applyTheme()
}

media?.addEventListener?.('change', () => theme === 'system' && applyTheme())
