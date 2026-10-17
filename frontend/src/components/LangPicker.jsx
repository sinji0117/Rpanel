// 언어 고르기 — 국기와 그 언어 이름으로 보여 줍니다.
// 국기는 이모지 대신 작은 SVG 로 그립니다 (Windows 는 국기 이모지를 글자로 보여 줌).
import { useEffect, useRef, useState } from 'react'
import { LANGS, setLang, t, useLang } from '../i18n'

const W = 21
const H = 14

/** 꼭짓점 5개 별 */
function star(cx, cy, r) {
  const pts = []
  for (let i = 0; i < 10; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / 5
    const rr = i % 2 ? r * 0.4 : r
    pts.push(`${(cx + rr * Math.cos(a)).toFixed(2)},${(cy + rr * Math.sin(a)).toFixed(2)}`)
  }
  return <polygon points={pts.join(' ')} />
}

const rows = (...c) => c.map((fill, i) => <rect key={i} y={(H / c.length) * i} width={W} height={H / c.length} fill={fill} />)
const cols = (...c) => c.map((fill, i) => <rect key={i} x={(W / c.length) * i} width={W / c.length} height={H} fill={fill} />)

const FLAGS = {
  de: () => rows('#000', '#dd0000', '#ffce00'),
  es: () => (
    <>
      <rect width={W} height={H} fill="#c60b1e" />
      <rect y={H / 4} width={W} height={H / 2} fill="#ffc400" />
    </>
  ),
  et: () => rows('#0072ce', '#000', '#fff'),
  pt: () => (
    <>
      <rect width={W} height={H} fill="#da291c" />
      <rect width={W * 0.4} height={H} fill="#046a38" />
      <circle cx={W * 0.4} cy={H / 2} r={3} fill="#ffe900" />
    </>
  ),
  fr: () => cols('#002654', '#fff', '#ce1126'),
  ga: () => cols('#169b62', '#fff', '#ff883e'),
  ja: () => (
    <>
      <rect width={W} height={H} fill="#fff" />
      <circle cx={W / 2} cy={H / 2} r={4.2} fill="#bc002d" />
    </>
  ),
  it: () => cols('#009246', '#fff', '#ce2b37'),
  nl: () => rows('#ae1c28', '#fff', '#21468b'),
  pl: () => rows('#fff', '#dc143c'),
  ru: () => rows('#fff', '#0039a6', '#d52b1e'),
  sk: () => (
    <>
      {rows('#fff', '#0b4ea2', '#ee1c25')}
      <path d="M4 3.5h5v4.5c0 2-1.3 3-2.5 3.7C5.3 11 4 10 4 8z" fill="#ee1c25" stroke="#fff" strokeWidth="0.6" />
      <path d="M6.5 5v5M5.3 6.4h2.4M5 7.8h3" stroke="#fff" strokeWidth="0.7" />
    </>
  ),
  cs: () => (
    <>
      {rows('#fff', '#d7141a')}
      <polygon points={`0,0 ${W / 2},${H / 2} 0,${H}`} fill="#11457e" />
    </>
  ),
  vi: () => (
    <>
      <rect width={W} height={H} fill="#da251d" />
      <g fill="#ffcd00">{star(W / 2, H / 2 + 0.3, 4.3)}</g>
    </>
  ),
  zh: () => (
    <>
      <rect width={W} height={H} fill="#ee1c25" />
      <g fill="#ffff00">
        {star(3.6, 3.6, 2.6)}
        {star(7.4, 1.5, 0.8)}
        {star(8.8, 3, 0.8)}
        {star(8.8, 5.1, 0.8)}
        {star(7.4, 6.5, 0.8)}
      </g>
    </>
  ),
  ko: () => (
    <>
      <rect width={W} height={H} fill="#fff" stroke="#d6d6d6" strokeWidth="0.5" />
      <path d={`M${W / 2 - 3.4} ${H / 2}a3.4 3.4 0 0 1 6.8 0z`} fill="#cd2e3a" />
      <path d={`M${W / 2 - 3.4} ${H / 2}a3.4 3.4 0 0 0 6.8 0z`} fill="#0047a0" />
      <g stroke="#000" strokeWidth="0.9">
        <path d="M3 3.2l1.6-1.4M3.6 3.9l1.6-1.4" />
        <path d="M16.4 1.8l1.6 1.4M15.8 2.5l1.6 1.4" />
        <path d="M3 10.8l1.6 1.4M3.6 10.1l1.6 1.4" />
        <path d="M16.4 12.2l1.6-1.4M15.8 11.5l1.6-1.4" />
      </g>
    </>
  ),
  bg: () => rows('#fff', '#00966e', '#d62612'),
  id: () => rows('#ce1126', '#fff'),
  tr: () => (
    <>
      <rect width={W} height={H} fill="#e30a17" />
      <circle cx={7.6} cy={H / 2} r={3.6} fill="#fff" />
      <circle cx={8.6} cy={H / 2} r={2.9} fill="#e30a17" />
      <g fill="#fff">{star(12.6, H / 2, 1.6)}</g>
    </>
  ),
  hu: () => rows('#ce2939', '#fff', '#477050'),
  no: () => (
    <>
      <rect width={W} height={H} fill="#ba0c2f" />
      <path d={`M6.5 0v${H}M0 ${H / 2}h${W}`} stroke="#fff" strokeWidth="3.6" />
      <path d={`M6.5 0v${H}M0 ${H / 2}h${W}`} stroke="#00205b" strokeWidth="1.8" />
    </>
  ),
}

/** 언어 국기 (영어는 지구본) */
export function Flag({ id, className = '' }) {
  if (id === 'en' || !FLAGS[id]) {
    return (
      <svg viewBox="0 0 24 24" width={W} height={H} className={`shrink-0 text-ink-300 ${className}`} fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true">
        <circle cx="12" cy="12" r="9" />
        <path d="M3 12h18M12 3c2.5 2.6 3.7 5.6 3.7 9s-1.2 6.4-3.7 9c-2.5-2.6-3.7-5.6-3.7-9S9.5 5.6 12 3z" />
      </svg>
    )
  }
  return (
    <svg viewBox={`0 0 ${W} ${H}`} width={W} height={H} className={`shrink-0 overflow-hidden rounded-[2px] ${className}`} aria-hidden="true">
      {FLAGS[id]()}
    </svg>
  )
}

/** 작은 언어 버튼 → 위(또는 아래)로 펼쳐지는 목록 */
export function LangPicker({ up = true, right = false, className = '' }) {
  const lang = useLang()
  const [open, setOpen] = useState(false)
  const box = useRef(null)
  const listRef = useRef(null)
  const cur = LANGS.find((l) => l.id === lang) ?? LANGS[0]

  useEffect(() => {
    if (!open) return
    const onDown = (e) => box.current && !box.current.contains(e.target) && setOpen(false)
    const onKey = (e) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    // 지금 언어가 보이도록
    listRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' })
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  return (
    <div ref={box} className={`relative ${className}`}>
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`${t('언어')}: ${cur.label}`}
        data-testid="lang-picker"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-900 px-2 py-0.5 text-xs text-ink-300 hover:border-ink-600 hover:text-ink-100"
      >
        <Flag id={cur.id} />
        <span className="max-w-28 truncate">{cur.label}</span>
        <span aria-hidden="true" className="text-[10px] text-ink-500">
          ▾
        </span>
      </button>
      {open && (
        <ul
          ref={listRef}
          role="listbox"
          aria-label={t('언어')}
          className={`absolute ${right ? 'right-0' : 'left-0'} z-50 max-h-[70vh] w-56 overflow-y-auto rounded-lg border border-ink-700 bg-ink-900 py-1 shadow-xl ${up ? 'bottom-full mb-1' : 'top-full mt-1'}`}
        >
          {LANGS.map((l) => (
            <li key={l.id}>
              <button
                type="button"
                role="option"
                aria-selected={l.id === lang}
                lang={l.bcp}
                onClick={() => {
                  setLang(l.id)
                  setOpen(false)
                }}
                className={`flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-sm ${l.id === lang ? 'bg-signal/10 text-signal' : 'text-ink-200 hover:bg-ink-800'}`}
              >
                <Flag id={l.id} />
                <span className="truncate">{l.label}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/** 설정 화면용: 국기와 이름이 있는 칸들 */
export function LangGrid() {
  const lang = useLang()
  return (
    <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-3" role="radiogroup" aria-label={t('언어')}>
      {LANGS.map((l) => (
        <button
          key={l.id}
          type="button"
          role="radio"
          aria-checked={lang === l.id}
          lang={l.bcp}
          onClick={() => setLang(l.id)}
          className={`flex items-center gap-2 rounded-lg border px-2.5 py-1.5 text-left text-sm transition ${lang === l.id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
        >
          <Flag id={l.id} />
          <span className="truncate">{l.label}</span>
        </button>
      ))}
    </div>
  )
}
