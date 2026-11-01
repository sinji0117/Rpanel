import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { useToast } from '../context/ToastContext'
import { t } from '../i18n'
import { get, post } from '../lib/api'
import { dateTime, duration, shortSha } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { useDeploymentLog } from '../lib/useSockets'
import LogViewer from './LogViewer'
import { ErrorBox, Loading, Spinner, StatusBadge } from './ui'
import { confirmDialog } from './Dialog'

/** 진행 중(queued·running)인 배포를 중지 — 확인을 받은 뒤 취소 요청. 보냈으면 true */
export async function cancelDeployment(id, toast) {
  if (!(await confirmDialog(t('진행 중인 작업을 중지할까요? 지금 실행 중인 버전은 그대로 유지됩니다.'), { title: t('배포 중지'), confirmText: t('중지'), danger: true }))) return false
  try {
    await post(`/deployments/${id}/cancel`)
    toast.push(t('취소 요청을 보냈습니다'))
    return true
  } catch (e) {
    toast.push(e.message, 'error')
    return false
  }
}

/** 진행 중인 배포 하나를 중지하는 버튼 */
export function CancelDeployButton({ id, className = 'btn-danger', children, onDone }) {
  const toast = useToast()
  const [busy, setBusy] = useState(false)
  return (
    <button
      type="button"
      className={className}
      disabled={busy}
      onClick={async (e) => {
        e.stopPropagation()
        setBusy(true)
        try {
          if (await cancelDeployment(id, toast)) onDone?.()
        } finally {
          setBusy(false)
        }
      }}
    >
      {busy ? <Spinner /> : (children ?? t('중지'))}
    </button>
  )
}

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

  if (d.loading) return <Loading compact className="card" />
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
        <span className="ml-auto flex items-center gap-3">
          {active && (
            <CancelDeployButton id={dep.id} className="btn-danger px-2.5 py-1 text-xs" onDone={() => d.reload(true)}>
              ■ {t('중지')}
            </CancelDeployButton>
          )}
          <Link to={`/deployments/${dep.id}`} className="hover:text-ink-100">
            {t('전체 화면 ↗')}
          </Link>
        </span>
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
