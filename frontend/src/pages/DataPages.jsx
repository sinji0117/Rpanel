import { Link } from 'react-router'
import { Empty, ErrorBox, PageHeader, Spinner } from '../components/ui'
import { get } from '../lib/api'
import { dateTime } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { BackupTable } from './AppTabs'

export function DatabasesPage() {
  const d = useFetch(() => get('/databases'), [])
  if (d.loading) return <Spinner className="text-ink-400" />
  if (d.error) return <ErrorBox error={d.error} />
  const { databases, redis, redis_range, panel_redis_db } = d.data
  return (
    <>
      <PageHeader title="데이터베이스" subtitle="패널이 만든 MariaDB 데이터베이스와 앱별 Redis DB 배정 현황입니다. 생성·삭제는 각 앱의 데이터베이스 탭에서 합니다." />
      <section className="card mb-4">
        <h2 className="border-b border-ink-700/70 px-5 py-3.5 text-sm font-medium">MariaDB</h2>
        {databases.length === 0 ? (
          <p className="px-5 py-8 text-center text-sm text-ink-400">패널이 만든 데이터베이스가 없습니다</p>
        ) : (
          <table className="w-full text-sm">
            <thead className="text-left text-xs text-ink-400">
              <tr>
                <th className="px-5 py-2.5 font-medium">DB</th>
                <th className="px-3 py-2.5 font-medium">사용자</th>
                <th className="px-3 py-2.5 font-medium">환경변수</th>
                <th className="px-3 py-2.5 font-medium">앱</th>
                <th className="px-5 py-2.5 font-medium">생성</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-ink-800">
              {databases.map((db) => (
                <tr key={db.id}>
                  <td className="px-5 py-2.5 font-mono">{db.name}</td>
                  <td className="px-3 py-2.5 font-mono text-xs text-ink-300">{db.username}</td>
                  <td className="px-3 py-2.5">
                    <span className="kbd">{db.env_key}</span>
                  </td>
                  <td className="px-3 py-2.5">
                    <Link to={`/apps/${db.app_id}`} className="hover:text-signal">
                      {db.app_name}
                    </Link>
                  </td>
                  <td className="px-5 py-2.5 text-xs text-ink-400">{dateTime(db.created_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      <section className="card">
        <h2 className="border-b border-ink-700/70 px-5 py-3.5 text-sm font-medium">
          Redis <span className="ml-2 text-xs font-normal text-ink-400">자동 배정 범위 DB {redis_range.join('–')} · 패널 DB {panel_redis_db} · 0–3 은 수동용으로 비워 둠</span>
        </h2>
        {redis.length === 0 ? (
          <p className="px-5 py-8 text-center text-sm text-ink-400">배정된 Redis DB가 없습니다</p>
        ) : (
          <ul className="divide-y divide-ink-800">
            {redis.map((r) => (
              <li key={r.app_id} className="flex items-center gap-4 px-5 py-2.5 text-sm">
                <span className="w-16 font-mono">DB {r.db}</span>
                <Link to={`/apps/${r.app_id}`} className="flex-1 hover:text-signal">
                  {r.app_name}
                </Link>
                <span className="text-xs text-ink-400">키 {r.keys ?? '?'}개</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  )
}

export function BackupsPage() {
  const list = useFetch(() => get('/backups?limit=300'), [])
  useInterval(() => list.reload(true), 3000, !!list.data?.some((b) => b.status === 'running'))
  return (
    <>
      <PageHeader title="백업" subtitle="모든 앱의 백업 파일 (/var/lib/rpanel/backups). 지금 백업·예약 설정은 각 앱의 백업 탭에서 합니다." />
      <ErrorBox error={list.error} />
      {!list.loading && !list.data?.length ? (
        <Empty title="백업이 없습니다">앱 → 백업 탭에서 “지금 백업”을 누르거나 매일/매주 예약을 켜세요.</Empty>
      ) : (
        <BackupTable rows={list.data} loading={list.loading} onChanged={() => list.reload(true)} showApp />
      )}
    </>
  )
}
