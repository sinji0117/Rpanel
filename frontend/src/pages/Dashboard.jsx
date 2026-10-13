import { Link } from 'react-router'
import Sparkline from '../components/Sparkline'
import { ErrorBox, Meter, PageHeader, StatusBadge } from '../components/ui'
import { get } from '../lib/api'
import { ago, bytes, percent, shortSha, uptime } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { useMetrics } from '../lib/useSockets'

function Stat({ label, value, sub }) {
  return (
    <div className="card px-5 py-4">
      <p className="text-xs text-ink-400">{label}</p>
      <p className="mt-1.5 text-2xl font-semibold tabular-nums">{value}</p>
      {sub && <p className="mt-1 text-xs text-ink-400">{sub}</p>}
    </div>
  )
}

export default function Dashboard() {
  const overview = useFetch(() => get('/system/overview'), [])
  const info = useFetch(() => get('/system/info'), [])
  const { history, latest, connected } = useMetrics(60)
  useInterval(() => overview.reload(true), 10000)

  const o = overview.data
  const host = info.data?.host
  const cpuHist = history.map((h) => h.cpu_percent)
  const memHist = history.map((h) => percent(h.mem_used, h.mem_total))

  return (
    <>
      <PageHeader
        title="대시보드"
        subtitle={host ? `${host.hostname ?? 'app-server'} · ${host.os ?? ''} · ${host.cpu_count} vCPU` : '서버 상태'}
        actions={
          <span className={`inline-flex items-center gap-2 text-xs ${connected ? 'text-signal' : 'text-ink-400'}`}>
            <span className={`h-1.5 w-1.5 rounded-full ${connected ? 'bg-signal animate-pulse' : 'bg-ink-600'}`} />
            {connected ? '실시간' : '연결 중…'}
          </span>
        }
      />

      {info.data?.exec_mode === 'dry-run' && (
        <div className="mb-6 rounded-lg border border-warn/40 bg-warn/10 px-4 py-3 text-sm text-amber-200">
          <b>DRY-RUN 모드</b> — 배포 명령을 실제로 실행하지 않고 로그만 남깁니다. 운영 시 <span className="kbd">RPANEL_EXEC_MODE=real</span>
        </div>
      )}
      <ErrorBox error={overview.error} />

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Stat label="프로젝트" value={o?.projects ?? '–'} />
        <Stat label="앱" value={o?.apps.total ?? '–'} sub={o ? `실행 중 ${o.apps.by_status.running ?? 0} · 실패 ${o.apps.by_status.failed ?? 0}` : null} />
        <Stat label="배포 (24시간)" value={o?.deployments_24h.total ?? '–'} sub={o ? `실패 ${o.deployments_24h.failed}` : null} />
        <Stat label="가동 시간" value={latest ? uptime(latest.uptime) : '–'} sub={latest ? `load ${latest.load.map((l) => l.toFixed(2)).join(' · ')}` : null} />
      </div>

      <div className="mt-4 grid gap-4 lg:grid-cols-3">
        <div className="card p-5">
          <div className="flex items-baseline justify-between">
            <p className="text-sm font-medium">CPU</p>
            <p className="font-mono text-lg tabular-nums" data-testid="cpu-value">
              {latest ? `${latest.cpu_percent.toFixed(1)}%` : '–'}
            </p>
          </div>
          <div className="mt-4 h-28">
            <Sparkline values={cpuHist} />
          </div>
        </div>
        <div className="card p-5">
          <div className="flex items-baseline justify-between">
            <p className="text-sm font-medium">메모리</p>
            <p className="font-mono text-lg tabular-nums">{latest ? `${bytes(latest.mem_used)} / ${bytes(latest.mem_total)}` : '–'}</p>
          </div>
          <div className="mt-4 h-28">
            <Sparkline values={memHist} className="text-sky-400" />
          </div>
        </div>
        <div className="card space-y-4 p-5">
          <p className="text-sm font-medium">디스크</p>
          {latest?.disks.length ? (
            latest.disks.slice(0, 4).map((d) => (
              <Meter key={d.mount} label={d.mount} value={percent(d.used, d.total)} detail={`${bytes(d.used)} / ${bytes(d.total)}`} />
            ))
          ) : (
            <p className="text-sm text-ink-400">–</p>
          )}
          {latest && latest.swap_total > 0 && <Meter label="swap" value={percent(latest.swap_used, latest.swap_total)} detail={bytes(latest.swap_used)} />}
        </div>
      </div>

      <div className="card mt-4">
        <div className="flex items-center justify-between border-b border-ink-700/70 px-5 py-3.5">
          <h2 className="text-sm font-medium">최근 배포</h2>
          <Link to="/projects" className="text-xs text-ink-400 hover:text-ink-100">
            프로젝트 →
          </Link>
        </div>
        {o?.recent_deployments.length ? (
          <ul className="divide-y divide-ink-800">
            {o.recent_deployments.map((d) => (
              <li key={d.id}>
                <Link to={`/deployments/${d.id}`} className="flex items-center gap-4 px-5 py-3 text-sm hover:bg-ink-850">
                  <StatusBadge status={d.status} deployment />
                  <span className="min-w-0 flex-1 truncate">
                    {d.app_name} <span className="text-ink-400">#{d.id}</span>
                  </span>
                  <span className="hidden font-mono text-xs text-ink-400 sm:inline">{shortSha(d.commit_sha)}</span>
                  <span className="w-20 text-right text-xs text-ink-400">{ago(d.created_at)}</span>
                </Link>
              </li>
            ))}
          </ul>
        ) : (
          <p className="px-5 py-8 text-center text-sm text-ink-400">아직 배포가 없습니다</p>
        )}
      </div>
    </>
  )
}
