import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import CreateSiteModal from '../components/CreateSiteModal'
import { Empty, ErrorBox, KindBadge, PageHeader, Spinner, StatusBadge } from '../components/ui'
import { get } from '../lib/api'
import { ago, httpClass } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { t } from '../i18n'

export default function Sites() {
  const sites = useFetch(() => get('/sites'), [])
  const [params, setParams] = useSearchParams()
  const [creating, setCreatingRaw] = useState(params.get('new') === '1')
  const setCreating = (v) => {
    setCreatingRaw(v)
    if (!v && params.get('new')) setParams({}, { replace: true })
  }
  const rows = sites.data ?? []
  const down = rows.filter((s) => s.app_status !== 'idle' && (s.http_status == null || s.http_status >= 500)).length

  return (
    <>
      <PageHeader
        title={t('사이트')}
        subtitle={
          rows.length
            ? t('도메인 {length}개 · {v} — 이 서버의 Nginx 에 직접 요청해 확인합니다', {
                length: rows.length,

                v: down ? t('응답 이상 {down}개', { down }) : t('모두 응답 중'),
              })
            : t('Rust · Python · HTML · 리버스 프록시 사이트를 만들고 관리합니다')
        }
        actions={
          <>
            <button className="btn-ghost" onClick={() => sites.reload()} disabled={sites.loading}>
              {sites.loading ? <Spinner /> : t('다시 확인')}
            </button>
            <button className="btn-primary" onClick={() => setCreating(true)}>
              {t('+ 사이트 추가')}
            </button>
          </>
        }
      />
      <ErrorBox error={sites.error} />
      {sites.loading && !sites.data ? (
        <Spinner className="text-ink-400" />
      ) : rows.length === 0 ? (
        <Empty
          title={t('사이트가 없습니다')}
          action={
            <button className="btn-primary" onClick={() => setCreating(true)}>
              {t('첫 사이트 만들기')}
            </button>
          }
        >
          {t('도메인을 입력하면 사이트 유저·폴더·Nginx 설정이 만들어집니다. 파일은 파일 관리자나 SFTP 로 올리세요.')}
        </Empty>
      ) : (
        <div className="card overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="border-b border-ink-700/70 text-left text-xs text-ink-400">
              <tr>
                <th className="px-5 py-3 font-medium">{t('도메인')}</th>
                <th className="px-3 py-3 font-medium">{t('응답')}</th>
                <th className="px-3 py-3 font-medium">{t('사이트')}</th>
                <th className="px-3 py-3 font-medium">{t('상태')}</th>
                <th className="px-5 py-3 font-medium">{t('최근 적용')}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-ink-800">
              {rows.map((s) => (
                <tr key={s.domain} className="hover:bg-ink-850">
                  <td className="px-5 py-3">
                    <a href={`${s.https ? 'https' : 'http'}://${s.domain}`} target="_blank" rel="noreferrer" className="font-mono hover:text-signal">
                      {s.https && (
                        <span className="mr-1 text-signal" title="HTTPS">
                          🔒
                        </span>
                      )}
                      {s.domain} ↗
                    </a>
                  </td>
                  <td className="px-3 py-3 font-mono text-xs" title={s.error ?? ''}>
                    <span className={httpClass(s.http_status)}>{s.http_status ?? (s.error?.includes('dry-run') || s.app_status === 'idle' ? '—' : t('응답 없음'))}</span>
                    {s.latency_ms != null && <span className="ml-2 text-ink-400">{s.latency_ms}ms</span>}
                  </td>
                  <td className="px-3 py-3">
                    <Link to={`/apps/${s.app_id}`} className="flex items-center gap-2 hover:text-signal">
                      {s.app_name} <KindBadge kind={s.kind} source={s.source} />
                    </Link>
                  </td>
                  <td className="px-3 py-3">
                    <StatusBadge status={s.app_status} />
                  </td>
                  <td className="px-5 py-3 text-xs text-ink-400">
                    {s.last_deploy_status ? (
                      <span className="flex items-center gap-2">
                        <StatusBadge status={s.last_deploy_status} deployment /> {ago(s.last_deploy_at)}
                      </span>
                    ) : (
                      '–'
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="mt-4 text-xs text-ink-400">{t('응답 코드는 서버 안에서 확인한 값입니다. 인터넷에서 접속되려면 도메인 DNS 가 이 서버(또는 앞단 프록시)를 가리켜야 합니다.')}</p>
      {creating && <CreateSiteModal open onClose={() => setCreating(false)} />}
    </>
  )
}
