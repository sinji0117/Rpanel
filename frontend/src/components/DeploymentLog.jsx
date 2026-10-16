import { useEffect } from 'react'
import { Link } from 'react-router'
import { t } from '../i18n'
import { get } from '../lib/api'
import { dateTime, duration, shortSha } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { useDeploymentLog } from '../lib/useSockets'
import LogViewer from './LogViewer'
import { ErrorBox, Spinner, StatusBadge } from './ui'

/** 배포 하나의 로그 (진행 중이면 실시간) — 앱 화면 안에서 바로 보기 */
export default function DeploymentLogPanel({ id, onFinished, height = 'h-[50vh]' }) {
  const d = useFetch(() => get(`/deployments/${id}`), [id])
  const { lines, status, connected } = useDeploymentLog(id, d.data?.status)

  useEffect(() => {
    if (status && d.data && status !== d.data.status) {
      d.reload(true)
      if (!['queued', 'running'].includes(status)) onFinished?.(status)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status])

  if (d.loading) return <Spinner className="text-ink-400" />
  if (d.error) return <ErrorBox error={d.error} />
  const dep = d.data
  const live = status ?? dep.status
  const active = ['queued', 'running'].includes(live)
  return (
    <section className="card p-4" aria-label={t('배포 #{id} 로그', { id: dep.id })}>
      <div className="mb-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-ink-400">
        <span className="flex items-center gap-2 text-sm font-medium text-ink-100">
          {t('배포 #{id} 로그', { id: dep.id })} <StatusBadge status={live} deployment />
        </span>
        <span>
          {t('커밋')} <span className="font-mono text-ink-300">{shortSha(dep.commit_sha)}</span>
        </span>
        <span>{dateTime(dep.started_at ?? dep.created_at)}</span>
        <span>{t('소요 {duration}', { duration: duration(dep.started_at, dep.finished_at) })}</span>
        {active && <span className={connected ? 'text-signal' : ''}>{connected ? t('● 실시간 스트리밍') : t('연결 중…')}</span>}
        <Link to={`/deployments/${dep.id}`} className="ml-auto hover:text-ink-100">
          {t('전체 화면 ↗')}
        </Link>
      </div>
      {dep.error && !active && (
        <div className="mb-3">
          <ErrorBox error={{ message: dep.error }} />
        </div>
      )}
      <LogViewer lines={lines} height={height} emptyText={active ? t('로그를 기다리는 중…') : t('로그가 없습니다')} />
    </section>
  )
}
