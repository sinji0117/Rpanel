import { useEffect, useRef, useState } from 'react'
import { t } from '../i18n'

const STATUS = {
  running: { label: '실행 중', cls: 'text-signal bg-signal/10 border-signal/30', dot: 'bg-signal' },
  success: { label: '성공', cls: 'text-signal bg-signal/10 border-signal/30', dot: 'bg-signal' },
  deploying: { label: '진행 중', cls: 'text-sky-300 bg-sky-400/10 border-sky-400/30', dot: 'bg-sky-400 animate-pulse' },
  running_deploy: { label: '진행 중', cls: 'text-sky-300 bg-sky-400/10 border-sky-400/30', dot: 'bg-sky-400 animate-pulse' },
  queued: { label: '대기', cls: 'text-ink-300 bg-ink-700/40 border-ink-600', dot: 'bg-ink-400 animate-pulse' },
  failed: { label: '실패', cls: 'text-fail bg-fail/10 border-fail/30', dot: 'bg-fail' },
  cancelled: { label: '취소됨', cls: 'text-warn bg-warn/10 border-warn/30', dot: 'bg-warn' },
  stopped: { label: '중지됨', cls: 'text-warn bg-warn/10 border-warn/30', dot: 'bg-warn' },
  idle: { label: '미배포', cls: 'text-ink-300 bg-ink-700/40 border-ink-600', dot: 'bg-ink-400' },
}

/** status: 앱 상태 또는 배포 상태. deployment=true면 running을 '진행 중'으로 표시 */
export function StatusBadge({ status, deployment = false }) {
  const key = deployment && status === 'running' ? 'running_deploy' : status
  const s = STATUS[key] || { label: status, cls: 'text-ink-300 border-ink-600', dot: 'bg-ink-400' }
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs font-medium ${s.cls}`}>
      <span className={`h-1.5 w-1.5 rounded-full ${s.dot}`} />
      {t(s.label)}
    </span>
  )
}

export function KindBadge({ kind, source }) {
  const map = { rust: 'Rust', python: 'Python', static: source === 'files' ? 'HTML' : 'Static', proxy: 'Proxy' }
  return <span className="kbd uppercase">{map[kind] || kind}</span>
}

export function Spinner({ className = '' }) {
  return <span className={`inline-block h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent ${className}`} aria-label={t('로딩 중')} />
}

/** ? 아이콘 — 누르면 설명이 말풍선으로 열림 (바깥 클릭·Esc 로 닫힘) */
export function HelpTip({ children, label, align = 'right' }) {
  const [open, setOpen] = useState(false)
  const ref = useRef(null)
  useEffect(() => {
    if (!open) return
    const onDown = (e) => ref.current && !ref.current.contains(e.target) && setOpen(false)
    const onKey = (e) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])
  if (!children) return null
  const pos = align === 'center' ? 'left-1/2 -translate-x-1/2' : align === 'left' ? 'left-0' : 'right-0'
  return (
    <span ref={ref} className="relative inline-flex">
      <button
        type="button"
        className={`inline-flex h-6 w-6 items-center justify-center rounded-full border text-xs font-semibold transition ${open ? 'border-signal/60 text-signal' : 'border-ink-600 text-ink-400 hover:border-ink-400 hover:text-ink-100'}`}
        aria-label={label ?? t('도움말')}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        ?
      </button>
      {open && (
        <span
          role="tooltip"
          className={`absolute top-full z-30 mt-2 w-72 max-w-[80vw] rounded-lg border border-ink-700 bg-ink-900 p-3 text-left text-sm leading-relaxed font-normal text-ink-300 shadow-xl shadow-black/30 ${pos}`}
        >
          {children}
        </span>
      )}
    </span>
  )
}

export function PageHeader({ title, subtitle, help, actions, children }) {
  return (
    <div className="mb-6 flex flex-wrap items-start justify-between gap-4">
      <div className="min-w-0">
        <h1 className="truncate text-xl font-semibold tracking-tight">{title}</h1>
        {subtitle && <p className="mt-1 text-sm text-ink-400">{subtitle}</p>}
        {children}
      </div>
      {(actions || help) && (
        <div className="flex flex-wrap items-center gap-2">
          <HelpTip label={t('{title} 도움말', { title })}>{help}</HelpTip>
          {actions}
        </div>
      )}
    </div>
  )
}

/** 빈 목록. 설명은 페이지 위쪽 ? (PageHeader help) 에 둡니다 */
export function Empty({ title, children, action }) {
  return (
    <div className="card flex flex-col items-center px-6 py-14 text-center">
      <p className="font-medium">{title}</p>
      {children && <p className="mt-1.5 max-w-md text-sm text-ink-400">{children}</p>}
      {action && <div className="mt-5">{action}</div>}
    </div>
  )
}

export function ErrorBox({ error }) {
  if (!error) return null
  return (
    <div role="alert" className="rounded-lg border border-fail/40 bg-fail/10 px-4 py-3 text-sm text-red-200">
      {error.message || String(error)}
    </div>
  )
}

export function Modal({ open, title, onClose, children, wide = false }) {
  const ref = useRef(null)
  useEffect(() => {
    if (!open) return
    const onKey = (e) => e.key === 'Escape' && onClose?.()
    window.addEventListener('keydown', onKey)
    ref.current?.querySelector('input,select,textarea,button')?.focus()
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])
  if (!open) return null
  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-black/60 p-4 pt-[8vh] backdrop-blur-sm">
      <div ref={ref} role="dialog" aria-modal="true" aria-label={title} className={`card w-full ${wide ? 'max-w-2xl' : 'max-w-md'} shadow-2xl shadow-black/50`}>
        <div className="flex items-center justify-between border-b border-ink-700/70 px-5 py-3.5">
          <h2 className="font-semibold">{title}</h2>
          <button className="text-ink-400 hover:text-ink-100" onClick={onClose} aria-label={t('닫기')}>
            ✕
          </button>
        </div>
        <div className="p-5">{children}</div>
      </div>
    </div>
  )
}

/** 카드 제목 줄: 제목 + (? 도움말) + 버튼 */
export function CardHeader({ title, help, className = '', children }) {
  return (
    <div className={`flex flex-wrap items-center justify-between gap-3 ${className}`}>
      <h3 className="font-medium">{title}</h3>
      {(help || children) && (
        <div className="flex items-center gap-2">
          <HelpTip label={typeof title === 'string' ? t('{title} 도움말', { title }) : undefined}>{help}</HelpTip>
          {children}
        </div>
      )}
    </div>
  )
}

export function Section({ title, desc, actions, children }) {
  return (
    <section className="card p-5">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <h3 className="font-medium">{title}</h3>
        {(actions || desc) && (
          <div className="flex items-center gap-2">
            <HelpTip label={typeof title === 'string' ? t('{title} 도움말', { title }) : undefined}>{desc}</HelpTip>
            {actions}
          </div>
        )}
      </div>
      {children}
    </section>
  )
}

export function Field({ label, hint, children, htmlFor }) {
  return (
    <div>
      <label className="label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {hint && <p className="mt-1 text-xs text-ink-400">{hint}</p>}
    </div>
  )
}

export function CopyButton({ text, label = t('복사') }) {
  const [done, setDone] = useState(false)
  return (
    <button
      type="button"
      className="btn-ghost px-2.5 py-1 text-xs"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text)
          setDone(true)
          setTimeout(() => setDone(false), 1500)
        } catch {
          // http(비보안 컨텍스트)에서는 clipboard API가 막힐 수 있음
          window.prompt(t('복사하세요'), text)
        }
      }}
    >
      {done ? t('복사됨') : label}
    </button>
  )
}

export function Tabs({ tabs, active, onChange }) {
  return (
    <div role="tablist" className="mb-5 flex gap-1 overflow-x-auto border-b border-ink-700/70">
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={active === t.id}
          onClick={() => onChange(t.id)}
          className={`-mb-px border-b-2 px-3.5 py-2.5 text-sm whitespace-nowrap transition ${
            active === t.id ? 'border-signal text-ink-100' : 'border-transparent text-ink-400 hover:text-ink-100'
          }`}
        >
          {t.label}
        </button>
      ))}
    </div>
  )
}

export function Meter({ value, label, detail }) {
  const v = Math.round(value)
  const color = v >= 90 ? 'bg-fail' : v >= 70 ? 'bg-warn' : 'bg-signal'
  return (
    <div>
      <div className="mb-1.5 flex items-baseline justify-between text-xs">
        <span className="text-ink-300">{label}</span>
        <span className="font-mono text-ink-100">{detail ?? `${v}%`}</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-ink-800" role="meter" aria-label={label} aria-valuenow={v} aria-valuemin={0} aria-valuemax={100}>
        <div className={`h-full rounded-full ${color} transition-[width] duration-500`} style={{ width: `${v}%` }} />
      </div>
    </div>
  )
}
