/** 0~100 값의 미니 영역 차트 */
export default function Sparkline({ values, height = 40, className = 'text-signal' }) {
  const w = 120
  const n = values.length
  if (n < 2) return <svg viewBox={`0 0 ${w} ${height}`} className={`h-full min-h-10 w-full ${className}`} aria-hidden="true" />
  const step = w / (n - 1)
  const y = (v) => height - (Math.min(100, Math.max(0, v)) / 100) * (height - 2) - 1
  const pts = values.map((v, i) => `${(i * step).toFixed(1)},${y(v).toFixed(1)}`)
  return (
    <svg viewBox={`0 0 ${w} ${height}`} preserveAspectRatio="none" className={`h-full min-h-10 w-full ${className}`} aria-hidden="true">
      <polygon points={`0,${height} ${pts.join(' ')} ${w},${height}`} fill="currentColor" opacity="0.12" />
      <polyline points={pts.join(' ')} fill="none" stroke="currentColor" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}
