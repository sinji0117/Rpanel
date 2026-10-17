import { Link } from 'react-router'
import { useState } from 'react'
import LineChart, { timeLabel } from '../components/LineChart'
import { VersionRows } from '../components/VersionInfo'
import { ErrorBox, HelpTip, PageHeader, StatusBadge } from '../components/ui'
import { get } from '../lib/api'
import { ago, bytes, percent, shortSha, uptime } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { useMetrics } from '../lib/useSockets'
import SystemResourceMonitor from '../components/SystemResourceMonitor'
import SystemLogViewer from '../components/SystemLogViewer'
import ExportDataModal from '../components/ExportDataModal'
import HistoricalTrendChart from '../components/HistoricalTrendChart'
import SystemSnapshotModal from '../components/SystemSnapshotModal'
import { t, tx } from '../i18n'

function Stat({ label, value, sub }) {
  return (
    <div className="card px-5 py-4">
      <p className="text-xs text-ink-400">{label}</p>
      <p className="mt-1.5 text-2xl font-semibold tabular-nums">{value}</p>
      {sub && <p className="mt-1 text-xs text-ink-400">{sub}</p>}
    </div>
  )
}

const RANGES = [
  { id: '30m', label: '최근 30분' },
  { id: '1h', label: '최근 1시간' },
  { id: '6h', label: '최근 6시간' },
  { id: '24h', label: '최근 24시간' },
  { id: '7d', label: '최근 7일' },
]
const DISK_COLORS = ['#22c55e', '#a3e635', '#14b8a6', '#eab308']
const pct = (v) => `${Math.round(v)}%`

/** 서버 가동 시간 — 부팅 시각으로 계산해 화면에서 계속 갱신 (실시간 연결이 끊겨도 표시) */
function UptimeStat({ bootTime, panelStarted, latest }) {
  const [now, setNow] = useState(() => Date.now())
  useInterval(() => setNow(Date.now()), 30000)
  const secs = bootTime ? Math.max(0, Math.floor(now / 1000) - bootTime) : latest?.uptime
  const sub = [
    bootTime ? t('부팅 {at}', { at: timeLabel(bootTime, 99 * 3600) }) : null,
    panelStarted ? t('패널 {v}', { v: uptime(Math.max(0, Math.floor(now / 1000) - panelStarted)) }) : null,
  ]
    .filter(Boolean)
    .join(' · ')
  return <Stat label={t('가동 시간')} value={secs != null ? <span data-testid="uptime-value">{uptime(secs)}</span> : '–'} sub={sub || null} />
}

function ChartCard({ title, right, children }) {
  return (
    <div className="rounded-lg border border-ink-700/70 bg-ink-950/40">
      <div className="flex items-center justify-between border-b border-ink-700/70 px-4 py-2.5">
        <h3 className="text-sm font-medium">{title}</h3>
        <span className="text-sm font-medium tabular-nums">{right}</span>
      </div>
      <div className="px-2 pt-3 pb-2">{children}</div>
    </div>
  )
}

/** 모니터링: CPU · 메모리 · 디스크 · Load average 기록 그래프 */
function Monitoring({ latest, cpuCount, autoRefresh = true }) {
  const [range, setRange] = useState(() => {
    try {
      return localStorage.getItem('rpanel.metricsRange') || '30m'
    } catch {
      return '30m'
    }
  })
  const hist = useFetch(() => get(`/system/metrics?range=${range}`), [range])
  useInterval(() => autoRefresh && hist.reload(true), 60000)
  const pts = hist.data?.points ?? []
  const times = pts.map((p) => p.t)
  const cpus = hist.data?.cpu_count || cpuCount
  const memTotal = latest?.mem_total ?? pts[pts.length - 1]?.mt
  const mounts = [...new Set(pts.flatMap((p) => p.d.map((d) => d[0])))].slice(0, 4)
  const diskTotal = (m) => {
    for (let i = pts.length - 1; i >= 0; i--) {
      const d = pts[i].d.find((x) => x[0] === m)
      if (d) return d[2]
    }
    return 0
  }

  return (
    <section className="card mt-4" aria-label={t('모니터링')}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink-700/70 px-5 py-3.5">
        <h2 className="text-sm font-medium">{t('모니터링')}</h2>
        <div className="flex items-center gap-2">
          <HelpTip label={t('모니터링 도움말')}>{t('패널이 1분마다 서버 상태를 기록합니다 (최근 7일 보관). 그래프에 마우스를 올리면 그 시각의 값을 볼 수 있습니다.')}</HelpTip>
          <select
            aria-label={t('기간')}
            className="rounded-lg border border-ink-700 bg-ink-900 px-2 py-1 text-xs"
            value={range}
            onChange={(e) => {
              setRange(e.target.value)
              try {
                localStorage.setItem('rpanel.metricsRange', e.target.value)
              } catch {
                /* 저장 못 해도 동작 */
              }
            }}
          >
            {RANGES.map((r) => (
              <option key={r.id} value={r.id}>
                {t(r.label)}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="p-5">
        <ErrorBox error={hist.error} />
        {!hist.loading && pts.length < 2 && (
          <p className="mb-4 rounded-lg border border-ink-700 bg-ink-950/60 px-4 py-2.5 text-xs text-ink-400">{t('기록을 모으는 중입니다. 1분마다 한 점씩 쌓입니다.')}</p>
        )}
        <div className="grid gap-4 lg:grid-cols-2">
          <ChartCard
            title={t('CPU 사용률')}
            right={
              <>
                <span data-testid="cpu-value">{latest ? `${latest.cpu_percent.toFixed(1)}%` : '–'}</span>
                <span className="ml-2 text-xs text-ink-400">{cpus ? t('{n} CPU', { n: cpus }) : ''}</span>
              </>
            }
          >
            <LineChart label={t('CPU 사용률')} times={times} max={100} format={pct} series={[{ name: 'CPU', color: '#3b82f6', values: pts.map((p) => p.c) }]} />
          </ChartCard>
          <ChartCard
            title={t('메모리 사용률')}
            right={
              <>
                <span data-testid="mem-value">{latest ? `${bytes(latest.mem_used)} / ${bytes(latest.mem_total)}` : '–'}</span>
              </>
            }
          >
            <LineChart
              label={t('메모리 사용률')}
              times={times}
              max={100}
              format={pct}
              series={[{ name: t('메모리'), color: '#f97316', values: pts.map((p) => percent(p.m, p.mt || memTotal)) }]}
            />
          </ChartCard>
          <ChartCard title={t('디스크 사용률')} right={latest?.disks[0] ? `${bytes(latest.disks[0].used)} / ${bytes(latest.disks[0].total)}` : ''}>
            <LineChart
              label={t('디스크 사용률')}
              times={times}
              max={100}
              format={pct}
              legend
              series={mounts.map((m, i) => ({
                name: `${m} (${bytes(diskTotal(m))})`,
                color: DISK_COLORS[i % DISK_COLORS.length],
                values: pts.map((p) => {
                  const d = p.d.find((x) => x[0] === m)
                  return d ? percent(d[1], d[2]) : null
                }),
              }))}
            />
          </ChartCard>
          <ChartCard title="Load Average" right={cpus ? <span className="text-xs text-ink-400">{t('{n} CPU', { n: cpus })}</span> : ''}>
            <LineChart
              label="Load Average"
              times={times}
              format={(v) => (v >= 10 ? Math.round(v) : Number(v.toFixed(2)))}
              legend
              series={[
                { name: t('1분'), color: '#a855f7', values: pts.map((p) => p.l[0]) },
                { name: t('5분'), color: '#ef4444', values: pts.map((p) => p.l[1]) },
                { name: t('15분'), color: '#0ea5e9', values: pts.map((p) => p.l[2]) },
              ]}
            />
          </ChartCard>
        </div>
      </div>
    </section>
  )
}

export default function Dashboard() {
  const [autoRefresh, setAutoRefresh] = useState(true)
  const [exportOpen, setExportOpen] = useState(false)
  const [snapshotOpen, setSnapshotOpen] = useState(false)
  const overview = useFetch(() => get('/system/overview'), [])
  const info = useFetch(() => get('/system/info'), [])
  const { latest, connected } = useMetrics(30, autoRefresh)
  useInterval(() => autoRefresh && overview.reload(true), 10000)

  const o = overview.data
  const host = info.data?.host

  return (
    <>
      <PageHeader
        title={t('대시보드')}
        subtitle={host ? `${host.hostname ?? 'app-server'} · ${host.os ?? ''} · ${host.cpu_count} vCPU` : t('서버 상태')}
        actions={
          <>
            <button
              type="button"
              role="switch"
              aria-checked={autoRefresh}
              onClick={() => setAutoRefresh((v) => !v)}
              className={`inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs font-medium transition cursor-pointer ${
                autoRefresh
                  ? 'border-signal/40 bg-signal/10 text-signal hover:bg-signal/20'
                  : 'border-ink-700 bg-ink-900 text-ink-400 hover:border-ink-600 hover:text-ink-200'
              }`}
              title={autoRefresh ? t('실시간 갱신 일시정지') : t('실시간 갱신 재개')}
            >
              <span className="relative flex h-2 w-2 items-center justify-center">
                {autoRefresh && connected ? (
                  <>
                    <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-signal opacity-75" />
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-signal" />
                  </>
                ) : (
                  <span className={`inline-flex h-2 w-2 rounded-full ${autoRefresh ? 'bg-signal' : 'bg-ink-600'}`} />
                )}
              </span>
              <span className="whitespace-nowrap font-medium">{t('자동 갱신')}</span>
              <span
                className={`rounded px-1.5 py-0.5 text-[10px] font-mono uppercase ${
                  autoRefresh ? 'bg-signal/20 text-signal' : 'bg-ink-800 text-ink-400'
                }`}
              >
                {autoRefresh ? t('켜짐') : t('일시정지')}
              </span>
            </button>

            <button
              type="button"
              onClick={() => setSnapshotOpen(true)}
              className="btn-ghost inline-flex items-center gap-1.5 px-3 py-1.5 text-xs text-ink-300 hover:text-ink-100 hover:border-ink-600 cursor-pointer"
              title={t('시스템 스냅샷')}
            >
              <span aria-hidden="true">📷</span>
              <span>{t('시스템 스냅샷')}</span>
            </button>

            <button
              type="button"
              onClick={() => setExportOpen(true)}
              className="btn-ghost inline-flex items-center gap-1.5 px-3 py-1.5 text-xs text-ink-300 hover:text-ink-100 hover:border-ink-600 cursor-pointer"
              title={t('데이터 내보내기')}
            >
              <span aria-hidden="true">📥</span>
              <span>{t('내보내기')}</span>
            </button>

            <Link to="/sites?new=1" className="btn-primary">
              {t('+ 사이트 추가')}
            </Link>
          </>
        }
      />

      <ExportDataModal
        open={exportOpen}
        onClose={() => setExportOpen(false)}
        latest={latest}
        host={host}
      />

      <SystemSnapshotModal
        open={snapshotOpen}
        onClose={() => setSnapshotOpen(false)}
        latest={latest}
        host={host}
      />

      {info.data?.exec_mode === 'dry-run' && (
        <div className="mb-6 rounded-lg border border-warn/40 bg-warn/10 px-4 py-3 text-sm text-amber-200">
          {tx('{mode} — 배포 명령을 실제로 실행하지 않고 로그만 남깁니다. 운영 시 {env}', {
            mode: <b>{t('DRY-RUN 모드')}</b>,
            env: <span className="kbd">RPANEL_EXEC_MODE=real</span>,
          })}
        </div>
      )}
      <ErrorBox error={overview.error} />

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Stat label={t('프로젝트')} value={o?.projects ?? '–'} />
        <Stat label={t('앱')} value={o?.apps.total ?? '–'} sub={o ? t('실행 중 {v} · 실패 {v2}', { v: o.apps.by_status.running ?? 0, v2: o.apps.by_status.failed ?? 0 }) : null} />
        <Stat label={t('배포 (24시간)')} value={o?.deployments_24h.total ?? '–'} sub={o ? t('실패 {failed}', { failed: o.deployments_24h.failed }) : null} />
        <UptimeStat bootTime={info.data?.boot_time} panelStarted={info.data?.panel_started_at} latest={latest} />
      </div>

      <div className="mt-4">
        <SystemResourceMonitor
          latest={latest}
          host={host}
          connected={connected}
          autoRefresh={autoRefresh}
          onToggleAutoRefresh={() => setAutoRefresh((v) => !v)}
        />
      </div>

      <div className="mt-4">
        <HistoricalTrendChart autoRefresh={autoRefresh} />
      </div>

      <Monitoring latest={latest} cpuCount={host?.cpu_count} autoRefresh={autoRefresh} />

      <div className="mt-4">
        <SystemLogViewer autoRefresh={autoRefresh} />
      </div>

      <div className="mt-4 grid gap-4 lg:grid-cols-3">
        <div className="card p-5" aria-label={t('시스템')}>
          <h2 className="text-sm font-medium">{t('시스템')}</h2>
          {info.data && (
            <dl className="mt-4 grid grid-cols-[96px_1fr] gap-y-2 text-sm">
              <VersionRows />
              <dt className="text-ink-400">{t('실행 모드')}</dt>
              <dd className="font-mono">{info.data.exec_mode}</dd>
              <dt className="text-ink-400">{t('호스트')}</dt>
              <dd className="truncate font-mono">{host?.hostname}</dd>
              <dt className="text-ink-400">OS</dt>
              <dd className="truncate">{host?.os}</dd>
              <dt className="text-ink-400">{t('커널')}</dt>
              <dd className="font-mono">{host?.kernel}</dd>
              <dt className="text-ink-400">CPU</dt>
              <dd className="truncate">
                {host?.cpu_brand} × {host?.cpu_count}
              </dd>
              <dt className="text-ink-400">{t('앱 포트 범위')}</dt>
              <dd className="font-mono">{info.data.app_port_range?.join('–')}</dd>
            </dl>
          )}
        </div>
        <div className="card lg:col-span-2">
          <div className="flex items-center justify-between border-b border-ink-700/70 px-5 py-3.5">
            <h2 className="text-sm font-medium">{t('최근 배포')}</h2>
            <Link to="/projects" className="text-xs text-ink-400 hover:text-ink-100">
              {t('프로젝트 →')}
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
            <p className="px-5 py-8 text-center text-sm text-ink-400">{t('아직 배포가 없습니다')}</p>
          )}
        </div>
      </div>
    </>
  )
}
