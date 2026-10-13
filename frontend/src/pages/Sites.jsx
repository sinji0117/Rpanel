import { Link } from 'react-router'
import { Empty, ErrorBox, KindBadge, PageHeader, Spinner, StatusBadge } from '../components/ui'
import { get } from '../lib/api'
import { ago, httpClass } from '../lib/format'
import { useFetch } from '../lib/useFetch'

export default function Sites() {
  const sites = useFetch(() => get('/sites'), [])
  const rows = sites.data ?? []
  const down = rows.filter((s) => s.app_status !== 'idle' && (s.http_status == null || s.http_status >= 500)).length

  return (
    <>
      <PageHeader
        title="사이트"
        subtitle={
          rows.length ? `도메인 ${rows.length}개 · ${down ? `응답 이상 ${down}개` : '모두 응답 중'}  —  VM102 Nginx(:80)에 Host 헤더로 직접 확인합니다` : '도메인별 서비스 상태'
        }
        actions={
          <button className="btn-ghost" onClick={() => sites.reload()} disabled={sites.loading}>
            {sites.loading ? <Spinner /> : '다시 확인'}
          </button>
        }
      />
      <ErrorBox error={sites.error} />
      {sites.loading && !sites.data ? (
        <Spinner className="text-ink-400" />
      ) : rows.length === 0 ? (
        <Empty title="도메인이 연결된 앱이 없습니다">앱 설정에서 도메인을 넣고 배포하면 여기 나타납니다. CT100(NPM)에 Proxy Host → 10.0.0.12:80 도 등록하세요.</Empty>
      ) : (
        <div className="card overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="border-b border-ink-700/70 text-left text-xs text-ink-400">
              <tr>
                <th className="px-5 py-3 font-medium">도메인</th>
                <th className="px-3 py-3 font-medium">응답</th>
                <th className="px-3 py-3 font-medium">앱</th>
                <th className="px-3 py-3 font-medium">상태</th>
                <th className="px-5 py-3 font-medium">최근 배포</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-ink-800">
              {rows.map((s) => (
                <tr key={s.domain} className="hover:bg-ink-850">
                  <td className="px-5 py-3">
                    <a href={`https://${s.domain}`} target="_blank" rel="noreferrer" className="font-mono hover:text-signal">
                      {s.domain} ↗
                    </a>
                  </td>
                  <td className="px-3 py-3 font-mono text-xs" title={s.error ?? ''}>
                    <span className={httpClass(s.http_status)}>{s.http_status ?? (s.error?.includes('dry-run') || s.app_status === 'idle' ? '—' : '응답 없음')}</span>
                    {s.latency_ms != null && <span className="ml-2 text-ink-400">{s.latency_ms}ms</span>}
                  </td>
                  <td className="px-3 py-3">
                    <Link to={`/apps/${s.app_id}`} className="flex items-center gap-2 hover:text-signal">
                      {s.app_name} <KindBadge kind={s.kind} />
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
      <p className="mt-4 text-xs text-ink-400">
        응답 코드는 VM102 내부에서 확인한 값입니다. 외부(인터넷) 접속은 CT100 의 Proxy Host·인증서와 DNS 까지 맞아야 합니다 (Vol.4 §10).
      </p>
    </>
  )
}
