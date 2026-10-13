import { Link, useParams } from 'react-router'
import LogViewer from '../components/LogViewer'
import { ErrorBox, PageHeader, Spinner, StatusBadge } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { get, post } from '../lib/api'
import { dateTime, duration, shortSha } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { useDeploymentLog } from '../lib/useSockets'
import { useEffect } from 'react'

export default function DeploymentView() {
  const { id } = useParams()
  const toast = useToast()
  const d = useFetch(() => get(`/deployments/${id}`), [id])
  const { lines, status, connected } = useDeploymentLog(id, d.data?.status)

  // 상태가 바뀌면(종료) 상세 다시 조회 → 소요 시간/에러 반영
  useEffect(() => {
    if (status && d.data && status !== d.data.status) d.reload(true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status])

  if (d.loading) return <Spinner className="text-ink-400" />
  if (d.error) return <ErrorBox error={d.error} />
  const dep = d.data
  const live = status ?? dep.status
  const active = ['queued', 'running'].includes(live)

  return (
    <>
      <nav className="mb-3 text-xs text-ink-400">
        <Link to={`/apps/${dep.app_id}`} className="hover:text-ink-100">
          {dep.app_name}
        </Link>{' '}
        / 배포 #{dep.id}
      </nav>
      <PageHeader
        title={
          <span className="flex items-center gap-3">
            배포 #{dep.id} <StatusBadge status={live} deployment />
          </span>
        }
        actions={
          <>
            {active && (
              <button
                className="btn-danger"
                onClick={async () => {
                  try {
                    await post(`/deployments/${dep.id}/cancel`)
                    toast.push('취소 요청을 보냈습니다')
                  } catch (e) {
                    toast.push(e.message, 'error')
                  }
                }}
              >
                취소
              </button>
            )}
            <Link to={`/apps/${dep.app_id}`} className="btn-ghost">
              앱으로
            </Link>
          </>
        }
      >
        <dl className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-xs text-ink-400">
          <div>
            커밋 <span className="font-mono text-ink-300">{shortSha(dep.commit_sha)}</span>
          </div>
          <div>시작 {dateTime(dep.started_at ?? dep.created_at)}</div>
          <div>소요 {duration(dep.started_at, dep.finished_at)}</div>
          {active && <div className={connected ? 'text-signal' : ''}>{connected ? '● 실시간 스트리밍' : '연결 중…'}</div>}
        </dl>
      </PageHeader>
      {dep.error && !active && (
        <div className="mb-4">
          <ErrorBox error={{ message: dep.error }} />
        </div>
      )}
      <LogViewer lines={lines} emptyText={active ? '로그를 기다리는 중…' : '로그가 없습니다'} />
    </>
  )
}
