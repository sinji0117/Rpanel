import { useEffect, useRef, useState } from 'react'
import { locale } from '../i18n'

/** 시간 축 라벨: 짧은 기간은 HH:MM, 긴 기간(하루 넘게)은 MM/DD HH:MM */
export function timeLabel(t, spanSecs) {
  const d = new Date(t * 1000)
  const hm = d.toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit', hour12: false })
  if (spanSecs <= 26 * 3600) return hm
  return `${d.getMonth() + 1}/${d.getDate()} ${hm}`
}

/** 0 ~ max 를 보기 좋은 눈금으로 (load average 처럼 범위가 정해지지 않은 값) */
export function niceMax(v) {
  if (!(v > 0)) return 1
  const p = 10 ** Math.floor(Math.log10(v))
  for (const m of [1, 2, 2.5, 4, 5, 10]) if (v <= m * p) return m * p
  return 10 * p
}

/**
 * 여러 계열을 그리는 선 그래프 (SVG). 마우스를 올리면 그 시각의 값을 보여 줌.
 * series: [{ name, color, values: number[] }], times: 초 단위 시각 배열
 */
export default function LineChart({ series, times, max, format = (v) => v, height = 190, label, legend = false }) {
  const wrap = useRef(null)
  const [width, setWidth] = useState(480)
  const [hover, setHover] = useState(null)
  useEffect(() => {
    const el = wrap.current
    if (!el || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(([e]) => setWidth(Math.max(200, Math.round(e.contentRect.width))))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const n = times.length
  const all = series.flatMap((s) => s.values).filter((v) => Number.isFinite(v))
  const yMax = max ?? niceMax(Math.max(...all, 0) * 1.1)
  const pad = { l: 44, r: 12, t: 10, b: 24 }
  const w = width - pad.l - pad.r
  const h = height - pad.t - pad.b
  const span = n > 1 ? times[n - 1] - times[0] : 0
  const x = (i) => pad.l + (n > 1 ? ((times[i] - times[0]) / span) * w : w / 2)
  const y = (v) => pad.t + h - (Math.min(yMax, Math.max(0, v)) / yMax) * h
  const grid = [0, 0.25, 0.5, 0.75, 1]
  const tickCount = Math.min(7, n)
  const ticks = tickCount > 1 ? Array.from({ length: tickCount }, (_, k) => Math.round((k * (n - 1)) / (tickCount - 1))) : n ? [0] : []

  const onMove = (e) => {
    if (n < 1) return
    const rect = e.currentTarget.getBoundingClientRect()
    const px = ((e.clientX - rect.left) / rect.width) * width
    let best = 0
    for (let i = 1; i < n; i++) if (Math.abs(x(i) - px) < Math.abs(x(best) - px)) best = i
    setHover(best)
  }

  return (
    <div ref={wrap} className="relative w-full">
      <svg
        role="img"
        aria-label={label}
        viewBox={`0 0 ${width} ${height}`}
        width="100%"
        height={height}
        className="block select-none"
        onMouseMove={onMove}
        onMouseLeave={() => setHover(null)}
      >
        {grid.map((g) => (
          <g key={g}>
            <line x1={pad.l} x2={pad.l + w} y1={pad.t + h - g * h} y2={pad.t + h - g * h} className="stroke-ink-700" strokeWidth="1" />
            <text x={pad.l - 8} y={pad.t + h - g * h + 3.5} textAnchor="end" className="fill-ink-400 text-[10px] tabular-nums">
              {format(g * yMax)}
            </text>
          </g>
        ))}
        {ticks.map((i, k) => (
          <text
            key={i}
            x={x(i)}
            y={height - 6}
            textAnchor={ticks.length > 1 && k === ticks.length - 1 ? 'end' : k === 0 && ticks.length > 1 ? 'start' : 'middle'}
            className="fill-ink-400 text-[10px] tabular-nums"
          >
            {timeLabel(times[i], span)}
          </text>
        ))}
        {series.map((s) => {
          const pts = s.values.map((v, i) => (Number.isFinite(v) ? `${x(i).toFixed(1)},${y(v).toFixed(1)}` : null)).filter(Boolean)
          return (
            <g key={s.name} style={{ color: s.color }}>
              {pts.length > 1 && <polyline points={pts.join(' ')} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" />}
              {n <= 60 && s.values.map((v, i) => Number.isFinite(v) && <circle key={i} cx={x(i)} cy={y(v)} r="2.2" fill="currentColor" />)}
            </g>
          )
        })}
        {hover != null && <line x1={x(hover)} x2={x(hover)} y1={pad.t} y2={pad.t + h} className="stroke-ink-400" strokeDasharray="3 3" />}
      </svg>
      {hover != null && (
        <div
          className="pointer-events-none absolute top-1 z-10 rounded-md border border-ink-700 bg-ink-900/95 px-2.5 py-1.5 text-[11px] shadow-lg"
          style={x(hover) > width / 2 ? { right: width - x(hover) + 8 } : { left: x(hover) + 8 }}
        >
          <p className="mb-0.5 text-ink-400 tabular-nums">{timeLabel(times[hover], 99 * 3600)}</p>
          {series.map((s) => (
            <p key={s.name} className="flex items-center gap-1.5 whitespace-nowrap tabular-nums">
              <span className="h-2 w-2 rounded-full" style={{ background: s.color }} />
              {s.name} <b className="ml-auto pl-2">{Number.isFinite(s.values[hover]) ? format(s.values[hover]) : '–'}</b>
            </p>
          ))}
        </div>
      )}
      {legend && (
        <div className="mt-1 flex flex-wrap justify-center gap-x-4 gap-y-1 text-[11px] text-ink-300">
          {series.map((s) => (
            <span key={s.name} className="inline-flex items-center gap-1.5">
              <span className="h-0.5 w-3.5 rounded" style={{ background: s.color }} />
              {s.name}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}
