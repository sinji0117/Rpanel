import { useState } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router'
import { useAuth } from '../context/AuthContext'
import { LANGS, setLang, t, useLang } from '../i18n'
import { getTheme, setTheme } from '../lib/prefs'
import NotificationBell from './NotificationBell'
import { StaleBundleBanner, VersionBadge } from './VersionInfo'

const DB_ICON = 'M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zm0 0v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3'

// label 은 한국어 원문 (화면에는 t() 로 번역해서 표시)
// 1차 메뉴(묶음) → 2차 메뉴. match: 이 경로로 시작하는 화면도 그 메뉴로 표시
export const NAV = [
  { to: '/', label: '대시보드', icon: 'M3 12h4l3-8 4 16 3-8h4', end: true },
  {
    id: 'web',
    label: '웹사이트',
    icon: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c2.5 2.7 2.5 15.3 0 18M12 3c-2.5 2.7-2.5 15.3 0 18',
    children: [
      { to: '/sites', label: '사이트' },
      { to: '/projects', label: '프로젝트', match: ['/projects', '/apps', '/deployments'] },
    ],
  },
  {
    id: 'db',
    label: '데이터베이스',
    icon: DB_ICON,
    children: [
      { to: '/mariadb', label: 'MariaDB' },
      { to: '/mongodb', label: 'MongoDB' },
      { to: '/redis', label: 'Redis' },
    ],
  },
  {
    id: 'ops',
    label: '운영',
    icon: 'M5 4h14v16H5zM8 8h8M8 12h8M8 16h5',
    children: [
      { to: '/backups', label: '백업' },
      { to: '/logs', label: '로그' },
      { to: '/notifications', label: '알림' },
    ],
  },
  {
    id: 'system',
    label: '시스템',
    icon: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM4 12h2m12 0h2M12 4v2m0 12v2',
    children: [
      { to: '/runtimes', label: '런타임' },
      { to: '/settings', label: '설정' },
      { to: '/admins', label: '관리자' },
    ],
  },
]

const childMatches = (c, path) => (c.match ?? [c.to]).some((p) => path === p || path.startsWith(p + '/'))

/** 지금 화면이 속한 1차·2차 메뉴 */
export function currentNav(path) {
  for (const g of NAV) {
    if (!g.children) continue
    const c = g.children.find((x) => childMatches(x, path))
    if (c) return { group: g, item: c }
  }
  return null
}

const OPEN_KEY = 'rpanel.nav.open'
function loadOpen() {
  try {
    return JSON.parse(localStorage.getItem(OPEN_KEY) || 'null')
  } catch {
    return null
  }
}

function Icon({ d, className = 'h-4 w-4' }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={d} />
    </svg>
  )
}

function SideNav({ onNavigate }) {
  const { pathname } = useLocation()
  const active = currentNav(pathname)
  const [open, setOpen] = useState(() => loadOpen() ?? { web: true, db: true, ops: true, system: true })
  const toggle = (id) => {
    const next = { ...open, [id]: !open[id] }
    setOpen(next)
    try {
      localStorage.setItem(OPEN_KEY, JSON.stringify(next))
    } catch {
      /* 저장 못 해도 동작 */
    }
  }
  return (
    <nav className="flex flex-col gap-0.5" aria-label={t('주 메뉴')}>
      {NAV.map((n) =>
        n.children ? (
          <div key={n.id} className="mt-1">
            <button
              type="button"
              onClick={() => toggle(n.id)}
              aria-expanded={!!open[n.id] || active?.group.id === n.id}
              data-testid={`nav-group-${n.id}`}
              className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm transition ${active?.group.id === n.id ? 'text-ink-100' : 'text-ink-400 hover:bg-ink-850 hover:text-ink-100'}`}
            >
              <Icon d={n.icon} />
              <span className="flex-1 text-left">{t(n.label)}</span>
              <span aria-hidden="true" className={`text-[10px] transition ${open[n.id] || active?.group.id === n.id ? 'rotate-90' : ''}`}>
                ▶
              </span>
            </button>
            {(open[n.id] || active?.group.id === n.id) && (
              <div className="mt-0.5 ml-5 flex flex-col gap-0.5 border-l border-ink-800 pl-3">
                {n.children.map((c) => (
                  <NavLink
                    key={c.to}
                    to={c.to}
                    onClick={onNavigate}
                    className={() =>
                      `rounded-md px-2.5 py-1.5 text-sm transition ${active?.item === c ? 'bg-signal/10 font-medium text-signal' : 'text-ink-400 hover:bg-ink-850 hover:text-ink-100'}`
                    }
                  >
                    {t(c.label)}
                  </NavLink>
                ))}
              </div>
            )}
          </div>
        ) : (
          <NavLink
            key={n.to}
            to={n.to}
            end={n.end}
            onClick={onNavigate}
            className={({ isActive }) =>
              `flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition ${isActive ? 'bg-signal/10 font-medium text-signal' : 'text-ink-400 hover:bg-ink-850 hover:text-ink-100'}`
            }
          >
            <Icon d={n.icon} />
            {t(n.label)}
          </NavLink>
        ),
      )}
    </nav>
  )
}

/** 화면 위: 1차 › 2차 메뉴 위치와 같은 묶음의 다른 화면 (화면 구분) */
function SectionBar() {
  const { pathname } = useLocation()
  const cur = currentNav(pathname)
  if (!cur) return null
  return (
    <div className="mb-6 flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-ink-800 pb-3" data-testid="section-bar">
      <span className="flex items-center gap-2 text-xs font-medium tracking-wide text-ink-400 uppercase">
        <Icon d={cur.group.icon} className="h-3.5 w-3.5" />
        {t(cur.group.label)}
      </span>
      <nav className="flex flex-wrap gap-1" aria-label={t('{group} 메뉴', { group: t(cur.group.label) })}>
        {cur.group.children.map((c) => (
          <NavLink
            key={c.to}
            to={c.to}
            className={() =>
              `rounded-full px-3 py-1 text-xs transition ${cur.item === c ? 'bg-ink-800 font-medium text-ink-100 ring-1 ring-ink-600' : 'text-ink-400 hover:bg-ink-850 hover:text-ink-100'}`
            }
          >
            {t(c.label)}
          </NavLink>
        ))}
      </nav>
    </div>
  )
}

export const DONATE_URL = 'https://buymeacoffee.com/bandiuri'

function DonateLink({ className = '' }) {
  return (
    <a href={DONATE_URL} target="_blank" rel="noopener noreferrer" className={`inline-flex items-center gap-1.5 text-xs text-ink-400 hover:text-warn ${className}`}>
      <span aria-hidden="true">☕</span> {t('후원하기')}
    </a>
  )
}

export function Logo() {
  return (
    <div className="flex items-center gap-2.5">
      <svg viewBox="0 0 32 32" className="h-7 w-7" aria-hidden="true">
        <rect width="32" height="32" rx="7" fill="#141922" stroke="#252d39" />
        <path d="M9 23V9h7.5a4.5 4.5 0 0 1 0 9H13l7 5" fill="none" stroke="#34d399" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <span className="font-semibold tracking-tight">Rpanel</span>
    </div>
  )
}

const THEME_OPTIONS = [
  { id: 'light', label: '낮', icon: '☀' },
  { id: 'dark', label: '밤', icon: '☾' },
  { id: 'system', label: '자동', icon: '◐' },
]

/** 테마(낮·밤·자동) 와 언어를 바로 바꾸는 작은 스위치 */
export function ThemeLangSwitch({ compact = false }) {
  const lang = useLang()
  const [theme, setThemeState] = useState(getTheme())
  return (
    <div className={`flex items-center gap-2 ${compact ? '' : 'flex-wrap'}`}>
      <div className="inline-flex rounded-lg border border-ink-700 p-0.5" role="radiogroup" aria-label={t('테마')}>
        {THEME_OPTIONS.map((o) => (
          <button
            key={o.id}
            type="button"
            role="radio"
            aria-checked={theme === o.id}
            title={t(o.label)}
            aria-label={t(o.label)}
            onClick={() => {
              setTheme(o.id)
              setThemeState(o.id)
            }}
            className={`rounded-md px-2 py-0.5 text-xs transition ${theme === o.id ? 'bg-ink-800 text-ink-100' : 'text-ink-400 hover:text-ink-100'}`}
          >
            <span aria-hidden="true">{o.icon}</span>
          </button>
        ))}
      </div>
      <select
        aria-label={t('언어')}
        className="rounded-lg border border-ink-700 bg-ink-900 px-1.5 py-0.5 text-xs text-ink-300"
        value={lang}
        onChange={(e) => setLang(e.target.value)}
      >
        {LANGS.map((l) => (
          <option key={l.id} value={l.id}>
            {l.label}
          </option>
        ))}
      </select>
    </div>
  )
}

export default function Layout() {
  const { user, logout } = useAuth()
  const [open, setOpen] = useState(false)

  const nav = <SideNav onNavigate={() => setOpen(false)} />

  return (
    <div className="min-h-screen md:grid md:grid-cols-[220px_1fr]">
      <aside className="hidden border-r border-ink-800 bg-ink-900/60 md:flex md:min-h-screen md:flex-col md:px-3 md:py-5">
        <div className="flex items-center justify-between px-2 pb-6">
          <Logo />
          <NotificationBell />
        </div>
        {nav}
        <div className="mt-auto space-y-3 border-t border-ink-800 px-2 pt-4">
          <ThemeLangSwitch compact />
          <VersionBadge />
          <p className="truncate text-xs text-ink-400" title={user?.email}>
            {user?.email}
          </p>
          <div className="flex items-center justify-between">
            <button className="text-xs text-ink-400 hover:text-ink-100" onClick={logout}>
              {t('로그아웃')}
            </button>
            <DonateLink />
          </div>
        </div>
      </aside>

      <header className="flex items-center justify-between border-b border-ink-800 px-4 py-3 md:hidden">
        <Logo />
        <span className="ml-auto mr-2">
          <NotificationBell />
        </span>
        <button className="btn-ghost px-2.5 py-1.5" onClick={() => setOpen((o) => !o)} aria-expanded={open} aria-label={t('메뉴')}>
          ☰
        </button>
      </header>
      {open && (
        <div className="border-b border-ink-800 px-3 py-3 md:hidden">
          {nav}
          <div className="mt-3 px-3">
            <ThemeLangSwitch />
          </div>
          <div className="mt-2 flex items-center justify-between px-3">
            <button className="text-sm text-ink-400" onClick={logout}>
              {t('로그아웃')}
            </button>
            <DonateLink />
          </div>
        </div>
      )}

      <main className="min-w-0 px-4 py-6 md:px-8 md:py-8">
        <div className="mx-auto max-w-6xl">
          <StaleBundleBanner />
          <SectionBar />
          <Outlet />
        </div>
      </main>
    </div>
  )
}
