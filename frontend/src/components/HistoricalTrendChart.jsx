import { useMemo, useState } from 'react'
import {
  ResponsiveContainer,
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ReferenceLine,
} from 'recharts'
import { get } from '../lib/api'
import { useFetch, useInterval } from '../lib/useFetch'
import { t } from '../i18n'

const RANGES = [
  { id: '30m', label: '30분' },
  { id: '1h', label: '1시간' },
  { id: '6h', label: '6시간' },
  { id: '24h', label: '24시간' },
]

function CustomTooltip({ active, payload, label }) {
  if (active && payload && payload.length) {
    const cpuVal = payload.find((p) => p.dataKey === 'cpu')?.value ?? 0
    const ramVal = payload.find((p) => p.dataKey === 'ram')?.value ?? 0
    return (
      <div className="rounded-lg border border-ink-700 bg-ink-900/95 p-3 shadow-xl backdrop-blur">
        <p className="font-mono text-xs text-ink-400 mb-2">{label}</p>
        <div className="space-y-1 text-xs">
          <div className="flex items-center gap-2">
            <span className="h-2 w-2 rounded-full bg-sky-400" />
            <span className="text-ink-300">CPU:</span>
            <span className={`font-mono font-bold tabular-nums ${cpuVal >= 90 ? 'text-fail' : 'text-sky-300'}`}>
              {cpuVal}%
            </span>
            {cpuVal >= 90 && <span className="rounded bg-fail/20 px-1 text-[10px] text-fail">CRITICAL</span>}
          </div>
          <div className="flex items-center gap-2">
            <span className="h-2 w-2 rounded-full bg-purple-400" />
            <span className="text-ink-300">{t('메모리')}:</span>
            <span className={`font-mono font-bold tabular-nums ${ramVal >= 90 ? 'text-fail' : 'text-purple-300'}`}>
              {ramVal}%
            </span>
            {ramVal >= 90 && <span className="rounded bg-fail/20 px-1 text-[10px] text-fail">CRITICAL</span>}
          </div>
        </div>
      </div>
    )
  }
  return null
}

export default function HistoricalTrendChart({ autoRefresh = true }) {
  const [range, setRange] = useState('1h') // default to last hour as requested

  const metricsFetch = useFetch(() => get(`/system/metrics?range=${range}`), [range])
  useInterval(() => autoRefresh && metricsFetch.reload(true), 30000)

  const points = metricsFetch.data?.points ?? []

  const chartData = useMemo(() => {
    return points.map((p) => {
      const d = new Date(p.t * 1000)
      const hours = String(d.getHours()).padStart(2, '0')
      const minutes = String(d.getMinutes()).padStart(2, '0')
      const timeStr = `${hours}:${minutes}`
      const memTotal = p.mt || 0
      const memUsed = p.m || 0
      const ramPercent = memTotal > 0 ? Number(((memUsed / memTotal) * 100).toFixed(1)) : 0
      const cpuPercent = Number((p.c ?? 0).toFixed(1))

      return {
        timestamp: p.t,
        time: timeStr,
        cpu: cpuPercent,
        ram: ramPercent,
      }
    })
  }, [points])

  // Statistics over the period
  const stats = useMemo(() => {
    if (!chartData.length) return { avgCpu: 0, maxCpu: 0, avgRam: 0, maxRam: 0 }
    const cpus = chartData.map((d) => d.cpu)
    const rams = chartData.map((d) => d.ram)
    const avgCpu = (cpus.reduce((a, b) => a + b, 0) / cpus.length).toFixed(1)
    const maxCpu = Math.max(...cpus).toFixed(1)
    const avgRam = (rams.reduce((a, b) => a + b, 0) / rams.length).toFixed(1)
    const maxRam = Math.max(...rams).toFixed(1)
    return { avgCpu, maxCpu, avgRam, maxRam }
  }, [chartData])

  return (
    <section className="card overflow-hidden" aria-label={t('자원 사용 추이')}>
      {/* Header */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink-700/70 bg-ink-950/40 px-5 py-3.5">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-sky-500/30 bg-sky-500/10 text-sky-400">
            <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
            </svg>
          </div>
          <div>
            <h2 className="text-sm font-semibold tracking-tight text-ink-100">
              {t('자원 사용 추이')}
            </h2>
            <p className="text-xs text-ink-400">
              {t('CPU 및 RAM 사용 추이')}
            </p>
          </div>
        </div>

        {/* Range Selector Controls */}
        <div className="flex items-center gap-2">
          <div className="inline-flex rounded-lg border border-ink-700 p-0.5">
            {RANGES.map((r) => (
              <button
                key={r.id}
                type="button"
                onClick={() => setRange(r.id)}
                className={`rounded-md px-2.5 py-1 text-xs font-medium transition cursor-pointer ${
                  range === r.id ? 'bg-ink-800 text-ink-100' : 'text-ink-400 hover:text-ink-100'
                }`}
              >
                {t(r.label)}
              </button>
            ))}
          </div>

          <button
            type="button"
            onClick={() => metricsFetch.reload()}
            className="btn-ghost px-2 py-1 text-xs cursor-pointer"
            title={t('새로고침')}
          >
            ↻
          </button>
        </div>
      </div>

      {/* KPI Stats Strip */}
      <div className="grid grid-cols-2 gap-4 border-b border-ink-800/80 bg-ink-950/20 px-5 py-3 sm:grid-cols-4">
        <div>
          <span className="text-[11px] text-ink-400">{t('기간 평균 CPU')}</span>
          <p className="mt-0.5 font-mono text-base font-bold text-sky-400 tabular-nums">
            {stats.avgCpu}%
          </p>
        </div>
        <div>
          <span className="text-[11px] text-ink-400">{t('기간 최고 CPU')}</span>
          <p className={`mt-0.5 font-mono text-base font-bold tabular-nums ${Number(stats.maxCpu) >= 90 ? 'text-fail' : 'text-sky-300'}`}>
            {stats.maxCpu}%
          </p>
        </div>
        <div>
          <span className="text-[11px] text-ink-400">{t('기간 평균 RAM')}</span>
          <p className="mt-0.5 font-mono text-base font-bold text-purple-400 tabular-nums">
            {stats.avgRam}%
          </p>
        </div>
        <div>
          <span className="text-[11px] text-ink-400">{t('기간 최고 RAM')}</span>
          <p className={`mt-0.5 font-mono text-base font-bold tabular-nums ${Number(stats.maxRam) >= 90 ? 'text-fail' : 'text-purple-300'}`}>
            {stats.maxRam}%
          </p>
        </div>
      </div>

      {/* Recharts Line Chart Container */}
      <div className="p-5">
        <div className="h-64 w-full">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={chartData} margin={{ top: 10, right: 10, left: -20, bottom: 0 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="var(--color-ink-700)" opacity={0.6} />
              <XAxis
                dataKey="time"
                stroke="var(--color-ink-400)"
                fontSize={11}
                tickLine={false}
                axisLine={{ stroke: "var(--color-ink-700)" }}
              />
              <YAxis
                domain={[0, 100]}
                unit="%"
                stroke="var(--color-ink-400)"
                fontSize={11}
                tickLine={false}
                axisLine={{ stroke: "var(--color-ink-700)" }}
              />
              <Tooltip content={<CustomTooltip />} />
              <Legend
                verticalAlign="top"
                align="right"
                wrapperStyle={{ paddingBottom: '12px', fontSize: '12px' }}
                formatter={(val) => (
                  <span className="text-xs text-ink-300">
                    {val === 'cpu' ? 'CPU (%)' : 'RAM (%)'}
                  </span>
                )}
              />
              {/* 90% threshold reference alert line */}
              <ReferenceLine
                y={90}
                stroke="#ef4444"
                strokeDasharray="4 4"
                label={{
                  value: '90% Alert Threshold',
                  fill: '#ef4444',
                  fontSize: 10,
                  position: 'insideTopRight',
                }}
              />
              {/* CPU Usage Trend Line */}
              <Line
                type="monotone"
                dataKey="cpu"
                name="cpu"
                stroke="#38bdf8"
                strokeWidth={2.5}
                dot={false}
                activeDot={{ r: 5, fill: '#38bdf8', stroke: '#0369a1', strokeWidth: 2 }}
                isAnimationActive={false}
              />
              {/* RAM Usage Trend Line */}
              <Line
                type="monotone"
                dataKey="ram"
                name="ram"
                stroke="#a78bfa"
                strokeWidth={2.5}
                dot={false}
                activeDot={{ r: 5, fill: '#a78bfa', stroke: '#6d28d9', strokeWidth: 2 }}
                isAnimationActive={false}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      </div>
    </section>
  )
}
