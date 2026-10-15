import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import AppForm from '../components/AppForm'
import { Empty, ErrorBox, KindBadge, Modal, PageHeader, Spinner, StatusBadge } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { del, get, post, put } from '../lib/api'
import { shortSha } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { ProjectFormModal } from './Projects'

export default function ProjectDetail() {
  const { id } = useParams()
  const nav = useNavigate()
  const toast = useToast()
  const project = useFetch(() => get(`/projects/${id}`), [id])
  const apps = useFetch(() => get(`/apps?project_id=${id}`), [id])
  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState(false)
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)

  useInterval(() => apps.reload(true), 5000, !!apps.data?.some((a) => a.status === 'deploying'))

  if (project.loading) return <Spinner className="text-ink-400" />
  if (project.error) return <ErrorBox error={project.error} />
  const p = project.data

  return (
    <>
      <nav className="mb-3 text-xs text-ink-400">
        <Link to="/projects" className="hover:text-ink-100">
          프로젝트
        </Link>{' '}
        / {p.name}
      </nav>
      <PageHeader
        title={p.name}
        subtitle={p.description}
        actions={
          <>
            <button className="btn-ghost" onClick={() => setEditing(true)}>
              수정
            </button>
            <button
              className="btn-ghost"
              onClick={async () => {
                if (!window.confirm(`프로젝트 '${p.name}' 를 삭제할까요?`)) return
                try {
                  await del(`/projects/${id}`)
                  toast.push('프로젝트 삭제됨', 'success')
                  nav('/projects')
                } catch (e) {
                  toast.push(e.message, 'error')
                }
              }}
            >
              삭제
            </button>
            <button className="btn-primary" onClick={() => setCreating(true)}>
              + 새 앱
            </button>
          </>
        }
      />

      {apps.data?.length ? (
        <div className="card overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="border-b border-ink-700/70 text-left text-xs text-ink-400">
              <tr>
                <th className="px-5 py-3 font-medium">앱</th>
                <th className="px-3 py-3 font-medium">상태</th>
                <th className="px-3 py-3 font-medium">도메인</th>
                <th className="px-3 py-3 font-medium">포트</th>
                <th className="px-5 py-3 font-medium">커밋</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-ink-800">
              {apps.data.map((a) => (
                <tr key={a.id} className="hover:bg-ink-850">
                  <td className="px-5 py-3">
                    <Link to={`/apps/${a.id}`} className="flex items-center gap-2.5 font-medium hover:text-signal">
                      {a.name} <KindBadge kind={a.kind} />
                    </Link>
                    <span className="font-mono text-xs text-ink-400">{a.slug}</span>
                  </td>
                  <td className="px-3 py-3">
                    <StatusBadge status={a.status} />
                  </td>
                  <td className="max-w-56 truncate px-3 py-3 font-mono text-xs text-ink-300">{a.domains || '–'}</td>
                  <td className="px-3 py-3 font-mono text-xs text-ink-300">{a.port ?? (a.api_proxy_port ? `→${a.api_proxy_port}` : '–')}</td>
                  <td className="px-5 py-3 font-mono text-xs text-ink-400">{shortSha(a.current_commit)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <Empty
          title="앱이 없습니다"
          action={
            <button className="btn-primary" onClick={() => setCreating(true)}>
              앱 추가
            </button>
          }
        >
          Git 저장소에서 빌드하는 앱입니다. Rust(Axum) 백엔드는 <b>Rust</b>, React + Vite 프런트는 <b>Static</b>으로 만들고, 프런트의 “/api/ 프록시 포트”에
          백엔드 포트를 넣으면 한 도메인에서 프런트와 API 를 함께 서비스합니다. 파일을 직접 올리는 PHP·HTML 사이트는 <b>사이트</b> 메뉴에서 만드세요.
        </Empty>
      )}

      <Modal open={creating} onClose={() => setCreating(false)} title="새 앱" wide>
        <ErrorBox error={error} />
        <div className={error ? 'mt-4' : ''}>
          <AppForm
            submitting={busy}
            onSubmit={async (payload) => {
              setBusy(true)
              setError(null)
              try {
                const a = await post(`/projects/${id}/apps`, payload)
                toast.push(`앱 '${a.name}' 생성됨 — 포트 ${a.port ?? '없음'}`, 'success')
                nav(`/apps/${a.id}`)
              } catch (e) {
                setError(e)
              } finally {
                setBusy(false)
              }
            }}
          />
        </div>
      </Modal>
      {editing && (
        <ProjectFormModal
          open
          initial={p}
          onClose={() => setEditing(false)}
          onSaved={async (body) => {
            await put(`/projects/${id}`, body)
            setEditing(false)
            project.reload(true)
          }}
        />
      )}
    </>
  )
}
