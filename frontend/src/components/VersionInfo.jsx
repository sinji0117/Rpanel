import { useState } from 'react'
import { Link } from 'react-router'
import { t } from '../i18n'
import { get } from '../lib/api'
import { ago } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { CopyButton } from './ui'

export const UPDATE_CMD = 'sudo rpanel-update'

export function useVersion() {
  return useFetch(() => get('/system/version'), [])
}

/** 대시보드 시스템 카드: 지금 버전 · GitHub 최신 버전 · 업데이트 방법 */
export function VersionRows() {
  const v = useVersion()
  const [checking, setChecking] = useState(false)
  const d = v.data
  return (
    <>
      <dt className="text-ink-400">Rpanel</dt>
      <dd className="font-mono" data-testid="version-current">
        v{d?.current ?? '…'}
      </dd>
      <dt className="text-ink-400">GitHub</dt>
      <dd className="space-y-1">
        <div className="flex flex-wrap items-center gap-2">
          {d?.latest ? (
            <a href={d.repo_url} target="_blank" rel="noopener noreferrer" className="font-mono hover:text-signal" data-testid="version-latest">
              v{d.latest}
            </a>
          ) : (
            <span className="text-xs text-ink-400">{d?.repo_url ? t('확인 못 함') : t('확인 안 함')}</span>
          )}
          {d?.update_available ? (
            <span className="rounded border border-warn/50 px-1.5 text-[11px] text-warn">{t('새 버전 있음')}</span>
          ) : (
            d?.latest && <span className="rounded border border-signal/40 px-1.5 text-[11px] text-signal">{t('최신')}</span>
          )}
          {d?.repo_url && (
            <button
              className="text-[11px] text-ink-400 hover:text-ink-100 disabled:opacity-50"
              disabled={checking}
              onClick={async () => {
                setChecking(true)
                try {
                  v.setData(await get('/system/version?refresh=true'))
                } catch {
                  /* 화면은 그대로 */
                } finally {
                  setChecking(false)
                }
              }}
            >
              {checking ? t('확인 중…') : t('다시 확인')}
            </button>
          )}
        </div>
        {d?.error && <p className="text-[11px] text-ink-400">{d.error}</p>}
        {d?.checked_at && !d.error && <p className="text-[11px] text-ink-400">{t('{when} 확인', { when: ago(d.checked_at) })}</p>}
        {d?.update_available && (
          <p className="flex flex-wrap items-center gap-1.5 text-[11px] text-ink-300">
            {t('서버에서 실행:')} <span className="kbd whitespace-nowrap">{UPDATE_CMD}</span> <CopyButton text={UPDATE_CMD} />
          </p>
        )}
      </dd>
    </>
  )
}

/** 왼쪽 메뉴 아래: 버전 + 새 버전 알림 */
export function VersionBadge() {
  const v = useVersion()
  const d = v.data
  if (!d) return null
  return (
    <Link to="/" className="flex items-center gap-1.5 text-[11px] text-ink-400 hover:text-ink-100" title={d.latest ? t('GitHub 최신 v{v}', { v: d.latest }) : undefined}>
      <span className="font-mono">v{d.current}</span>
      {d.update_available && (
        <span className="rounded border border-warn/50 px-1 text-warn" data-testid="update-badge">
          {t('새 버전 v{v}', { v: d.latest })}
        </span>
      )}
    </Link>
  )
}

/* global __APP_VERSION__ */
export const BUNDLE_VERSION = typeof __APP_VERSION__ === 'undefined' ? null : __APP_VERSION__

/**
 * 서버(패널)는 업데이트됐는데 브라우저가 옛 화면을 들고 있으면 한 번 새로 고침.
 * 그래도 다르면(예: 서버 쪽 화면 빌드가 늦음) 위쪽에 안내 띠를 보여 줌.
 */
export function StaleBundleBanner() {
  const v = useVersion()
  const server = v.data?.current
  if (!server || !BUNDLE_VERSION || server === BUNDLE_VERSION) return null
  let reloaded = null
  try {
    reloaded = sessionStorage.getItem('rpanel.reloadedFor')
    if (reloaded !== server) {
      sessionStorage.setItem('rpanel.reloadedFor', server)
      window.location.reload()
      return null
    }
  } catch {
    /* 저장소를 못 쓰면 안내만 */
  }
  return (
    <div role="status" className="mb-4 flex flex-wrap items-center gap-3 rounded-lg border border-sky-400/40 bg-sky-400/10 px-4 py-2.5 text-sm" data-testid="stale-bundle">
      {t('패널이 v{server} 로 업데이트됐습니다. 지금 화면은 v{bundle} 입니다.', { server, bundle: BUNDLE_VERSION })}
      <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => window.location.reload()}>
        {t('새로 고침')}
      </button>
    </div>
  )
}
