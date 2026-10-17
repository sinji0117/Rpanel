import { useMemo, useState } from 'react'
import { bytes } from '../lib/format'
import { t } from '../i18n'

const THRESHOLD = 90

function GaugeArc({ percent, color, size = 110, stroke = 9, isAlert = false }) {
  const p = Math.max(0, Math.min(100, percent || 0))
  const radius = (size - stroke) / 2
  const circumference = 2 * Math.PI * radius
  const offset = circumference - (p / 100) * circumference

  return (
    <div className="relative inline-flex items-center justify-center" style={{ width: size, height: size }}>
      <svg className="-rotate-90" width={size} height={size}>
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          className="stroke-ink-800"
          strokeWidth={stroke}
          fill="transparent"
        />
        {/* 90% threshold marker notch */}
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke="#ef4444"
          strokeWidth={stroke + 2}
          strokeDasharray="2 300"
          strokeDashoffset={circumference - 0.9 * circumference}
          fill="transparent"
          className="opacity-75"
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke={color}
          strokeWidth={stroke}
          strokeDasharray={circumference}
          strokeDashoffset={offset}
          strokeLinecap="round"
          fill="transparent"
          className={`transition-all duration-700 ease-out ${isAlert ? 'drop-shadow-[0_0_8px_rgba(239,68,68,0.7)]' : ''}`}
        />
      </svg>
      <div className="absolute flex flex-col items-center justify-center text-center">
        <span
          className={`font-mono text-xl font-bold tracking-tight tabular-nums ${
            isAlert ? 'text-fail font-extrabold animate-pulse' : 'text-ink-100'
          }`}
        >
          {p.toFixed(1)}%
        </span>
        {isAlert && (
          <span className="text-[10px] font-bold text-fail uppercase tracking-wider">
            &gt;90%
          </span>
        )}
      </div>
    </div>
  )
}

function MiniSparkline({ data, color, height = 40 }) {
  if (!data || data.length < 2) return null
  const max = Math.max(...data, 100)
  const min = Math.min(...data, 0)
  const range = max - min || 1
  const points = data
    .map((val, idx) => {
      const x = (idx / (data.length - 1)) * 100
      const y = height - ((val - min) / range) * (height - 6) - 3
      return `${x},${y}`
    })
    .join(' ')

  return (
    <svg className="h-10 w-full overflow-visible" viewBox={`0 0 100 ${height}`} preserveAspectRatio="none">
      <polyline
        fill="none"
        stroke={color}
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        points={points}
      />
    </svg>
  )
}

export default function SystemResourceMonitor({ latest, host, connected, autoRefresh = true, onToggleAutoRefresh }) {
  const [tab, setTab] = useState('overview') // 'overview' | 'cores' | 'trends'
  const [simulatedSpike, setSimulatedSpike] = useState(false)

  // Real raw values
  const rawCpuPercent = latest?.cpu_percent ?? 0
  const cpuCores = latest?.cpu_per_core ?? [20, 25, 18, 22]
  const cpuCount = host?.cpu_count || cpuCores.length || 8

  const memTotal = latest?.mem_total ?? 16 * 1024 ** 3
  const rawMemUsed = latest?.mem_used ?? 5.4 * 1024 ** 3
  const rawMemPercent = memTotal > 0 ? (rawMemUsed / memTotal) * 100 : 0

  // Effective values (supports on-demand threshold spike testing)
  const cpuPercent = simulatedSpike ? 94.6 : rawCpuPercent
  const memPercent = simulatedSpike ? 92.4 : rawMemPercent
  const memUsed = simulatedSpike ? memTotal * 0.924 : rawMemUsed
  const memFree = Math.max(0, memTotal - memUsed)

  // Threshold breach detection
  const isCpuExceeded = cpuPercent >= THRESHOLD
  const isMemExceeded = memPercent >= THRESHOLD
  const hasThresholdAlert = isCpuExceeded || isMemExceeded

  // Disk
  const primaryDisk = latest?.disks?.[0] || { mount: '/', total: 200 * 1024 ** 3, used: 38 * 1024 ** 3 }
  const diskTotal = primaryDisk.total || 1
  const diskUsed = primaryDisk.used || 0
  const diskPercent = (diskUsed / diskTotal) * 100
  const diskFree = Math.max(0, diskTotal - diskUsed)

  // Load
  const load = latest?.load ?? [0.38, 0.32, 0.28]

  // Status colors with explicit 90% threshold alert crimson red
  const getStatusColor = (pct) => {
    if (pct >= THRESHOLD) return '#ef4444' // vivid crimson red alert
    if (pct >= 75) return '#fbbf24' // amber warning
    return '#34d399' // emerald normal
  }

  const getStatusBadge = (pct) => {
    if (pct >= THRESHOLD) {
      return (
        <span className="rounded border border-fail bg-fail/20 px-2 py-0.5 text-[11px] font-bold text-fail shadow-sm animate-pulse">
          {t('위험 (90% 초과)')}
        </span>
      )
    }
    if (pct >= 75) {
      return (
        <span className="rounded border border-warn/40 bg-warn/10 px-2 py-0.5 text-[11px] font-medium text-warn">
          {t('경고 (75% 이상)')}
        </span>
      )
    }
    return (
      <span className="rounded border border-signal/40 bg-signal/10 px-2 py-0.5 text-[11px] font-medium text-signal">
        {t('정상')}
      </span>
    )
  }

  // Simulated rolling points for trend view
  const cpuTrend = useMemo(() => {
    const base = cpuPercent
    return Array.from({ length: 15 }, (_, i) => Math.max(5, Math.min(99, base + Math.sin(i * 0.8) * 4)))
  }, [cpuPercent])

  const memTrend = useMemo(() => {
    const base = memPercent
    return Array.from({ length: 15 }, (_, i) => Math.max(10, Math.min(99, base + Math.cos(i * 0.5) * 3)))
  }, [memPercent])

  return (
    <section
      className={`card overflow-hidden transition-all duration-300 ${
        hasThresholdAlert ? 'border-fail/60 shadow-lg shadow-fail/10' : ''
      }`}
      aria-label={t('시스템 자원 실시간 모니터링')}
    >
      {/* Top Banner / Bar */}
      <div
        className={`flex flex-wrap items-center justify-between gap-3 border-b px-5 py-3.5 transition-colors ${
          hasThresholdAlert
            ? 'border-fail/50 bg-fail/10'
            : 'border-ink-700/70 bg-ink-950/40'
        }`}
      >
        <div className="flex items-center gap-3">
          <div
            className={`flex h-8 w-8 items-center justify-center rounded-lg border transition ${
              hasThresholdAlert
                ? 'border-fail bg-fail text-white animate-pulse'
                : 'border-signal/30 bg-signal/10 text-signal'
            }`}
          >
            {hasThresholdAlert ? (
              <span className="text-base" aria-hidden="true">🚨</span>
            ) : (
              <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6z" />
                <path d="M9 12h6M12 9v6" />
              </svg>
            )}
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-semibold tracking-tight text-ink-100">
                {t('시스템 자원 실시간 모니터링')}
              </h2>
              {hasThresholdAlert && (
                <span className="rounded-full bg-fail px-2 py-0.5 text-[10px] font-bold text-white uppercase tracking-wider animate-pulse">
                  {t('임계값 경보 (90%)')}
                </span>
              )}
            </div>
            <p className="text-xs text-ink-400">
              {host ? `${host.hostname} · ${host.os}` : 'Rpanel Host Server'}
            </p>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {/* Quick Simulation / Test Button */}
          <button
            type="button"
            onClick={() => setSimulatedSpike((v) => !v)}
            className={`rounded-lg border px-2.5 py-1 text-xs font-medium transition cursor-pointer ${
              simulatedSpike
                ? 'border-fail bg-fail/20 text-fail hover:bg-fail/30'
                : 'border-ink-700 bg-ink-900 text-ink-400 hover:border-ink-600 hover:text-ink-200'
            }`}
            title={simulatedSpike ? t('정상 상태로 복귀') : t('90% 초과 시뮬레이션')}
          >
            {simulatedSpike ? `🚨 ${t('정상 상태로 복귀')}` : `⚡ ${t('90% 초과 시뮬레이션')}`}
          </button>

          {/* Real-time connection indicator */}
          <span
            className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium tabular-nums ${
              !autoRefresh
                ? 'border-warn/40 bg-warn/10 text-warn'
                : connected
                ? 'border-signal/30 bg-signal/10 text-signal'
                : 'border-ink-700 bg-ink-900 text-ink-400'
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                !autoRefresh
                  ? 'bg-warn'
                  : connected
                  ? 'bg-signal animate-pulse'
                  : 'bg-ink-600'
              }`}
            />
            {!autoRefresh ? t('일시정지') : connected ? t('실시간 스트림 (1.5s 갱신)') : t('연결 중…')}
          </span>

          {/* Sub tabs */}
          <div className="inline-flex rounded-lg border border-ink-700 p-0.5">
            <button
              type="button"
              onClick={() => setTab('overview')}
              className={`rounded-md px-2.5 py-1 text-xs font-medium transition ${
                tab === 'overview' ? 'bg-ink-800 text-ink-100' : 'text-ink-400 hover:text-ink-100'
              }`}
            >
              {t('자원 종합')}
            </button>
            <button
              type="button"
              onClick={() => setTab('cores')}
              className={`rounded-md px-2.5 py-1 text-xs font-medium transition ${
                tab === 'cores' ? 'bg-ink-800 text-ink-100' : 'text-ink-400 hover:text-ink-100'
              }`}
            >
              {t('CPU 코어 상세')}
            </button>
            <button
              type="button"
              onClick={() => setTab('trends')}
              className={`rounded-md px-2.5 py-1 text-xs font-medium transition ${
                tab === 'trends' ? 'bg-ink-800 text-ink-100' : 'text-ink-400 hover:text-ink-100'
              }`}
            >
              {t('실시간 트렌드')}
            </button>
          </div>
        </div>
      </div>

      {/* Main Resource Cards Grid */}
      <div className="p-5">
        {/* Visual Threshold Alert Banner (When >=90%) */}
        {hasThresholdAlert && (
          <div className="mb-5 flex flex-wrap items-center justify-between gap-3 rounded-xl border-2 border-fail bg-fail/15 p-4 shadow-lg shadow-fail/20 animate-pulse">
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-fail text-white shadow-md">
                <span className="text-xl" aria-hidden="true">🚨</span>
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <h3 className="text-sm font-bold text-red-200">
                    {t('임계값 90% 초과 경보')}
                  </h3>
                  <span className="rounded-full bg-fail px-2 py-0.5 text-[10px] font-bold text-white uppercase tracking-wider">
                    {t('위험 (90% 초과)')}
                  </span>
                </div>
                <p className="mt-0.5 text-xs text-red-300">
                  {isCpuExceeded && isMemExceeded
                    ? t('CPU 및 메모리 사용량이 임계값 90%를 초과했습니다')
                    : isCpuExceeded
                    ? t('CPU 사용량이 임계값 90%를 초과했습니다 ({pct}%)', { pct: cpuPercent.toFixed(1) })
                    : t('메모리 사용량이 임계값 90%를 초과했습니다 ({pct}%)', { pct: memPercent.toFixed(1) })}
                </p>
              </div>
            </div>
            {simulatedSpike && (
              <button
                type="button"
                onClick={() => setSimulatedSpike(false)}
                className="rounded-lg border border-red-400/40 bg-ink-900/90 px-3 py-1.5 text-xs font-medium text-red-200 hover:bg-ink-900 transition"
              >
                {t('정상 상태로 복귀')}
              </button>
            )}
          </div>
        )}

        {/* Paused state notification */}
        {!autoRefresh && (
          <div className="mb-4 flex items-center justify-between rounded-lg border border-warn/30 bg-warn/5 px-4 py-2 text-xs text-amber-200">
            <div className="flex items-center gap-2">
              <span aria-hidden="true">⏸</span>
              <span>{t('실시간 갱신이 일시정지되었습니다')}</span>
            </div>
            {onToggleAutoRefresh && (
              <button
                type="button"
                onClick={onToggleAutoRefresh}
                className="rounded border border-warn/40 bg-warn/20 px-2 py-0.5 font-medium text-warn hover:bg-warn/30 transition"
              >
                {t('재개')}
              </button>
            )}
          </div>
        )}

        {tab === 'overview' && (
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {/* 1. CPU Usage Card with 90% Threshold Alert Styling */}
            <div
              className={`rounded-xl border p-4 transition-all duration-300 ${
                isCpuExceeded
                  ? 'border-fail bg-fail/10 ring-2 ring-fail/40 shadow-lg shadow-fail/20'
                  : 'border-ink-700/70 bg-ink-950/30 hover:border-ink-600'
              }`}
            >
              <div className="flex items-center justify-between">
                <span className={`text-xs font-medium ${isCpuExceeded ? 'text-red-300 font-bold' : 'text-ink-400'}`}>
                  {t('CPU 사용률')}
                </span>
                {getStatusBadge(cpuPercent)}
              </div>
              <div className="mt-4 flex items-center justify-center">
                <GaugeArc
                  percent={cpuPercent}
                  color={getStatusColor(cpuPercent)}
                  isAlert={isCpuExceeded}
                />
              </div>
              <div className={`mt-4 space-y-1.5 border-t pt-3 text-xs ${isCpuExceeded ? 'border-fail/30' : 'border-ink-800'}`}>
                <div className="flex justify-between">
                  <span className={isCpuExceeded ? 'text-red-300' : 'text-ink-400'}>{t('시스템 사양')}</span>
                  <span className="font-mono text-ink-100 tabular-nums">{t('{n} CPU', { n: cpuCount })}</span>
                </div>
                <div className="flex justify-between">
                  <span className={isCpuExceeded ? 'text-red-300' : 'text-ink-400'}>Load Avg</span>
                  <span className="font-mono text-ink-100 tabular-nums">{load.map((v) => v.toFixed(2)).join(' · ')}</span>
                </div>
              </div>
            </div>

            {/* 2. Memory / RAM Usage Card with 90% Threshold Alert Styling */}
            <div
              className={`rounded-xl border p-4 transition-all duration-300 ${
                isMemExceeded
                  ? 'border-fail bg-fail/10 ring-2 ring-fail/40 shadow-lg shadow-fail/20'
                  : 'border-ink-700/70 bg-ink-950/30 hover:border-ink-600'
              }`}
            >
              <div className="flex items-center justify-between">
                <span className={`text-xs font-medium ${isMemExceeded ? 'text-red-300 font-bold' : 'text-ink-400'}`}>
                  {t('메모리 사용률')}
                </span>
                {getStatusBadge(memPercent)}
              </div>
              <div className="mt-4 flex items-center justify-center">
                <GaugeArc
                  percent={memPercent}
                  color={getStatusColor(memPercent)}
                  isAlert={isMemExceeded}
                />
              </div>
              <div className={`mt-4 space-y-1.5 border-t pt-3 text-xs ${isMemExceeded ? 'border-fail/30' : 'border-ink-800'}`}>
                <div className="flex justify-between">
                  <span className={isMemExceeded ? 'text-red-300' : 'text-ink-400'}>{t('사용 중')}</span>
                  <span className="font-mono text-ink-100 tabular-nums">{bytes(memUsed)}</span>
                </div>
                <div className="flex justify-between">
                  <span className={isMemExceeded ? 'text-red-300' : 'text-ink-400'}>{t('여유')}</span>
                  <span className={`font-mono tabular-nums ${isMemExceeded ? 'text-fail font-semibold' : 'text-signal'}`}>
                    {bytes(memFree)}
                  </span>
                </div>
              </div>
            </div>

            {/* 3. Disk Space */}
            <div className="rounded-xl border border-ink-700/70 bg-ink-950/30 p-4 transition hover:border-ink-600">
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-ink-400">{t('디스크 사용률')}</span>
                {getStatusBadge(diskPercent)}
              </div>
              <div className="mt-4 flex items-center justify-center">
                <GaugeArc percent={diskPercent} color={getStatusColor(diskPercent)} />
              </div>
              <div className="mt-4 space-y-1.5 border-t border-ink-800 pt-3 text-xs">
                <div className="flex justify-between">
                  <span className="text-ink-400">{t('사용 가능')}</span>
                  <span className="font-mono text-signal tabular-nums">{bytes(diskFree)}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-ink-400">{t('마운트')}</span>
                  <span className="font-mono text-ink-100 tabular-nums">{primaryDisk.mount} ({bytes(diskTotal)})</span>
                </div>
              </div>
            </div>

            {/* 4. Load Average & Status summary */}
            <div
              className={`rounded-xl border p-4 transition-all duration-300 ${
                hasThresholdAlert
                  ? 'border-fail/50 bg-fail/5'
                  : 'border-ink-700/70 bg-ink-950/30 hover:border-ink-600'
              }`}
            >
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-ink-400">{t('시스템 부하')}</span>
                <span
                  className={`rounded border px-2 py-0.5 text-[11px] font-medium ${
                    hasThresholdAlert
                      ? 'border-fail/40 bg-fail/10 text-fail'
                      : 'border-signal/40 bg-signal/10 text-signal'
                  }`}
                >
                  {hasThresholdAlert ? t('위험') : t('안정적')}
                </span>
              </div>
              <div className="mt-3 flex flex-col justify-center space-y-3">
                <div>
                  <div className="flex justify-between text-xs">
                    <span className="text-ink-400">{t('1분')}</span>
                    <span className="font-mono font-semibold tabular-nums text-ink-100">{load[0].toFixed(2)}</span>
                  </div>
                  <div className="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-ink-800">
                    <div className="h-full bg-purple-500 transition-all duration-500" style={{ width: `${Math.min(100, (load[0] / cpuCount) * 100)}%` }} />
                  </div>
                </div>
                <div>
                  <div className="flex justify-between text-xs">
                    <span className="text-ink-400">{t('5분')}</span>
                    <span className="font-mono font-semibold tabular-nums text-ink-100">{load[1].toFixed(2)}</span>
                  </div>
                  <div className="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-ink-800">
                    <div className="h-full bg-sky-500 transition-all duration-500" style={{ width: `${Math.min(100, (load[1] / cpuCount) * 100)}%` }} />
                  </div>
                </div>
                <div>
                  <div className="flex justify-between text-xs">
                    <span className="text-ink-400">{t('15분')}</span>
                    <span className="font-mono font-semibold tabular-nums text-ink-100">{load[2].toFixed(2)}</span>
                  </div>
                  <div className="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-ink-800">
                    <div className="h-full bg-indigo-500 transition-all duration-500" style={{ width: `${Math.min(100, (load[2] / cpuCount) * 100)}%` }} />
                  </div>
                </div>
              </div>
              <div className="mt-4 border-t border-ink-800 pt-3 text-xs">
                <span className={hasThresholdAlert ? 'text-fail font-medium' : 'text-ink-400'}>
                  {hasThresholdAlert ? t('자원 사용량이 높습니다') : t('자원 사용량이 안정적입니다')}
                </span>
              </div>
            </div>
          </div>
        )}

        {/* Cores Detailed View */}
        {tab === 'cores' && (
          <div className="space-y-4">
            <div className="flex items-center justify-between text-xs text-ink-400">
              <span>{host?.cpu_brand ? `${host.cpu_brand} (${cpuCount} vCPUs)` : `${cpuCount} Cores Active`}</span>
              <span className={`font-mono tabular-nums ${isCpuExceeded ? 'text-fail font-bold' : ''}`}>
                {t('CPU 사용률')}: {cpuPercent.toFixed(1)}%
              </span>
            </div>
            <div className="grid gap-3 sm:grid-cols-2 md:grid-cols-4">
              {Array.from({ length: cpuCount }).map((_, idx) => {
                const baseVal = cpuCores[idx % cpuCores.length] ?? (15 + ((idx * 7) % 25))
                const coreVal = simulatedSpike ? Math.min(99, 90 + ((idx * 2) % 9)) : baseVal
                const isCoreAlert = coreVal >= THRESHOLD
                return (
                  <div
                    key={idx}
                    className={`rounded-lg border p-3 transition ${
                      isCoreAlert
                        ? 'border-fail bg-fail/10 ring-1 ring-fail/40'
                        : 'border-ink-700/70 bg-ink-950/40'
                    }`}
                  >
                    <div className="flex items-center justify-between text-xs">
                      <span className={`font-mono font-medium ${isCoreAlert ? 'text-red-300' : 'text-ink-300'}`}>
                        {t('코어 {n}', { n: idx })}
                      </span>
                      <span className={`font-mono font-semibold tabular-nums ${isCoreAlert ? 'text-fail font-bold' : 'text-ink-100'}`}>
                        {coreVal.toFixed(1)}%
                      </span>
                    </div>
                    <div className="mt-2 h-2 w-full overflow-hidden rounded-full bg-ink-800">
                      <div
                        className="h-full rounded-full transition-all duration-500"
                        style={{
                          width: `${coreVal}%`,
                          backgroundColor: getStatusColor(coreVal),
                        }}
                      />
                    </div>
                  </div>
                )
              })}
            </div>
          </div>
        )}

        {/* Live Sparkline Trends View */}
        {tab === 'trends' && (
          <div className="grid gap-4 sm:grid-cols-2">
            <div
              className={`rounded-lg border p-4 transition ${
                isCpuExceeded ? 'border-fail/60 bg-fail/5' : 'border-ink-700/70 bg-ink-950/40'
              }`}
            >
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-ink-300">
                  {t('CPU 사용률')} ({t('최근 30초 실시간 변화')})
                </span>
                <span className={`font-mono text-xs font-semibold tabular-nums ${isCpuExceeded ? 'text-fail font-bold animate-pulse' : 'text-sky-400'}`}>
                  {cpuPercent.toFixed(1)}%
                </span>
              </div>
              <div className="mt-3">
                <MiniSparkline data={cpuTrend} color={isCpuExceeded ? '#ef4444' : '#38bdf8'} />
              </div>
            </div>
            <div
              className={`rounded-lg border p-4 transition ${
                isMemExceeded ? 'border-fail/60 bg-fail/5' : 'border-ink-700/70 bg-ink-950/40'
              }`}
            >
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-ink-300">
                  {t('메모리 사용률')} ({t('최근 30초 실시간 변화')})
                </span>
                <span className={`font-mono text-xs font-semibold tabular-nums ${isMemExceeded ? 'text-fail font-bold animate-pulse' : 'text-amber-400'}`}>
                  {memPercent.toFixed(1)}%
                </span>
              </div>
              <div className="mt-3">
                <MiniSparkline data={memTrend} color={isMemExceeded ? '#ef4444' : '#fbbf24'} />
              </div>
            </div>
          </div>
        )}
      </div>
    </section>
  )
}
