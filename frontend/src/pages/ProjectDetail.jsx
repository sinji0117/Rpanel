import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import AppForm from '../components/AppForm'
import { Empty, ErrorBox, KindBadge, Modal, PageHeader, Spinner, StatusBadge } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { del, get, post, put } from '../lib/api'
import { shortSha } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { ProjectFormModal } from './Projects'
import { t, tx } from '../i18n'
import { confirmDialog } from '../components/Dialog'

const isBackend = (a) => a.kind === 'rust' || a.kind === 'python'
const isAdminFront = (a) => a.kind === 'static' && /admin|관리/i.test(`${a.name} ${a.slug} ${a.domains}`)

function roleOf(a) {
  if (isBackend(a)) return t('백엔드 (API)')
  if (a.kind === 'static') return isAdminFront(a) ? t('관리자 프런트') : t('프런트')
  return t('사이트')
}

function Domains({ value }) {
  const list = (value || '').split(/\s+/).filter(Boolean)
  if (!list.length) return <span className="text-ink-500">{t('도메인 없음')}</span>
  return (
    <span className="flex flex-wrap gap-x-2">
      {list.map((d) => (
        <a key={d} href={`http://${d}`} target="_blank" rel="noreferrer" className="font-mono hover:text-signal">
          {d}
        </a>
      ))}
    </span>
  )
}

/** 프런트 → /api/ → 백엔드 연결 고르기 */
function ApiLink({ app, backends, reload }) {
  const toast = useToast()
  const [busy, setBusy] = useState(false)
  const target = backends.find((b) => b.port && b.port === app.api_proxy_port)
  return (
    <label className="flex flex-wrap items-center gap-2 text-xs text-ink-400">
      {t('/api/ 연결')}
      <select
        className="input w-auto py-1 text-xs"
        value={app.api_proxy_port ?? ''}
        disabled={busy}
        aria-label={t('{name} 의 /api/ 연결', { name: app.name })}
        onChange={async (e) => {
          const port = e.target.value ? Number(e.target.value) : null
          setBusy(true)
          try {
            await put(`/apps/${app.id}`, { ...app, api_proxy_port: port })
            toast.push(t('저장했습니다 — 배포(적용)하면 반영됩니다'), 'success')
            reload()
          } catch (err) {
            toast.push(err.message, 'error')
          } finally {
            setBusy(false)
          }
        }}
      >
        <option value="">{t('연결 안 함')}</option>
        {backends.map((b) => (
          <option key={b.id} value={b.port ?? ''} disabled={!b.port}>
            {b.name} (:{b.port})
          </option>
        ))}
        {app.api_proxy_port && !target && <option value={app.api_proxy_port}>:{app.api_proxy_port}</option>}
      </select>
    </label>
  )
}

function AppCard({ a, backends, reload }) {
  const toast = useToast()
  const nav = useNavigate()
  return (
    <div className="card flex flex-col gap-3 p-4" data-testid={`project-app-${a.slug}`}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="text-xs text-ink-400">{roleOf(a)}</p>
          <Link to={`/apps/${a.id}`} className="flex items-center gap-2 font-medium hover:text-signal">
            {a.name} <KindBadge kind={a.kind} />
          </Link>
        </div>
        <StatusBadge status={a.status} />
      </div>
      <div className="space-y-1 text-xs text-ink-300">
        <Domains value={a.domains} />
        <p className="font-mono text-ink-400">
          {a.port ? `:${a.port}` : ''} {a.current_commit ? `· ${shortSha(a.current_commit)}` : `· ${t('배포 전')}`}
        </p>
      </div>
      {a.kind === 'static' && a.source === 'git' && <ApiLink app={a} backends={backends} reload={reload} />}
      <div className="mt-auto flex gap-1.5">
        <button
          className="btn-primary px-2.5 py-1 text-xs"
          disabled={a.status === 'deploying'}
          onClick={async () => {
            try {
              const { deployment_id } = await post(`/apps/${a.id}/deploy`)
              nav(`/deployments/${deployment_id}`)
            } catch (e) {
              toast.push(e.message, 'error')
            }
          }}
        >
          {t('배포')}
        </button>
        <Link to={`/apps/${a.id}`} className="btn-ghost px-2.5 py-1 text-xs">
          {t('관리')}
        </Link>
      </div>
    </div>
  )
}

function ProjectOverview({ apps, reload, onAdd, projectName = '' }) {
  const toast = useToast()
  const [busy, setBusy] = useState(false)
  const backends = apps.filter(isBackend)
  const fronts = apps.filter((a) => a.kind === 'static' && !isAdminFront(a))
  const admins = apps.filter(isAdminFront)
  const others = apps.filter((a) => !isBackend(a) && a.kind !== 'static')
  const groups = [
    { id: 'front', title: t('프런트'), list: fronts, add: { role: 'front', kind: 'static', name: '', api_proxy_port: backends[0]?.port ?? '' } },
    { id: 'back', title: t('백엔드 (API)'), list: backends, add: { role: 'back', kind: 'rust' } },
    { id: 'admin', title: t('관리자 프런트'), list: admins, add: { role: 'admin', kind: 'static', name: `${projectName} Admin`.trim(), api_proxy_port: backends[0]?.port ?? '' } },
  ]
  return (
    <div className="space-y-5">
      <div className="card flex flex-wrap items-center gap-3 p-4 text-sm">
        <p className="flex-1 text-ink-300">
          {t(
            '프런트·백엔드·관리자 화면을 한 곳에서 관리합니다. 프런트의 “/api/ 연결”에서 백엔드를 고르면 같은 도메인의 /api/ 요청이 백엔드로 갑니다. 관리자 화면은 별도 Static 앱으로 만들어 admin.example.com 같은 다른 도메인을 붙이세요.',
          )}
        </p>
        <button
          className="btn-primary"
          disabled={busy}
          data-testid="deploy-all"
          onClick={async () => {
            if (!(await confirmDialog(t('이 프로젝트의 앱 {n}개를 모두 배포합니다 (백엔드 먼저).', { n: apps.length }), { title: t('전체 배포'), confirmText: t('배포') }))) return
            setBusy(true)
            let ok = 0
            for (const a of [...backends, ...apps.filter((x) => !isBackend(x))]) {
              try {
                await post(`/apps/${a.id}/deploy`)
                ok++
              } catch (e) {
                toast.push(`${a.name}: ${e.message}`, 'error')
              }
            }
            setBusy(false)
            toast.push(t('{n}개 앱 배포를 시작했습니다', { n: ok }), 'success')
            reload()
          }}
        >
          {t('전체 배포')}
        </button>
      </div>
      {groups.map((g) => (
        <section key={g.id}>
          <div className="mb-2 flex items-center gap-2">
            <h2 className="text-sm font-medium text-ink-300">{g.title}</h2>
            <button className="btn-ghost px-2 py-0.5 text-xs" onClick={() => onAdd(g.add)}>
              + {t('추가')}
            </button>
          </div>
          {g.list.length ? (
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {g.list.map((a) => (
                <AppCard key={a.id} a={a} backends={backends} reload={reload} />
              ))}
            </div>
          ) : (
            <p className="rounded-lg border border-dashed border-ink-700 px-4 py-3 text-xs text-ink-500">{t('아직 없습니다')}</p>
          )}
        </section>
      ))}
      {others.length > 0 && (
        <section>
          <h2 className="mb-2 text-sm font-medium text-ink-300">{t('기타')}</h2>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {others.map((a) => (
              <AppCard key={a.id} a={a} backends={backends} reload={reload} />
            ))}
          </div>
        </section>
      )}
    </div>
  )
}

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
          {t('프로젝트')}
        </Link>{' '}
        / {p.name}
      </nav>
      <PageHeader
        title={p.name}
        subtitle={p.description}
        help={tx(
          'Git 저장소에서 빌드하는 앱입니다. Rust(Axum) 백엔드는 {rust}, Flask·FastAPI·Django 는 {python}, React + Vite 프런트는 {static}으로 만들고, 프런트의 “/api/ 프록시 포트”에 백엔드 포트를 넣으면 한 도메인에서 프런트와 API 를 함께 서비스합니다. 빌드한 실행 파일·HTML 을 직접 올리는 사이트는 {sites} 메뉴에서 만드세요.',
          { rust: <b>Rust</b>, python: <b>Python</b>, static: <b>Static</b>, sites: <b>{t('사이트')}</b> },
        )}
        actions={
          <>
            <button className="btn-ghost" onClick={() => setEditing(true)}>
              {t('수정')}
            </button>
            <button
              className="btn-ghost"
              onClick={async () => {
                if (!(await confirmDialog(t("프로젝트 '{name}' 를 삭제할까요?", { name: p.name })))) return
                try {
                  await del(`/projects/${id}`)
                  toast.push(t('프로젝트 삭제됨'), 'success')
                  nav('/projects')
                } catch (e) {
                  toast.push(e.message, 'error')
                }
              }}
            >
              {t('삭제')}
            </button>
            <button className="btn-primary" onClick={() => setCreating(true)}>
              {t('+ 새 앱')}
            </button>
          </>
        }
      />

      {apps.data?.length ? (
        <ProjectOverview projectName={p.name} apps={apps.data} reload={() => apps.reload(true)} onAdd={(preset) => setCreating(preset || true)} />
      ) : (
        <Empty
          title={t('앱이 없습니다')}
          action={
            <button className="btn-primary" onClick={() => setCreating(true)}>
              {t('앱 추가')}
            </button>
          }
        />
      )}

      <Modal open={!!creating} onClose={() => setCreating(false)} title={t('새 앱')} wide>
        <ErrorBox error={error} />
        <div className={error ? 'mt-4' : ''}>
          <AppForm
            key={String(creating)}
            initial={typeof creating === 'object' ? creating : undefined}
            backends={(apps.data || []).filter(isBackend)}
            submitting={busy}
            onSubmit={async (payload) => {
              setBusy(true)
              setError(null)
              try {
                const a = await post(`/projects/${id}/apps`, payload)
                toast.push(t("앱 '{name}' 생성됨 — 포트 {v}", { name: a.name, v: a.port ?? t('없음') }), 'success')
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
