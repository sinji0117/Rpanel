import { useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import LogViewer from '../components/LogViewer'
import { ErrorBox, PageHeader, Spinner, StatusBadge, Tabs } from '../components/ui'
import { t } from '../i18n'
import { get } from '../lib/api'
import { dateTime, duration, shortSha } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { JournalTab } from './AppDetail'
import { NginxLogTab } from './AppTabs'

// label·desc 는 한국어 원문 (화면에서 t())
const TABS = [
  { id: 'audit', label: '활동 기록' },
  { id: 'deployments', label: '배포 기록' },
  { id: 'service', label: '서비스 로그' },
  { id: 'nginx', label: '사이트 접속 로그' },
  { id: 'cron', label: '크론 로그' },
  { id: 'system', label: '서버 로그' },
]

const SYSTEM_KINDS = [
  { id: 'panel', label: '패널 (Rpanel)', cmd: 'journalctl -u rpanel' },
  { id: 'nginx', label: '웹 서버 (Nginx) 오류', cmd: '/var/log/nginx/error.log' },
  { id: 'ssh', label: 'SSH 접속', cmd: 'journalctl -u ssh' },
  { id: 'mariadb', label: 'MariaDB', cmd: 'journalctl -u mariadb' },
  { id: 'redis', label: 'Redis', cmd: 'journalctl -u redis-server' },
]

/** 글자로 걸러 보기 */
function useFilter(lines) {
  const [q, setQ] = useState('')
  const shown = useMemo(() => {
    const k = q.trim().toLowerCase()
    return (k ? lines.filter((l) => l.toLowerCase().includes(k)) : lines).map((text, i) => ({ seq: i + 1, text }))
  }, [lines, q])
  const input = <input className="input w-48 py-1 text-xs" placeholder={t('검색…')} aria-label={t('로그 검색')} value={q} onChange={(e) => setQ(e.target.value)} />
  return [shown, input]
}

function LinesSelect({ value, onChange, options = [100, 200, 500, 1000, 2000] }) {
  return (
    <select className="input w-auto py-1 text-xs" value={value} onChange={(e) => onChange(Number(e.target.value))} aria-label={t('줄 수')}>
      {options.map((x) => (
        <option key={x} value={x}>
          {t('{x}줄', { x })}
        </option>
      ))}
    </select>
  )
}

/** 앱(사이트) 고르기 — filter 로 대상 앱만 */
function AppPicker({ apps, value, onChange, label }) {
  return (
    <select className="input w-auto min-w-[220px] py-1.5 text-sm" aria-label={label} value={value ?? ''} onChange={(e) => onChange(Number(e.target.value) || null)}>
      {!apps.length && <option value="">{t('대상이 없습니다')}</option>}
      {apps.map((a) => (
        <option key={a.id} value={a.id}>
          {a.name} {a.domains ? `· ${a.domains.split(' ')[0]}` : ''}
        </option>
      ))}
    </select>
  )
}

function AuditLog() {
  const [n, setN] = useState(200)
  const audit = useFetch(() => get(`/audit?limit=${Math.min(n, 500)}`), [n])
  const [q, setQ] = useState('')
  const k = q.trim().toLowerCase()
  const rows = (audit.data ?? []).filter((e) => !k || [e.action, e.target, e.email, e.ip].some((v) => v?.toLowerCase().includes(k)))
  return (
    <section className="card">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink-700/70 px-5 py-3">
        <p className="text-xs text-ink-400">{t('패널에서 누가 언제 무엇을 했는지 (로그인, 만들기·바꾸기·지우기)')}</p>
        <div className="flex items-center gap-2">
          <input className="input w-48 py-1 text-xs" placeholder={t('검색…')} aria-label={t('활동 검색')} value={q} onChange={(e) => setQ(e.target.value)} />
          <LinesSelect value={n} onChange={setN} options={[100, 200, 500]} />
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => audit.reload()}>
            {t('새로고침')}
          </button>
        </div>
      </div>
      <ErrorBox error={audit.error} />
      <div className="max-h-[65vh] overflow-auto">
        <table className="w-full text-sm" aria-label={t('활동 기록')}>
          <thead className="sticky top-0 bg-ink-900 text-left text-xs text-ink-400">
            <tr>
              <th className="px-5 py-2 font-medium">{t('시각')}</th>
              <th className="px-3 py-2 font-medium">{t('작업')}</th>
              <th className="px-3 py-2 font-medium">{t('대상')}</th>
              <th className="px-3 py-2 font-medium">{t('관리자')}</th>
              <th className="px-5 py-2 font-medium">IP</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-ink-800">
            {rows.map((e) => (
              <tr key={e.id}>
                <td className="px-5 py-2 text-xs whitespace-nowrap text-ink-400">{dateTime(e.created_at)}</td>
                <td className="px-3 py-2 font-mono text-xs">{e.action}</td>
                <td className="px-3 py-2 text-xs text-ink-300">{e.target}</td>
                <td className="px-3 py-2 text-xs text-ink-400">{e.email ?? '—'}</td>
                <td className="px-5 py-2 font-mono text-xs text-ink-400">{e.ip}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {!audit.loading && !rows.length && <p className="px-5 py-8 text-center text-sm text-ink-400">{t('기록이 없습니다')}</p>}
      </div>
    </section>
  )
}

function DeploymentLog() {
  const list = useFetch(() => get('/logs/deployments?limit=200'), [])
  useInterval(() => list.reload(true), 10000)
  return (
    <section className="card">
      <div className="flex items-center justify-between border-b border-ink-700/70 px-5 py-3">
        <p className="text-xs text-ink-400">{t('모든 앱·사이트의 배포·적용 기록. 줄을 누르면 전체 로그를 봅니다.')}</p>
        <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => list.reload()}>
          {t('새로고침')}
        </button>
      </div>
      <ErrorBox error={list.error} />
      {list.loading && <Spinner className="m-5 text-ink-400" />}
      <ul className="max-h-[65vh] divide-y divide-ink-800 overflow-auto" aria-label={t('배포 기록')}>
        {list.data?.map((d) => (
          <li key={d.id}>
            <Link to={`/deployments/${d.id}`} className="flex items-center gap-4 px-5 py-2.5 text-sm hover:bg-ink-850">
              <StatusBadge status={d.status} deployment />
              <span className="min-w-0 flex-1 truncate">
                {d.app_name} <span className="text-ink-400">#{d.id}</span>
              </span>
              <span className="hidden font-mono text-xs text-ink-400 sm:inline">{shortSha(d.commit_sha)}</span>
              <span className="hidden w-20 text-right text-xs text-ink-400 sm:inline">{duration(d.created_at, d.finished_at)}</span>
              <span className="w-36 text-right text-xs text-ink-400">{dateTime(d.created_at)}</span>
            </Link>
          </li>
        ))}
        {list.data && !list.data.length && <li className="px-5 py-8 text-center text-sm text-ink-400">{t('아직 배포가 없습니다')}</li>}
      </ul>
    </section>
  )
}

function PerApp({ apps, filter, label, render, empty }) {
  const targets = (apps ?? []).filter(filter)
  const [id, setId] = useState(null)
  const app = targets.find((a) => a.id === id) ?? targets[0]
  if (!apps) return <Spinner className="text-ink-400" />
  if (!targets.length) return <p className="card px-5 py-10 text-center text-sm text-ink-400">{empty}</p>
  return (
    <div className="space-y-4">
      <AppPicker apps={targets} value={app?.id} onChange={setId} label={label} />
      {app && <div key={app.id}>{render(app)}</div>}
    </div>
  )
}

function CronLog({ app }) {
  const jobs = useFetch(() => get(`/apps/${app.id}/cron`), [app.id])
  const [jobId, setJobId] = useState(null)
  const job = jobs.data?.find((j) => j.id === jobId) ?? jobs.data?.[0]
  const log = useFetch(() => (job ? get(`/cron/${job.id}/log`) : Promise.resolve({ lines: [] })), [job?.id])
  const [lines, search] = useFilter(log.data?.lines ?? [])
  if (jobs.loading) return <Spinner className="text-ink-400" />
  if (!jobs.data?.length) return <p className="card px-5 py-10 text-center text-sm text-ink-400">{t('이 앱에는 크론 작업이 없습니다')}</p>
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <select className="input w-auto py-1.5 text-sm" aria-label={t('크론 작업')} value={job?.id ?? ''} onChange={(e) => setJobId(Number(e.target.value))}>
          {jobs.data.map((j) => (
            <option key={j.id} value={j.id}>
              {j.name} · {j.schedule}
            </option>
          ))}
        </select>
        <span className="ml-auto" />
        {search}
        <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => log.reload()}>
          {t('새로고침')}
        </button>
      </div>
      <ErrorBox error={log.error} />
      <LogViewer lines={lines} emptyText={log.loading ? t('불러오는 중…') : t('아직 실행 기록이 없습니다')} />
    </div>
  )
}

function SystemLog() {
  const [kind, setKind] = useState('panel')
  const [n, setN] = useState(200)
  const [follow, setFollow] = useState(false)
  const log = useFetch(() => get(`/logs/system?kind=${kind}&lines=${n}`), [kind, n])
  useInterval(() => log.reload(true), 3000, follow)
  const [lines, search] = useFilter(log.data?.lines ?? [])
  const meta = SYSTEM_KINDS.find((k) => k.id === kind)
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        {SYSTEM_KINDS.map((k) => (
          <button
            key={k.id}
            className={`rounded-lg border px-3 py-1.5 text-sm ${kind === k.id ? 'border-signal/60 text-ink-100' : 'border-ink-700 text-ink-400 hover:text-ink-100'}`}
            aria-pressed={kind === k.id}
            onClick={() => setKind(k.id)}
          >
            {t(k.label)}
          </button>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-mono text-xs text-ink-400">{meta.cmd}</span>
        <span className="ml-auto" />
        {search}
        <LinesSelect value={n} onChange={setN} />
        <label className="flex items-center gap-1.5 text-xs text-ink-300">
          <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} /> {t('3초마다 갱신')}
        </label>
        <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => log.reload()}>
          {t('새로고침')}
        </button>
      </div>
      <ErrorBox error={log.error} />
      <LogViewer lines={lines} emptyText={log.loading ? t('불러오는 중…') : t('로그가 없습니다')} />
    </div>
  )
}

export default function LogsPage() {
  const [params, setParams] = useSearchParams()
  const tab = TABS.some((x) => x.id === params.get('tab')) ? params.get('tab') : 'audit'
  const apps = useFetch(() => get('/apps'), [])
  const isService = (a) => (a.kind === 'rust' || a.kind === 'python') && a.port
  const hasNginx = (a) => !!a.domains
  const hasCron = (a) => a.kind !== 'proxy'

  return (
    <>
      <PageHeader
        title={t('로그')}
        help={t('패널 활동, 배포, 앱 서비스, 사이트 접속, 크론, 서버(패널·Nginx·SSH·DB) 로그를 종류별로 봅니다. 앱 화면의 로그 탭과 같은 내용입니다.')}
      />
      <Tabs tabs={TABS.map((x) => ({ id: x.id, label: t(x.label) }))} active={tab} onChange={(id) => setParams({ tab: id }, { replace: true })} />
      <ErrorBox error={apps.error} />
      {tab === 'audit' && <AuditLog />}
      {tab === 'deployments' && <DeploymentLog />}
      {tab === 'service' && (
        <PerApp apps={apps.data} filter={isService} label={t('앱 선택')} empty={t('실행 중인 Rust·Python 앱이 없습니다')} render={(a) => <JournalTab app={a} />} />
      )}
      {tab === 'nginx' && (
        <PerApp apps={apps.data} filter={hasNginx} label={t('사이트 선택')} empty={t('도메인이 있는 사이트가 없습니다')} render={(a) => <NginxLogTab app={a} />} />
      )}
      {tab === 'cron' && <PerApp apps={apps.data} filter={hasCron} label={t('앱 선택')} empty={t('앱이 없습니다')} render={(a) => <CronLog app={a} />} />}
      {tab === 'system' && <SystemLog />}
    </>
  )
}
