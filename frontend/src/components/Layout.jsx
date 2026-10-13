import { useState } from 'react'
import { NavLink, Outlet } from 'react-router'
import { useAuth } from '../context/AuthContext'

const NAV = [
  { to: '/', label: '대시보드', icon: 'M3 12h4l3-8 4 16 3-8h4', end: true },
  { to: '/projects', label: '프로젝트', icon: 'M3 7h7l2 2h9v10H3z' },
  { to: '/settings', label: '설정', icon: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM4 12h2m12 0h2M12 4v2m0 12v2' },
]

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

export default function Layout() {
  const { user, logout } = useAuth()
  const [open, setOpen] = useState(false)

  const nav = (
    <nav className="flex flex-col gap-0.5" aria-label="주 메뉴">
      {NAV.map((n) => (
        <NavLink
          key={n.to}
          to={n.to}
          end={n.end}
          onClick={() => setOpen(false)}
          className={({ isActive }) =>
            `flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition ${
              isActive ? 'bg-ink-800 text-ink-100' : 'text-ink-400 hover:bg-ink-850 hover:text-ink-100'
            }`
          }
        >
          <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d={n.icon} />
          </svg>
          {n.label}
        </NavLink>
      ))}
    </nav>
  )

  return (
    <div className="min-h-screen md:grid md:grid-cols-[220px_1fr]">
      <aside className="hidden border-r border-ink-800 bg-ink-900/60 md:flex md:min-h-screen md:flex-col md:px-3 md:py-5">
        <div className="px-2 pb-6">
          <Logo />
        </div>
        {nav}
        <div className="mt-auto border-t border-ink-800 px-2 pt-4">
          <p className="truncate text-xs text-ink-400" title={user?.email}>
            {user?.email}
          </p>
          <button className="mt-2 text-xs text-ink-400 hover:text-ink-100" onClick={logout}>
            로그아웃
          </button>
        </div>
      </aside>

      <header className="flex items-center justify-between border-b border-ink-800 px-4 py-3 md:hidden">
        <Logo />
        <button className="btn-ghost px-2.5 py-1.5" onClick={() => setOpen((o) => !o)} aria-expanded={open} aria-label="메뉴">
          ☰
        </button>
      </header>
      {open && (
        <div className="border-b border-ink-800 px-3 py-3 md:hidden">
          {nav}
          <button className="mt-2 px-3 text-sm text-ink-400" onClick={logout}>
            로그아웃
          </button>
        </div>
      )}

      <main className="min-w-0 px-4 py-6 md:px-8 md:py-8">
        <div className="mx-auto max-w-6xl">
          <Outlet />
        </div>
      </main>
    </div>
  )
}
