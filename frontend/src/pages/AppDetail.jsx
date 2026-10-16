import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import AppForm from '../components/AppForm'
import LogViewer from '../components/LogViewer'
import { CopyButton, ErrorBox, KindBadge, PageHeader, Spinner, StatusBadge, Tabs } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { del, get, post, put } from '../lib/api'
import { ago, duration, mergeEnv, parseDotenv, shortSha } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { BackupTab, CronTab, DatabaseTab, NginxLogTab } from './AppTabs'
import { FileManager, SiteSettings, SshTab, SslTab } from './SiteTabs'

export default function AppDetail() {
  const { id } = useParams()
  const nav = useNavigate()
  const toast = useToast()
  const app = useFetch(() => get(`/apps/${id}`), [id])
  const [tabState, setTab] = useState(null)
  const [deploying, setDeploying] = useState(false)

  useInterval(() => app.reload(true), 4000, app.data?.status === 'deploying')

  if (app.loading) return <Spinner className="text-ink-400" />
  if (app.error) return <ErrorBox error={app.error} />
  const a = app.data
  const isService = a.kind === 'rust' || a.kind === 'python'
  const isGit = a.source === 'git'
  const isFiles = a.source === 'files'
  const tab = tabState ?? (isGit ? 'deployments' : isFiles ? 'files' : 'settings')

  const deploy = async () => {
    setDeploying(true)
    try {
      const { deployment_id } = await post(`/apps/${id}/deploy`)
      nav(`/deployments/${deployment_id}`)
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setDeploying(false)
    }
  }

  const tabs = isGit
    ? [
        { id: 'deployments', label: '배포' },
        { id: 'env', label: '환경변수' },
        { id: 'db', label: '데이터베이스' },
        { id: 'backups', label: '백업' },
        ...(isService ? [{ id: 'cron', label: '크론' }, { id: 'logs', label: '서비스 로그' }] : []),
        ...(a.domains ? [{ id: 'ssl', label: 'SSL' }, { id: 'nginx', label: 'Nginx 로그' }] : []),
        { id: 'git', label: 'Git · 웹훅' },
        { id: 'settings', label: '설정' },
      ]
    : isFiles
      ? [
          { id: 'files', label: '파일' },
          { id: 'ssl', label: 'SSL' },
          { id: 'ssh', label: 'SSH · SFTP' },
          { id: 'db', label: '데이터베이스' },
          { id: 'backups', label: '백업' },
          { id: 'cron', label: '크론' },
          ...(a.kind !== 'static' ? [{ id: 'env', label: '환경변수' }] : []),
          ...(isService ? [{ id: 'logs', label: '서비스 로그' }] : []),
          { id: 'nginx', label: 'Nginx 로그' },
          { id: 'deployments', label: '적용 기록' },
          { id: 'settings', label: '설정' },
        ]
      : [
          { id: 'settings', label: '설정' },
          { id: 'ssl', label: 'SSL' },
          { id: 'nginx', label: 'Nginx 로그' },
          { id: 'deployments', label: '적용 기록' },
        ]

  return (
    <>
      <nav className="mb-3 text-xs text-ink-400">
        {a.project_id ? (
          <>
            <Link to="/projects" className="hover:text-ink-100">
              프로젝트
            </Link>{' '}
            /{' '}
            <Link to={`/projects/${a.project_id}`} className="hover:text-ink-100">
              #{a.project_id}
            </Link>
          </>
        ) : (
          <Link to="/sites" className="hover:text-ink-100">
            사이트
          </Link>
        )}{' '}
        / {a.slug}
      </nav>
      <PageHeader
        title={
          <span className="flex items-center gap-3">
            {a.name} <KindBadge kind={a.kind} source={a.source} /> <StatusBadge status={a.status} />
          </span>
        }
        actions={
          <>
            {isService && <ServiceButtons app={a} onDone={() => app.reload(true)} />}
            <button className="btn-primary" onClick={deploy} disabled={deploying || a.status === 'deploying'} title={isGit ? '' : '설정(Nginx·서비스)을 다시 적용합니다'}>
              {a.status === 'deploying' ? (isGit ? '배포 중…' : '적용 중…') : isGit ? '배포' : '적용'}
            </button>
          </>
        }
      >
        <dl className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-xs text-ink-400">
          {isGit && (
            <>
              <div>
                <dt className="inline">브랜치 </dt>
                <dd className="inline font-mono text-ink-300">{a.git_branch}</dd>
              </div>
              <div>
                <dt className="inline">커밋 </dt>
                <dd className="inline font-mono text-ink-300">{shortSha(a.current_commit)}</dd>
              </div>
            </>
          )}
          {a.site_user && (
            <div>
              <dt className="inline">사이트 유저 </dt>
              <dd className="inline font-mono text-ink-300">{a.site_user}</dd>
            </div>
          )}
          {a.proxy_target && (
            <div>
              <dt className="inline">→ </dt>
              <dd className="inline font-mono text-ink-300">{a.proxy_target}</dd>
            </div>
          )}
          {a.port && (
            <div>
              <dt className="inline">포트 </dt>
              <dd className="inline font-mono text-ink-300">127.0.0.1:{a.port}</dd>
            </div>
          )}
          {a.domains &&
            a.domains.split(' ').map((d) => (
              <a key={d} href={`${a.ssl_mode !== 'none' ? 'https' : 'http'}://${d}`} target="_blank" rel="noreferrer" className="font-mono text-signal hover:underline">
                {d} ↗
              </a>
            ))}
        </dl>
      </PageHeader>

      <Tabs tabs={tabs} active={tab} onChange={setTab} />
      {tab === 'deployments' && <DeploymentsTab app={a} onChanged={() => app.reload(true)} />}
      {tab === 'env' && <EnvTab app={a} />}
      {tab === 'db' && <DatabaseTab app={a} onChanged={() => app.reload(true)} />}
      {tab === 'backups' && <BackupTab app={a} onChanged={() => app.reload(true)} />}
      {tab === 'cron' && <CronTab app={a} />}
      {tab === 'nginx' && <NginxLogTab app={a} />}
      {tab === 'logs' && <JournalTab app={a} />}
      {tab === 'git' && <GitTab app={a} />}
      {tab === 'files' && <FileManager app={a} />}
      {tab === 'ssl' && <SslTab app={a} onChanged={() => app.reload(true)} />}
      {tab === 'ssh' && <SshTab app={a} />}
      {tab === 'settings' && (isGit ? <SettingsTab app={a} onSaved={(n) => app.setData(n)} /> : <SiteSettings app={a} onSaved={(n) => app.setData(n)} />)}
    </>
  )
}

function ServiceButtons({ app, onDone }) {
  const toast = useToast()
  const status = useFetch(() => get(`/apps/${app.id}/service`), [app.id, app.status])
  const [busy, setBusy] = useState(null)
  const act = async (action) => {
    setBusy(action)
    try {
      await post(`/apps/${app.id}/service`, { action })
      toast.push(`${app.slug}: ${action} 완료`, 'success')
      status.reload(true)
      onDone()
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setBusy(null)
    }
  }
  const disabled = !app.current_release || !!busy
  return (
    <div className="flex items-center gap-1.5">
      {status.data?.active && <span className="mr-1 font-mono text-xs text-ink-400">systemd: {status.data.active}</span>}
      <button className="btn-ghost px-2.5" disabled={disabled} onClick={() => act('restart')} title="재시작">
        {busy === 'restart' ? <Spinner /> : '↻'}
      </button>
      {app.status === 'stopped' ? (
        <button className="btn-ghost px-2.5" disabled={disabled} onClick={() => act('start')} title="시작">
          ▶
        </button>
      ) : (
        <button className="btn-ghost px-2.5" disabled={disabled} onClick={() => act('stop')} title="중지">
          ■
        </button>
      )}
    </div>
  )
}

function DeploymentsTab({ app, onChanged }) {
  const toast = useToast()
  const list = useFetch(() => get(`/apps/${app.id}/deployments?limit=50`), [app.id])
  useInterval(() => list.reload(true), 3000, !!list.data?.some((d) => ['queued', 'running'].includes(d.status)) || app.status === 'deploying')
  useEffect(() => {
    list.reload(true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [app.status])

  if (list.loading) return <Spinner className="text-ink-400" />
  if (!list.data?.length) return <p className="card px-5 py-10 text-center text-sm text-ink-400">아직 배포가 없습니다. 오른쪽 위 “배포”를 누르세요.</p>

  return (
    <div className="card overflow-x-auto">
      <table className="w-full text-sm">
        <thead className="border-b border-ink-700/70 text-left text-xs text-ink-400">
          <tr>
            <th className="px-5 py-3 font-medium">#</th>
            <th className="px-3 py-3 font-medium">상태</th>
            <th className="px-3 py-3 font-medium">커밋</th>
            <th className="px-3 py-3 font-medium">소요</th>
            <th className="px-3 py-3 font-medium">시작</th>
            <th className="px-5 py-3" />
          </tr>
        </thead>
        <tbody className="divide-y divide-ink-800">
          {list.data.map((d) => (
            <tr key={d.id} className="hover:bg-ink-850">
              <td className="px-5 py-3">
                <Link to={`/deployments/${d.id}`} className="font-mono hover:text-signal">
                  #{d.id}
                </Link>
                {app.current_release === d.id && <span className="ml-2 text-[11px] text-signal">현재</span>}
              </td>
              <td className="px-3 py-3">
                <StatusBadge status={d.status} deployment />
              </td>
              <td className="px-3 py-3 font-mono text-xs text-ink-300">{shortSha(d.commit_sha)}</td>
              <td className="px-3 py-3 text-xs text-ink-400">{duration(d.started_at, d.finished_at)}</td>
              <td className="px-3 py-3 text-xs text-ink-400">{ago(d.created_at)}</td>
              <td className="px-5 py-3 text-right">
                {d.status === 'success' && app.current_release !== d.id && (
                  <button
                    className="btn-ghost px-2.5 py-1 text-xs"
                    onClick={async () => {
                      if (!window.confirm(`#${d.id} 릴리스로 되돌릴까요?`)) return
                      try {
                        await post(`/deployments/${d.id}/rollback`)
                        toast.push(`#${d.id} 로 롤백했습니다`, 'success')
                        onChanged()
                        list.reload(true)
                      } catch (e) {
                        toast.push(e.message, 'error')
                      }
                    }}
                  >
                    롤백
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function EnvTab({ app }) {
  const toast = useToast()
  const env = useFetch(() => get(`/apps/${app.id}/env`), [app.id])
  const [rows, setRows] = useState(null)
  const [reveal, setReveal] = useState(false)
  const managed = env.data?.managed ?? []
  const [bulk, setBulk] = useState(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState(null)

  useEffect(() => {
    if (env.data) setRows(env.data.vars)
  }, [env.data])

  if (!rows) return <Spinner className="text-ink-400" />
  const update = (i, k, v) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, [k]: v } : r)))

  return (
    <div className="card p-5">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-ink-400">
          {app.kind === 'static' ? '빌드 시점에 주입됩니다 (Vite는 VITE_ 로 시작하는 변수만 번들에 포함).' : '빌드와 서비스 실행 모두에 주입됩니다.'} 저장 후{' '}
          <b className="text-ink-100">재배포</b>해야 반영됩니다.
          {app.port && (
            <>
              {' '}
              <span className="kbd">PORT</span> <span className="kbd">BIND_ADDR</span>는 자동입니다.
            </>
          )}
        </p>
        <div className="flex gap-2">
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setReveal((r) => !r)}>
            {reveal ? '값 숨기기' : '값 보기'}
          </button>
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setBulk('')}>
            .env 붙여넣기
          </button>
        </div>
      </div>

      {managed.length > 0 && (
        <div className="mb-4 rounded-lg border border-ink-700 bg-ink-950/60 p-3">
          <p className="mb-2 text-xs text-ink-400">패널이 자동 주입 (데이터베이스 탭에서 관리) — 같은 키를 아래에 직접 넣으면 직접 넣은 값이 우선합니다</p>
          <ul className="space-y-1">
            {managed.map((m) => {
              const overridden = rows.some((r) => r.key === m.key)
              return (
                <li key={m.key} className={`flex items-center gap-3 font-mono text-xs ${overridden ? 'text-ink-600 line-through' : ''}`}>
                  <span className="w-2/5 truncate">{m.key}</span>
                  <span className="truncate text-ink-400">{reveal ? m.value : m.value.replace(/:\/\/([^:]*):[^@]+@/, '://$1:••••••@')}</span>
                  <span className="ml-auto shrink-0 font-sans text-[11px] text-ink-400">{m.source}</span>
                </li>
              )
            })}
          </ul>
        </div>
      )}

      {bulk !== null && (
        <div className="mb-4 space-y-2">
          <textarea
            aria-label=".env 내용"
            className="input min-h-32 font-mono text-xs"
            placeholder={'DATABASE_URL=mysql://app_user:...@127.0.0.1:3306/app_db\nREDIS_URL=redis://:...@127.0.0.1:6379/1'}
            value={bulk}
            onChange={(e) => setBulk(e.target.value)}
          />
          <div className="flex justify-end gap-2">
            <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setBulk(null)}>
              취소
            </button>
            <button
              className="btn-primary px-2.5 py-1 text-xs"
              onClick={() => {
                const parsed = parseDotenv(bulk)
                setRows((rs) => mergeEnv(rs.filter((r) => r.key), parsed))
                setBulk(null)
                toast.push(`${parsed.length}개 변수를 가져왔습니다 (아직 저장 전)`)
              }}
            >
              가져오기
            </button>
          </div>
        </div>
      )}

      <div className="space-y-2">
        {rows.map((r, i) => (
          <div key={i} className="flex gap-2">
            <input
              aria-label={`키 ${i + 1}`}
              className="input w-2/5 font-mono text-xs"
              value={r.key}
              onChange={(e) => update(i, 'key', e.target.value.replace(/[^A-Za-z0-9_]/g, '_'))}
              placeholder="KEY"
            />
            <input
              aria-label={`값 ${i + 1}`}
              className="input font-mono text-xs"
              type={reveal ? 'text' : 'password'}
              autoComplete="off"
              value={r.value}
              onChange={(e) => update(i, 'value', e.target.value)}
              placeholder="value"
            />
            <button className="btn-ghost px-2.5" aria-label={`${r.key || '변수'} 삭제`} onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}>
              ✕
            </button>
          </div>
        ))}
      </div>
      <div className="mt-4 flex items-center justify-between">
        <button className="btn-ghost text-xs" onClick={() => setRows((rs) => [...rs, { key: '', value: '' }])}>
          + 변수 추가
        </button>
        <button
          className="btn-primary"
          disabled={saving}
          onClick={async () => {
            setSaving(true)
            setError(null)
            try {
              const res = await put(`/apps/${app.id}/env`, { vars: rows.filter((r) => r.key.trim()) })
              setRows(res.vars)
              toast.push('환경변수 저장됨 — 재배포하면 적용됩니다', 'success')
            } catch (e) {
              setError(e)
            } finally {
              setSaving(false)
            }
          }}
        >
          {saving ? '저장 중…' : '저장'}
        </button>
      </div>
      {error && (
        <div className="mt-3">
          <ErrorBox error={error} />
        </div>
      )}
    </div>
  )
}

function JournalTab({ app }) {
  const [n, setN] = useState(200)
  const j = useFetch(() => get(`/apps/${app.id}/journal?lines=${n}`), [app.id, n])
  const [follow, setFollow] = useState(false)
  useInterval(() => j.reload(true), 3000, follow)
  const lines = (j.data?.lines ?? []).map((text, i) => ({ seq: i + 1, text }))
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
        <span className="font-mono text-xs text-ink-400">journalctl -u rpanel-app-{app.slug}.service -n {n}</span>
        <div className="flex items-center gap-2">
          <select className="input w-auto py-1 text-xs" value={n} onChange={(e) => setN(Number(e.target.value))} aria-label="줄 수">
            {[100, 200, 500, 1000].map((x) => (
              <option key={x} value={x}>
                {x}줄
              </option>
            ))}
          </select>
          <label className="flex items-center gap-1.5 text-xs text-ink-300">
            <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} /> 3초마다 갱신
          </label>
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => j.reload()}>
            새로고침
          </button>
        </div>
      </div>
      <ErrorBox error={j.error} />
      <LogViewer lines={lines} emptyText={j.loading ? '불러오는 중…' : '로그가 없습니다 (아직 배포 전이거나 서비스가 기록하지 않음)'} />
    </div>
  )
}

function GitTab({ app }) {
  const toast = useToast()
  const key = useFetch(() => get(`/apps/${app.id}/deploy-key`), [app.id])
  const hook = useFetch(() => get(`/apps/${app.id}/webhook`), [app.id])
  const [busy, setBusy] = useState(false)
  const hookUrl = hook.data ? `${window.location.origin}${hook.data.path}` : ''

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <section className="card p-5">
        <h3 className="font-medium">Deploy key (SSH)</h3>
        <p className="mt-1 text-sm text-ink-400">
          이 앱 전용 읽기 키입니다. GitLab → 프로젝트 → Settings → Repository → <b>Deploy keys</b>에 공개키를 등록하세요 (쓰기 권한 체크 X). 프로젝트마다 따로
          등록해야 합니다.
        </p>
        {key.data?.public_key ? (
          <div className="mt-4 space-y-2">
            <pre className="overflow-x-auto rounded-lg border border-ink-700 bg-ink-950 p-3 font-mono text-[11px] break-all whitespace-pre-wrap text-ink-300">
              {key.data.public_key}
            </pre>
            <div className="flex gap-2">
              <CopyButton text={key.data.public_key} />
            </div>
          </div>
        ) : (
          <p className="mt-4 text-sm text-ink-400">아직 키가 없습니다. public 저장소라면 필요 없습니다.</p>
        )}
        <button
          className="btn-ghost mt-4 text-xs"
          disabled={busy}
          onClick={async () => {
            if (key.data?.public_key && !window.confirm('키를 새로 만들면 GitLab에 다시 등록해야 합니다. 계속할까요?')) return
            setBusy(true)
            try {
              const r = await post(`/apps/${app.id}/deploy-key`)
              key.setData(r)
              toast.push('Deploy key 생성됨', 'success')
            } catch (e) {
              toast.push(e.message, 'error')
            } finally {
              setBusy(false)
            }
          }}
        >
          {key.data?.public_key ? '키 다시 만들기' : 'Deploy key 만들기'}
        </button>
        <p className="mt-4 text-xs text-ink-400">
          Git URL 예: <span className="font-mono">ssh://git@gitlab.example.com:2222/그룹/저장소.git</span> — 이 서버에서 접속할 수 있는 주소를 쓰세요.
        </p>
      </section>

      <section className="card p-5">
        <h3 className="font-medium">Webhook — push 시 자동 배포</h3>
        <p className="mt-1 text-sm text-ink-400">
          GitLab → Settings → Webhooks → URL과 Secret token 입력, <b>Push events</b> 체크 (브랜치 필터: <span className="font-mono">{hook.data?.branch}</span>).
        </p>
        {hook.data && (
          <div className="mt-4 space-y-3">
            <div>
              <span className="label">URL</span>
              <div className="flex gap-2">
                <input readOnly aria-label="Webhook URL" className="input font-mono text-xs" value={hookUrl} />
                <CopyButton text={hookUrl} />
              </div>
            </div>
            <div>
              <span className="label">Secret token (X-Gitlab-Token)</span>
              <div className="flex gap-2">
                <input readOnly aria-label="Webhook token" type="password" className="input font-mono text-xs" value={hook.data.token} />
                <CopyButton text={hook.data.token} />
              </div>
            </div>
            <button
              className="btn-ghost text-xs"
              onClick={async () => {
                if (!window.confirm('토큰을 바꾸면 GitLab 설정도 바꿔야 합니다. 계속할까요?')) return
                try {
                  hook.setData(await post(`/apps/${app.id}/webhook`))
                  toast.push('토큰을 교체했습니다', 'success')
                } catch (e) {
                  toast.push(e.message, 'error')
                }
              }}
            >
              토큰 교체
            </button>
          </div>
        )}
        <p className="mt-4 text-xs text-ink-400">
          Git 서버가 이 패널 주소(8443)에 접속할 수 있어야 합니다. 닿지 않으면 CI 에서{' '}
          <span className="font-mono">curl -X POST -H "X-Gitlab-Token: …"</span> 로 호출하세요.
        </p>
      </section>
    </div>
  )
}

function SettingsTab({ app, onSaved }) {
  const toast = useToast()
  const nav = useNavigate()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  return (
    <div className="space-y-6">
      <div className="card p-5">
        <ErrorBox error={error} />
        <div className={error ? 'mt-4' : ''}>
          <AppForm
            mode="edit"
            initial={app}
            submitting={busy}
            onSubmit={async (payload) => {
              setBusy(true)
              setError(null)
              try {
                const n = await put(`/apps/${app.id}`, payload)
                onSaved(n)
                toast.push('저장됨 — 다음 배포부터 적용됩니다', 'success')
              } catch (e) {
                setError(e)
              } finally {
                setBusy(false)
              }
            }}
          />
        </div>
      </div>
      <div className="card border-fail/30 p-5">
        <h3 className="font-medium text-fail">앱 삭제</h3>
        <p className="mt-1 text-sm text-ink-400">
          systemd 서비스(<span className="font-mono">rpanel-app-{app.slug}</span>), Nginx 사이트, 빌드·릴리스 파일, 배포 기록을 모두 지웁니다. 되돌릴 수 없습니다.
        </p>
        <button
          className="btn-danger mt-4"
          onClick={async () => {
            const typed = window.prompt(`삭제하려면 slug를 입력하세요: ${app.slug}`)
            if (typed !== app.slug) return
            try {
              await del(`/apps/${app.id}`)
              toast.push(`${app.slug} 삭제됨`, 'success')
              nav(`/projects/${app.project_id}`)
            } catch (e) {
              toast.push(e.message, 'error')
            }
          }}
        >
          앱 삭제
        </button>
      </div>
    </div>
  )
}
