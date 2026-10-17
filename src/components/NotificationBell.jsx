import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { t } from '../i18n'
import { get, post } from '../lib/api'
import { ago } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'

export const LEVEL = {
  error: { dot: 'bg-fail', text: 'text-fail' },
  warning: { dot: 'bg-warn', text: 'text-warn' },
  success: { dot: 'bg-signal', text: 'text-signal' },
  info: { dot: 'bg-sky-400', text: 'text-sky-300' },
}

/** 알림 제목: 한국어 원문 템플릿을 번역하고 params 로 채움 */
export function notificationTitle(n) {
  return t(n.title, n.params ?? {})
}

/** 알림을 읽거나 지웠을 때 종 아이콘을 바로 갱신 */
export function notificationsChanged() {
  window.dispatchEvent(new Event('rpanel:notifications'))
}

/** 왼쪽 메뉴 위 종 아이콘: 안 읽은 개수 + 최근 알림 */
export default function NotificationBell() {
  const nav = useNavigate()
  const list = useFetch(() => get('/notifications?limit=8'), [])
  useInterval(() => list.reload(true), 30000)
  useEffect(() => {
    const on = () => list.reload(true)
    window.addEventListener('rpanel:notifications', on)
    return () => window.removeEventListener('rpanel:notifications', on)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  const [open, setOpen] = useState(false)
  const ref = useRef(null)
  const unread = list.data?.unread ?? 0

  useEffect(() => {
    if (!open) return
    const onDown = (e) => ref.current && !ref.current.contains(e.target) && setOpen(false)
    const onKey = (e) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  const readAll = async () => {
    try {
      const r = await post('/notifications/read', { all: true })
      list.setData((d) => (d ? { ...d, unread: r.unread, items: d.items.map((n) => ({ ...n, read_at: n.read_at ?? new Date().toISOString() })) } : d))
    } catch {
      /* 다음 갱신 때 다시 */
    }
  }

  return (
    <div ref={ref} className="relative">
      <button
        className="relative inline-flex h-8 w-8 items-center justify-center rounded-lg text-ink-400 hover:bg-ink-850 hover:text-ink-100"
        aria-label={unread ? t('알림 {n}개 안 읽음', { n: unread }) : t('알림')}
        aria-expanded={open}
        onClick={() => {
          setOpen((o) => !o)
          if (!open) list.reload(true)
        }}
      >
        <svg viewBox="0 0 24 24" className="h-[18px] w-[18px]" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M6 8a6 6 0 1 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0" />
        </svg>
        {unread > 0 && (
          <span
            data-testid="notification-count"
            className="absolute -top-0.5 -right-0.5 min-w-4 rounded-full bg-fail px-1 text-center text-[10px] leading-4 font-semibold text-white"
          >
            {unread > 99 ? '99+' : unread}
          </span>
        )}
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={t('알림')}
          className="absolute top-full left-0 z-40 mt-2 w-80 max-w-[90vw] rounded-xl border border-ink-700 bg-ink-900 shadow-2xl shadow-black/40"
        >
          <div className="flex items-center justify-between border-b border-ink-700/70 px-4 py-2.5">
            <p className="text-sm font-medium">{t('알림')}</p>
            {unread > 0 && (
              <button className="text-xs text-ink-400 hover:text-ink-100" onClick={readAll}>
                {t('모두 읽음')}
              </button>
            )}
          </div>
          <ul className="max-h-96 divide-y divide-ink-800 overflow-auto">
            {list.data?.items.map((n) => (
              <li key={n.id}>
                <button
                  className={`flex w-full gap-2.5 px-4 py-2.5 text-left hover:bg-ink-850 ${n.read_at ? 'opacity-60' : ''}`}
                  onClick={async () => {
                    setOpen(false)
                    if (!n.read_at)
                      post('/notifications/read', { ids: [n.id] }).then(
                        () => list.reload(true),
                        () => {},
                      )
                    if (n.link) nav(n.link)
                  }}
                >
                  <span className={`mt-1.5 h-2 w-2 shrink-0 rounded-full ${LEVEL[n.level]?.dot ?? 'bg-ink-400'}`} />
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm">{notificationTitle(n)}</span>
                    {n.body && <span className="mt-0.5 block truncate text-xs text-ink-400">{n.body}</span>}
                    <span className="mt-0.5 block text-[11px] text-ink-600">{ago(n.created_at)}</span>
                  </span>
                </button>
              </li>
            ))}
            {list.data && !list.data.items.length && <li className="px-4 py-8 text-center text-sm text-ink-400">{t('알림이 없습니다')}</li>}
          </ul>
          <div className="flex items-center justify-between border-t border-ink-700/70 px-4 py-2 text-xs">
            <Link to="/notifications" className="text-ink-300 hover:text-ink-100" onClick={() => setOpen(false)}>
              {t('모두 보기')}
            </Link>
            <Link to="/notifications?tab=settings" className="text-ink-400 hover:text-ink-100" onClick={() => setOpen(false)}>
              {t('알림 설정')}
            </Link>
          </div>
        </div>
      )}
    </div>
  )
}
