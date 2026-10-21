import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router'
import AppForm, { sshUrl, toPayload } from '../components/AppForm'
import DeploymentLogPanel from '../components/DeploymentLog'
import LogViewer from '../components/LogViewer'
import { CardHeader, CopyButton, ErrorBox, HelpTip, KindBadge, PageHeader, Spinner, StatusBadge, Tabs } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { del, get, post, put } from '../lib/api'
import { ago, duration, mergeEnv, parseDotenv, shortSha } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { BackupTab, CronTab, DatabaseTab, NginxLogTab } from './AppTabs'
import { AccountsTab } from './SiteAccounts'
import { AppRuntimePicker } from './Runtimes'
import { FileManager, SiteSettings, SslTab } from './SiteTabs'
import { t, tx } from '../i18n'
import { confirmDialog } from '../components/Dialog'
import { isServiceKind } from '../lib/kinds'

export default function AppDetail() {
  const { id } = useParams()
  const nav = useNavigate()
  const toast = useToast()
  const app = useFetch(() => get(`/apps/${id}`), [id])
  const [params] = useSearchParams()
  const [tabState, setTab] = useState(() => params.get('tab'))
  const [deploying, setDeploying] = useState(false)
  // 배포 탭에서 로그를 볼 배포 (null = 가장 최근)
  const [logId, setLogId] = useState(null)

  useInterval(() => app.reload(true), 4000, app.data?.status === 'deploying')

  if (app.loading) return <Spinner className="text-ink-400" />
  if (app.error) return <ErrorBox error={app.error} />
  const a = app.data
  const isService = isServiceKind(a.kind)
  const isGit = a.source === 'git'
  const isFiles = a.source === 'files'
  const tab = tabState ?? (isGit ? 'deployments' : isFiles ? 'files' : 'settings')

  const deploy = async () => {
    setDeploying(true)
    try {
      const { deployment_id } = await post(`/apps/${id}/deploy`)
      // 화면을 떠나지 않고 배포 탭에서 실시간 로그를 보여 줌
      setLogId(deployment_id)
      setTab('deployments')
      app.reload(true)
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setDeploying(false)
    }
  }

  const tabs = isGit
    ? [
        { id: 'deployments', label: t('배포') },
        { id: 'env', label: t('환경변수') },
        { id: 'db', label: t('데이터베이스') },
        { id: 'backups', label: t('백업') },
        ...(isService
          ? [
              { id: 'cron', label: t('크론') },
              { id: 'logs', label: t('서비스 로그') },
            ]
          : []),
        ...(a.domains
          ? [
              { id: 'ssl', label: 'SSL' },
              { id: 'nginx', label: t('Nginx 로그') },
            ]
          : []),
        { id: 'git', label: t('Git · 웹훅') },
        { id: 'settings', label: t('설정') },
      ]
    : isFiles
      ? [
          { id: 'files', label: t('파일') },
          { id: 'ssl', label: 'SSL' },
          { id: 'accounts', label: t('사이트 계정') },
          { id: 'db', label: t('데이터베이스') },
          { id: 'backups', label: t('백업') },
          { id: 'cron', label: t('크론') },
          ...(a.kind !== 'static' ? [{ id: 'env', label: t('환경변수') }] : []),
          ...(isService ? [{ id: 'logs', label: t('서비스 로그') }] : []),
          { id: 'nginx', label: t('Nginx 로그') },
          { id: 'deployments', label: t('적용 기록') },
          { id: 'settings', label: t('설정') },
        ]
      : [
          { id: 'settings', label: t('설정') },
          { id: 'ssl', label: 'SSL' },
          { id: 'nginx', label: t('Nginx 로그') },
          { id: 'deployments', label: t('적용 기록') },
        ]

  return (
    <>
      <nav className="mb-3 text-xs text-ink-400">
        {a.project_id ? (
          <>
            <Link to="/projects" className="hover:text-ink-100">
              {t('프로젝트')}
            </Link>{' '}
            /{' '}
            <Link to={`/projects/${a.project_id}`} className="hover:text-ink-100">
              #{a.project_id}
            </Link>
          </>
        ) : (
          <Link to="/sites" className="hover:text-ink-100">
            {t('사이트')}
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
            <button
              className="btn-ghost"
              onClick={() => {
                setLogId(null)
                setTab('deployments')
              }}
            >
              {isGit ? t('배포 로그') : t('적용 로그')}
            </button>
            <button className="btn-primary" onClick={deploy} disabled={deploying || a.status === 'deploying'} title={isGit ? '' : t('설정(Nginx·서비스)을 다시 적용합니다')}>
              {a.status === 'deploying' ? (isGit ? t('배포 중…') : t('적용 중…')) : isGit ? t('배포') : t('적용')}
            </button>
          </>
        }
      >
        <dl className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-xs text-ink-400">
          {isGit && (
            <>
              <div>
                <dt className="inline">{t('브랜치')} </dt>
                <dd className="inline font-mono text-ink-300">{a.git_branch}</dd>
              </div>
              <div>
                <dt className="inline">{t('커밋')} </dt>
                <dd className="inline font-mono text-ink-300">{shortSha(a.current_commit)}</dd>
              </div>
            </>
          )}
          {a.site_user && (
            <div>
              <dt className="inline">{t('사이트 유저')} </dt>
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
              <dt className="inline">{t('포트')} </dt>
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
      {tab === 'deployments' && <DeploymentsTab app={a} selected={logId} onSelect={setLogId} onChanged={() => app.reload(true)} />}
      {tab === 'env' && <EnvTab app={a} />}
      {tab === 'db' && <DatabaseTab app={a} onChanged={() => app.reload(true)} />}
      {tab === 'backups' && <BackupTab app={a} onChanged={() => app.reload(true)} />}
      {tab === 'cron' && <CronTab app={a} />}
      {tab === 'nginx' && <NginxLogTab app={a} />}
      {tab === 'logs' && <JournalTab app={a} />}
      {tab === 'git' && <GitTab app={a} onSaved={(n) => app.setData(n)} />}
      {tab === 'files' && <FileManager app={a} />}
      {tab === 'ssl' && <SslTab app={a} onChanged={() => app.reload(true)} />}
      {tab === 'accounts' && <AccountsTab app={a} />}
      {tab === 'settings' && (
        <div className="space-y-4">
          <AppRuntimePicker app={a} onSaved={(n) => app.setData(n)} />
          {isGit ? <SettingsTab app={a} onSaved={(n) => app.setData(n)} /> : <SiteSettings app={a} onSaved={(n) => app.setData(n)} />}
        </div>
      )}
    </>
  )
}

// systemctl is-active 결과 → 표시
const SYSTEMD = {
  active: { label: '실행 중', cls: 'text-signal', dot: 'bg-signal' },
  activating: { label: '시작하는 중', cls: 'text-warn', dot: 'bg-warn animate-pulse' },
  reloading: { label: '다시 읽는 중', cls: 'text-warn', dot: 'bg-warn animate-pulse' },
  deactivating: { label: '멈추는 중', cls: 'text-warn', dot: 'bg-warn animate-pulse' },
  inactive: { label: '멈춤', cls: 'text-ink-400', dot: 'bg-ink-600' },
  failed: { label: '실패', cls: 'text-fail', dot: 'bg-fail' },
}

function ServiceButtons({ app, onDone }) {
  const toast = useToast()
  const status = useFetch(() => get(`/apps/${app.id}/service`), [app.id, app.status])
  const [busy, setBusy] = useState(null)
  const state = status.data?.active
  const settling = ['activating', 'deactivating', 'reloading'].includes(state)
  // 평소 5초마다, 상태가 바뀌는 중이면 1초마다 다시 확인
  useInterval(() => status.reload(true), settling ? 1000 : 5000, !!app.current_release)
  const act = async (action) => {
    setBusy(action)
    try {
      await post(`/apps/${app.id}/service`, { action })
      toast.push(t('{slug}: {action} 완료', { slug: app.slug, action }), 'success')
      status.reload(true)
      // systemd 가 상태를 바꾸는 데 시간이 조금 걸릴 수 있어 몇 번 더 확인
      for (const ms of [800, 2000, 4000]) setTimeout(() => status.reload(true), ms)
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
      {state && (
        <span
          className={`mr-1 inline-flex items-center gap-1.5 text-xs ${SYSTEMD[state]?.cls ?? 'text-ink-400'}`}
          data-testid="systemd-state"
          title={`systemctl is-active: ${state}`}
        >
          <span className={`h-1.5 w-1.5 rounded-full ${SYSTEMD[state]?.dot ?? 'bg-ink-600'}`} />
          <span className="font-mono text-ink-400">systemd</span> {SYSTEMD[state] ? t(SYSTEMD[state].label) : state}
        </span>
      )}
      <button className="btn-ghost px-2.5" disabled={disabled} onClick={() => act('restart')} title={t('재시작')}>
        {busy === 'restart' ? <Spinner /> : '↻'}
      </button>
      {app.status === 'stopped' || state === 'inactive' || state === 'failed' ? (
        <button className="btn-ghost px-2.5" disabled={disabled} onClick={() => act('start')} title={t('시작')}>
          ▶
        </button>
      ) : (
        <button className="btn-ghost px-2.5" disabled={disabled} onClick={() => act('stop')} title={t('중지')}>
          ■
        </button>
      )}
    </div>
  )
}

function DeploymentsTab({ app, selected, onSelect, onChanged }) {
  const toast = useToast()
  const list = useFetch(() => get(`/apps/${app.id}/deployments?limit=50`), [app.id])
  useInterval(() => list.reload(true), 3000, !!list.data?.some((d) => ['queued', 'running'].includes(d.status)) || app.status === 'deploying')
  useEffect(() => {
    list.reload(true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [app.status, selected])

  if (list.loading) return <Spinner className="text-ink-400" />
  if (!list.data?.length) return <p className="card px-5 py-10 text-center text-sm text-ink-400">{t('아직 배포가 없습니다. 오른쪽 위 “배포”를 누르세요.')}</p>
  const shown = selected && list.data.some((d) => d.id === selected) ? selected : list.data[0].id

  return (
    <div className="space-y-4">
      <div className="card max-h-[320px] overflow-auto">
        <table className="w-full text-sm" aria-label={t('배포 기록')}>
          <thead className="border-b border-ink-700/70 text-left text-xs text-ink-400">
            <tr>
              <th className="px-5 py-3 font-medium">#</th>
              <th className="px-3 py-3 font-medium">{t('상태')}</th>
              <th className="px-3 py-3 font-medium">{t('커밋')}</th>
              <th className="px-3 py-3 font-medium">{t('소요')}</th>
              <th className="px-3 py-3 font-medium">{t('시작')}</th>
              <th className="px-5 py-3" />
            </tr>
          </thead>
          <tbody className="divide-y divide-ink-800">
            {list.data.map((d) => (
              <tr key={d.id} className={`cursor-pointer hover:bg-ink-850 ${d.id === shown ? 'bg-ink-850' : ''}`} aria-selected={d.id === shown} onClick={() => onSelect?.(d.id)}>
                <td className="px-5 py-3">
                  <span className={`font-mono ${d.id === shown ? 'text-signal' : ''}`}>#{d.id}</span>
                  {app.current_release === d.id && <span className="ml-2 text-[11px] text-signal">{t('현재')}</span>}
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
                      onClick={async (e) => {
                        e.stopPropagation()
                        if (!(await confirmDialog(t('#{id} 릴리스로 되돌릴까요?', { id: d.id })))) return
                        try {
                          await post(`/deployments/${d.id}/rollback`)
                          toast.push(t('#{id} 로 롤백했습니다', { id: d.id }), 'success')
                          onChanged()
                          list.reload(true)
                        } catch (e) {
                          toast.push(e.message, 'error')
                        }
                      }}
                    >
                      {t('롤백')}
                    </button>
                  )}
                  <button className="btn-ghost ml-1.5 px-2.5 py-1 text-xs" aria-label={t('#{id} 로그 보기', { id: d.id })} onClick={(e) => (e.stopPropagation(), onSelect?.(d.id))}>
                    {t('로그')}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <DeploymentLogPanel key={shown} id={shown} onFinished={() => (list.reload(true), onChanged())} />
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
        <h3 className="font-medium">{t('환경변수')}</h3>
        <div className="flex items-center gap-2">
          <HelpTip label={t('환경변수 도움말')}>
            {app.kind === 'static' ? t('빌드 시점에 주입됩니다 (Vite는 VITE_ 로 시작하는 변수만 번들에 포함).') : t('빌드와 서비스 실행 모두에 주입됩니다.')}{' '}
            {tx('저장 후 {redeploy}해야 반영됩니다.', { redeploy: <b className="text-ink-100">{t('재배포')}</b> })}
            {app.port && <> {tx('{port} {bind} 는 자동입니다.', { port: <span className="kbd">PORT</span>, bind: <span className="kbd">BIND_ADDR</span> })}</>}
          </HelpTip>
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setReveal((r) => !r)}>
            {reveal ? t('값 숨기기') : t('값 보기')}
          </button>
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setBulk('')}>
            {t('.env 붙여넣기')}
          </button>
        </div>
      </div>

      {managed.length > 0 && (
        <div className="mb-4 rounded-lg border border-ink-700 bg-ink-950/60 p-3">
          <p className="mb-2 text-xs text-ink-400">{t('패널이 자동 주입 (데이터베이스 탭에서 관리) — 같은 키를 아래에 직접 넣으면 직접 넣은 값이 우선합니다')}</p>
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
            aria-label={t('.env 내용')}
            className="input min-h-32 font-mono text-xs"
            placeholder={'DATABASE_URL=mysql://app_user:...@127.0.0.1:3306/app_db\nREDIS_URL=redis://:...@127.0.0.1:6379/1'}
            value={bulk}
            onChange={(e) => setBulk(e.target.value)}
          />
          <div className="flex justify-end gap-2">
            <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setBulk(null)}>
              {t('취소')}
            </button>
            <button
              className="btn-primary px-2.5 py-1 text-xs"
              onClick={() => {
                const parsed = parseDotenv(bulk)
                setRows((rs) =>
                  mergeEnv(
                    rs.filter((r) => r.key),
                    parsed,
                  ),
                )
                setBulk(null)
                toast.push(t('{length}개 변수를 가져왔습니다 (아직 저장 전)', { length: parsed.length }))
              }}
            >
              {t('가져오기')}
            </button>
          </div>
        </div>
      )}

      <div className="space-y-2">
        {rows.map((r, i) => (
          <div key={i} className="flex gap-2">
            <input
              aria-label={t('키 {v}', { v: i + 1 })}
              className="input w-2/5 font-mono text-xs"
              value={r.key}
              onChange={(e) => update(i, 'key', e.target.value.replace(/[^A-Za-z0-9_]/g, '_'))}
              placeholder="KEY"
            />
            <input
              aria-label={t('값 {v}', { v: i + 1 })}
              className="input font-mono text-xs"
              type={reveal ? 'text' : 'password'}
              autoComplete="off"
              value={r.value}
              onChange={(e) => update(i, 'value', e.target.value)}
              placeholder="value"
            />
            <button className="btn-ghost px-2.5" aria-label={t('{v} 삭제', { v: r.key || t('변수') })} onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}>
              ✕
            </button>
          </div>
        ))}
      </div>
      <div className="mt-4 flex items-center justify-between">
        <button className="btn-ghost text-xs" onClick={() => setRows((rs) => [...rs, { key: '', value: '' }])}>
          {t('+ 변수 추가')}
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
              toast.push(t('환경변수 저장됨 — 재배포하면 적용됩니다'), 'success')
            } catch (e) {
              setError(e)
            } finally {
              setSaving(false)
            }
          }}
        >
          {saving ? t('저장 중…') : t('저장')}
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

export function JournalTab({ app }) {
  const [n, setN] = useState(200)
  const j = useFetch(() => get(`/apps/${app.id}/journal?lines=${n}`), [app.id, n])
  const [follow, setFollow] = useState(false)
  useInterval(() => j.reload(true), 3000, follow)
  const lines = (j.data?.lines ?? []).map((text, i) => ({ seq: i + 1, text }))
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
        <span className="font-mono text-xs text-ink-400">
          journalctl -u rpanel-app-{app.slug}.service -n {n}
        </span>
        <div className="flex items-center gap-2">
          <select className="input w-auto py-1 text-xs" value={n} onChange={(e) => setN(Number(e.target.value))} aria-label={t('줄 수')}>
            {[100, 200, 500, 1000].map((x) => (
              <option key={x} value={x}>
                {t('{x}줄', { x })}
              </option>
            ))}
          </select>
          <label className="flex items-center gap-1.5 text-xs text-ink-300">
            <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} /> {t('3초마다 갱신')}
          </label>
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => j.reload()}>
            {t('새로고침')}
          </button>
        </div>
      </div>
      <ErrorBox error={j.error} />
      <LogViewer lines={lines} emptyText={j.loading ? t('불러오는 중…') : t('로그가 없습니다 (아직 배포 전이거나 서비스가 기록하지 않음)')} />
    </div>
  )
}

function GitTab({ app, onSaved }) {
  const toast = useToast()
  const key = useFetch(() => get(`/apps/${app.id}/deploy-key`), [app.id])
  const hook = useFetch(() => get(`/apps/${app.id}/webhook`), [app.id])
  const [busy, setBusy] = useState(false)
  const hookUrl = hook.data ? `${window.location.origin}${hook.data.path}` : ''

  const ssh = sshUrl(app.git_url)
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      {ssh && (
        <div className="rounded-lg border border-warn/40 bg-warn/10 px-4 py-3 text-sm lg:col-span-2" data-testid="https-warning">
          <p>
            {tx('지금 Git 주소가 {url} 입니다. 공개 저장소면 그대로 되지만, {private} 저장소는 SSH 주소로 바꾸고 아래 Deploy key 를 저장소에 등록해야 합니다.', {
              url: <span className="font-mono">{app.git_url}</span>,
              private: <b>{t('비공개')}</b>,
            })}
          </p>
          <button
            className="btn-ghost mt-2 text-xs"
            disabled={busy}
            onClick={async () => {
              setBusy(true)
              try {
                const n = await put(`/apps/${app.id}`, toPayload({ ...app, git_url: ssh }))
                onSaved?.(n)
                toast.push(t('Git 주소를 {url} 로 바꿨습니다', { url: ssh }), 'success')
              } catch (e) {
                toast.push(e.message, 'error')
              } finally {
                setBusy(false)
              }
            }}
          >
            {t('{url} 로 바꾸기', { url: ssh })}
          </button>
        </div>
      )}
      <section className="card p-5">
        <CardHeader
          title="Deploy key (SSH)"
          help={
            <>
              {t('이 앱 전용 읽기 키입니다. 비공개 저장소에 공개키를 등록하세요 (쓰기 권한은 체크하지 않음). 저장소마다 따로 등록해야 합니다.')}
              <br />
              <br />
              {tx('GitHub: 저장소 → Settings → {keys} → Add deploy key', { keys: <b>Deploy keys</b> })}
              <br />
              {tx('GitLab: 프로젝트 → Settings → Repository → {keys}', { keys: <b>Deploy keys</b> })}
              <br />
              <br />
              {tx('Git URL 예: {url} — 이 서버에서 접속할 수 있는 주소를 쓰세요.', {
                url: <span className="font-mono break-all">{t('ssh://git@gitlab.example.com:2222/그룹/저장소.git')}</span>,
              })}
            </>
          }
        />
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
          <p className="mt-4 text-sm text-ink-400">{t('아직 키가 없습니다. public 저장소라면 필요 없습니다.')}</p>
        )}
        <button
          className="btn-ghost mt-4 text-xs"
          disabled={busy}
          onClick={async () => {
            if (key.data?.public_key && !(await confirmDialog(t('키를 새로 만들면 저장소(GitHub·GitLab)에 다시 등록해야 합니다. 계속할까요?')))) return
            setBusy(true)
            try {
              const r = await post(`/apps/${app.id}/deploy-key`)
              key.setData(r)
              toast.push(t('Deploy key 생성됨'), 'success')
            } catch (e) {
              toast.push(e.message, 'error')
            } finally {
              setBusy(false)
            }
          }}
        >
          {key.data?.public_key ? t('키 다시 만들기') : t('Deploy key 만들기')}
        </button>
      </section>

      <section className="card p-5">
        <CardHeader
          title={t('Webhook — push 시 자동 배포')}
          help={
            <>
              {tx('GitHub: 저장소 → Settings → Webhooks → Add webhook → Payload URL 에 URL, Content type 은 {json}, Secret 에 아래 Secret, 이벤트는 {push}.', {
                json: <b>application/json</b>,
                push: <b>Just the push event</b>,
              })}
              <br />
              <br />
              {tx('GitLab: Settings → Webhooks → URL 과 Secret token 입력, {push} 체크.', { push: <b>Push events</b> })}
              <br />
              <br />
              {tx('브랜치 {branch} 에 push 할 때만 배포합니다.', { branch: <span className="font-mono">{hook.data?.branch}</span> })}
              <br />
              <br />
              {tx('Git 서버가 이 패널 주소(8443)에 접속할 수 있어야 합니다. 닿지 않으면 CI 에서 {curl} 로 호출하세요.', {
                curl: <span className="font-mono break-all">curl -X POST -H "X-Gitlab-Token: …"</span>,
              })}
            </>
          }
        />
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
              <span className="label">Secret</span>
              <div className="flex gap-2">
                <input readOnly aria-label="Webhook token" type="password" className="input font-mono text-xs" value={hook.data.token} />
                <CopyButton text={hook.data.token} />
              </div>
            </div>
            <button
              className="btn-ghost text-xs"
              onClick={async () => {
                if (!(await confirmDialog(t('토큰을 바꾸면 GitHub·GitLab 웹훅 설정도 바꿔야 합니다. 계속할까요?')))) return
                try {
                  hook.setData(await post(`/apps/${app.id}/webhook`))
                  toast.push(t('토큰을 교체했습니다'), 'success')
                } catch (e) {
                  toast.push(e.message, 'error')
                }
              }}
            >
              {t('토큰 교체')}
            </button>
          </div>
        )}
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
                toast.push(t('저장됨 — 다음 배포부터 적용됩니다'), 'success')
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
        <h3 className="font-medium text-fail">{t('앱 삭제')}</h3>
        <p className="mt-1 text-sm text-ink-400">
          {tx('systemd 서비스({unit}), Nginx 사이트, 빌드·릴리스 파일, 배포 기록을 모두 지웁니다. 되돌릴 수 없습니다.', {
            unit: <span className="font-mono">rpanel-app-{app.slug}</span>,
          })}
        </p>
        <button
          className="btn-danger mt-4"
          onClick={async () => {
            const ok = await confirmDialog(t('앱 {slug} 을(를) 삭제합니다. 서비스·Nginx 설정·빌드 파일·배포 기록이 모두 지워지며 되돌릴 수 없습니다.', { slug: app.slug }), {
              title: t('앱 삭제'),
              requireText: app.slug,
              danger: true,
            })
            if (!ok) return
            try {
              await del(`/apps/${app.id}`)
              toast.push(t('{slug} 삭제됨', { slug: app.slug }), 'success')
              nav(`/projects/${app.project_id}`)
            } catch (e) {
              toast.push(e.message, 'error')
            }
          }}
        >
          {t('앱 삭제')}
        </button>
      </div>
    </div>
  )
}
