import { useEffect, useRef, useState } from 'react'

export function lineClass(text) {
  if (text.startsWith('▶')) return 'text-sky-300 font-semibold mt-2'
  if (text.startsWith('$ ')) return 'text-ink-300'
  if (text.startsWith('✔')) return 'text-signal font-semibold'
  if (text.startsWith('✘')) return 'text-fail font-semibold'
  if (/\b(error|failed|fatal|panicked)\b/i.test(text)) return 'text-red-300'
  if (/\bwarn(ing)?\b/i.test(text)) return 'text-amber-200'
  return 'text-ink-100'
}

/** lines: [{seq, text}] */
export default function LogViewer({ lines, height = 'h-[60vh]', emptyText = '로그가 없습니다' }) {
  const ref = useRef(null)
  const [follow, setFollow] = useState(true)

  useEffect(() => {
    if (follow && ref.current) ref.current.scrollTop = ref.current.scrollHeight
  }, [lines, follow])

  return (
    <div className="relative">
      <div
        ref={ref}
        data-testid="log-viewer"
        onScroll={(e) => {
          const el = e.currentTarget
          setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 40)
        }}
        className={`${height} overflow-auto rounded-lg border border-ink-700/70 bg-ink-950 p-4 font-mono text-[12.5px] leading-relaxed`}
      >
        {lines.length === 0 ? (
          <p className="text-ink-400">{emptyText}</p>
        ) : (
          lines.map((l) => (
            <div key={l.seq} className={`break-all whitespace-pre-wrap ${lineClass(l.text)}`}>
              {l.text || ' '}
            </div>
          ))
        )}
      </div>
      {!follow && lines.length > 0 && (
        <button
          className="btn-ghost absolute right-4 bottom-4 bg-ink-850 px-2.5 py-1 text-xs"
          onClick={() => {
            setFollow(true)
            if (ref.current) ref.current.scrollTop = ref.current.scrollHeight
          }}
        >
          ↓ 최신으로
        </button>
      )}
    </div>
  )
}
