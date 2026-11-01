import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { t } from '../i18n'
import { get } from '../lib/api'
import { duration } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { CancelDeployButton } from './DeploymentLog'
import { StatusBadge } from './ui'

/**
 * 왼쪽 메뉴 위 '진행 중인 배포' 표시 — 모든 앱의 대기·실행 중 배포를 한곳에서 확인하고 중지
 * (앱 화면을 떠나거나, 삭제하려다 막혔을 때도 여기서 상태를 볼 수 있음)
 */
export default function ActiveDeployments() {
  const list = useFetch(() => get('/logs/deployments?active=true'), [])
  const items = list.data ?? []
  const n = items.length
  // 진행 중이면 자주, 아니면 가끔 확인
  useInterval(() => list.reload(true), n ? 4000 : 15000)
  useEffect(() => {
    const on = () => {
      list.reload(true)
      // 배포가 대기 → 실행으로 넘어가는 순간도 잡도록 한 번 더
      setTimeout(() => list.reload(true), 1500)
    }
    window.addEventListener('rpanel:deployments', on)
    return () => window.removeEventListener('rpanel:deployments', on)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const [open, setOpen] = useState(false)
  const ref = useRef(null)
  const [now, setNow] = useState(() => new Date().toISOString())
  useInterval(() => setNow(new Date().toISOString()), 1000, open && n > 0)

  useEffect(() => {
    if (!open) return
    // 중지 확인 창이 떠 있는 동안에는 닫지 않음
    const onDown = (e) => ref.current && !ref.current.contains(e.target) && !document.querySelector('[aria-modal="true"]') && setOpen(false)
    const onKey = (e) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  return (
    <div ref={ref} className="relative">
      <button
        className={`relative inline-flex h-8 w-8 items-center justify-center rounded-lg hover:bg-ink-850 ${n ? 'text-sky-300' : 'text-ink-400 hover:text-ink-100'}`}
        aria-label={n ? t('진행 중인 배포 {n}개', { n }) : t('진행 중인 배포')}
        title={n ? t('진행 중인 배포 {n}개', { n }) : t('진행 중인 배포')}
        aria-expanded={open}
        onClick={() => {
          setOpen((o) => !o)
          if (!open) list.reload(true)
        }}
      >
        {n > 0 && <span className="absolute inset-1 animate-spin rounded-full border-2 border-sky-400/20 border-t-sky-400" aria-hidden="true" />}
        <svg viewBox="0 0 24 24" className="h-[16px] w-[16px]" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M12 19V5M5 12l7-7 7 7" />
        </svg>
        {n > 0 && (
          <span
            data-testid="active-deploy-count"
            className="absolute -top-0.5 -right-0.5 min-w-4 rounded-full bg-sky-500 px-1 text-center text-[10px] leading-4 font-semibold text-white"
          >
            {n > 99 ? '99+' : n}
          </span>
        )}
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={t('진행 중인 배포')}
          className="absolute top-full right-0 z-40 mt-2 w-80 max-w-[90vw] rounded-xl border border-ink-700 bg-ink-900 shadow-2xl shadow-black/40 md:right-auto md:left-0"
        >
          <div className="border-b border-ink-700/70 px-4 py-2.5">
            <p className="text-sm font-medium">{t('진행 중인 배포')}</p>
          </div>
          <ul className="max-h-96 divide-y divide-ink-800 overflow-auto">
            {items.map((d) => (
              <li key={d.id} className="flex items-center gap-2.5 px-4 py-2.5">
                <StatusBadge status={d.status} deployment />
                <Link to={`/deployments/${d.id}`} className="min-w-0 flex-1 hover:text-signal" onClick={() => setOpen(false)}>
                  <span className="block truncate text-sm">
                    {d.app_name} <span className="text-ink-400">#{d.id}</span>
                  </span>
                  <span className="block text-[11px] text-ink-400">{duration(d.created_at, now)}</span>
                </Link>
                <CancelDeployButton id={d.id} className="btn-danger px-2 py-0.5 text-xs" onDone={() => list.reload(true)} />
              </li>
            ))}
            {list.data && !n && <li className="px-4 py-8 text-center text-sm text-ink-400">{t('진행 중인 배포가 없습니다')}</li>}
          </ul>
          <div className="border-t border-ink-700/70 px-4 py-2 text-xs">
            <Link to="/logs?tab=deployments" className="text-ink-300 hover:text-ink-100" onClick={() => setOpen(false)}>
              {t('모든 배포 기록')}
            </Link>
          </div>
        </div>
      )}
    </div>
  )
}
