import { useEffect, useRef, useState } from 'react'
import { t } from '../i18n'

// 브라우저 기본 alert/confirm/prompt 대신 쓰는 팝업.
// 어디서나 `await confirmDialog('…')` / `await promptDialog('…')` 로 부르고, 화면에는 <DialogHost /> 하나만 둡니다.

let push = null
const queue = []

function open(opts) {
  return new Promise((resolve) => {
    const req = { ...opts, resolve }
    if (push) push(req)
    else queue.push(req)
  })
}

const DANGER = /삭제|지우|지울|지워|비울|해제|되돌릴|끄고|교체|다시 만들/

/**
 * 확인 팝업 → true / false
 * opts: { title, confirmText, cancelText, danger, requireText } — requireText 를 주면 그 글자를 입력해야 확인 버튼이 켜짐
 */
export function confirmDialog(message, opts = {}) {
  return open({ kind: 'confirm', message, danger: opts.danger ?? DANGER.test(message), ...opts })
}

/** 입력 팝업 → 입력값 (취소하면 null). opts: { title, defaultValue, placeholder, confirmText, readOnly, type } */
export function promptDialog(message, opts = {}) {
  // window.prompt(message, 기본값) 처럼 두 번째 인자로 문자열도 받음
  if (typeof opts === 'string') opts = { defaultValue: opts }
  return open({ kind: 'prompt', message, ...opts })
}

/** 알림 팝업 (확인 버튼 하나) */
export function alertDialog(message, opts = {}) {
  return open({ kind: 'alert', message, ...opts })
}

export function DialogHost() {
  const [items, setItems] = useState([])
  useEffect(() => {
    push = (req) => setItems((xs) => [...xs, req])
    if (queue.length) setItems((xs) => [...xs, ...queue.splice(0)])
    return () => {
      push = null
    }
  }, [])
  const current = items[0]
  if (!current) return null
  const close = (value) => {
    current.resolve(value)
    setItems((xs) => xs.slice(1))
  }
  return <DialogBox key={items.length + (current.message ?? '')} req={current} onClose={close} />
}

function DialogBox({ req, onClose }) {
  const [value, setValue] = useState(req.kind === 'prompt' ? (req.defaultValue ?? '') : '')
  const inputRef = useRef(null)
  const okRef = useRef(null)
  const needsText = req.kind === 'confirm' && req.requireText != null
  const canOk = !needsText || value === req.requireText
  const cancelValue = req.kind === 'prompt' ? null : req.kind === 'alert' ? undefined : false
  const ok = () => {
    if (!canOk) return
    onClose(req.kind === 'prompt' ? value : req.kind === 'alert' ? undefined : true)
  }

  useEffect(() => {
    const el = inputRef.current ?? okRef.current
    el?.focus()
    if (req.kind === 'prompt' && inputRef.current) inputRef.current.select()
    const onKey = (e) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        onClose(cancelValue)
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const title = req.title ?? (req.kind === 'prompt' ? t('입력') : req.kind === 'alert' ? t('알림') : req.danger ? t('정말 할까요?') : t('확인'))
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 p-4 pt-[18vh] backdrop-blur-sm"
      onMouseDown={(e) => e.target === e.currentTarget && onClose(cancelValue)}
    >
      <form
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        className="card w-full max-w-md shadow-2xl shadow-black/50"
        onSubmit={(e) => {
          e.preventDefault()
          ok()
        }}
      >
        <div className="flex items-center gap-2.5 border-b border-ink-700/70 px-5 py-3.5">
          {req.danger && <span className="h-2 w-2 rounded-full bg-fail" aria-hidden="true" />}
          <h2 className="font-semibold">{title}</h2>
        </div>
        <div className="space-y-3 p-5">
          {req.message && <p className="text-sm leading-relaxed whitespace-pre-line text-ink-300">{req.message}</p>}
          {req.kind === 'prompt' && (
            <input
              ref={inputRef}
              type={req.type ?? 'text'}
              aria-label={req.message || title}
              className="input font-mono"
              value={value}
              readOnly={req.readOnly}
              placeholder={req.placeholder}
              onChange={(e) => setValue(e.target.value)}
            />
          )}
          {needsText && (
            <div>
              <label className="label" htmlFor="dlg-require">
                {t('확인을 위해 {text} 를 입력하세요', { text: req.requireText })}
              </label>
              <input
                ref={inputRef}
                id="dlg-require"
                className="input font-mono"
                autoComplete="off"
                spellCheck={false}
                value={value}
                placeholder={req.requireText}
                onChange={(e) => setValue(e.target.value)}
              />
            </div>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-ink-700/70 px-5 py-3">
          {req.kind !== 'alert' && (
            <button type="button" className="btn-ghost" onClick={() => onClose(cancelValue)}>
              {req.cancelText ?? t('취소')}
            </button>
          )}
          <button ref={okRef} type="submit" className={req.danger ? 'btn-danger' : 'btn-primary'} disabled={!canOk}>
            {req.confirmText ?? (/삭제|지우|지울|지워/.test(req.message ?? '') ? t('삭제') : t('확인'))}
          </button>
        </div>
      </form>
    </div>
  )
}
