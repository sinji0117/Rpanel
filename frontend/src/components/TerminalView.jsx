import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import '@xterm/xterm/css/xterm.css'
import { openTerminal } from '../lib/api'
import { t } from '../i18n'

// 터미널은 패널 테마와 상관없이 늘 어두운 색 (명령 출력 색이 잘 보이도록)
const THEME = {
  background: '#0a0d12',
  foreground: '#e4e8ee',
  cursor: '#34d399',
  cursorAccent: '#0a0d12',
  selectionBackground: '#34d39955',
  black: '#1a2029',
  red: '#f87171',
  green: '#34d399',
  yellow: '#fbbf24',
  blue: '#60a5fa',
  magenta: '#c084fc',
  cyan: '#22d3ee',
  white: '#e4e8ee',
  brightBlack: '#7d889b',
  brightRed: '#fca5a5',
  brightGreen: '#6ee7b7',
  brightYellow: '#fde68a',
  brightBlue: '#93c5fd',
  brightMagenta: '#d8b4fe',
  brightCyan: '#67e8f9',
  brightWhite: '#ffffff',
}

const REASONS = {
  idle: '30분 동안 입력이 없어 닫았습니다',
  max: '최대 사용 시간이 지나 닫았습니다',
  revoked: '로그아웃되어 닫았습니다',
}

/**
 * 웹 터미널 한 개.
 * target: { kind: 'site', app_id } | { kind: 'root', code }
 * 연결이 끝나면 onEnded(message) — 다시 열기는 부모가 key 를 바꿔서
 */
export default function TerminalView({ target, onEnded, active = true, className = '' }) {
  const box = useRef(null)
  const termRef = useRef(null)
  const fitRef = useRef(null)
  const [state, setState] = useState('connecting') // connecting | open | closed
  const [error, setError] = useState(null)
  const targetRef = useRef(target)
  const endedRef = useRef(onEnded)
  endedRef.current = onEnded

  useEffect(() => {
    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "ui-monospace, 'JetBrains Mono', 'SFMono-Regular', Menlo, Consolas, 'D2Coding', monospace",
      fontSize: 13,
      lineHeight: 1.15,
      scrollback: 5000,
      theme: THEME,
      allowProposedApi: false,
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.loadAddon(new WebLinksAddon())
    term.open(box.current)
    termRef.current = term
    fitRef.current = fit
    try {
      fit.fit()
    } catch {
      /* 아직 크기가 0 */
    }

    let ws
    let disposed = false
    const enc = new TextEncoder()
    const sendSize = () => {
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }))
    }
    const finish = (msg) => {
      if (disposed) return
      setState('closed')
      term.write(`\r\n\x1b[90m── ${msg} ──\x1b[0m\r\n`)
      term.options.cursorBlink = false
      endedRef.current?.(msg)
    }

    openTerminal(targetRef.current, term.cols, term.rows)
      .then((socket) => {
        if (disposed) {
          socket.close()
          return
        }
        ws = socket
        let exitMsg = null
        ws.onopen = () => {
          setState('open')
          sendSize()
          term.focus()
        }
        ws.onmessage = (ev) => {
          if (typeof ev.data === 'string') {
            try {
              const m = JSON.parse(ev.data)
              if (m.type === 'exit') {
                exitMsg = m.reason ? t(REASONS[m.reason] ?? '터미널이 닫혔습니다') : t('터미널이 닫혔습니다')
              }
            } catch {
              /* 무시 */
            }
            return
          }
          term.write(new Uint8Array(ev.data))
        }
        ws.onclose = () => finish(exitMsg ?? t('연결이 끊겼습니다'))
        term.onData((d) => {
          if (ws.readyState === WebSocket.OPEN) ws.send(enc.encode(d))
        })
        term.onBinary((d) => {
          if (ws.readyState !== WebSocket.OPEN) return
          const b = new Uint8Array(d.length)
          for (let i = 0; i < d.length; i++) b[i] = d.charCodeAt(i) & 0xff
          ws.send(b)
        })
        term.onResize(sendSize)
      })
      .catch((e) => {
        if (disposed) return
        setError(e)
        finish(e.message || t('터미널을 열지 못했습니다'))
      })

    const ro = new ResizeObserver(() => {
      try {
        fit.fit()
      } catch {
        /* 숨겨진 상태 */
      }
    })
    ro.observe(box.current)

    return () => {
      disposed = true
      ro.disconnect()
      ws?.close()
      term.dispose()
    }
  }, [])

  // 탭을 다시 보이게 하면 크기 맞추고 포커스
  useEffect(() => {
    if (!active) return
    const id = requestAnimationFrame(() => {
      try {
        fitRef.current?.fit()
      } catch {
        /* 무시 */
      }
      termRef.current?.focus()
    })
    return () => cancelAnimationFrame(id)
  }, [active])

  return (
    <div className={`relative overflow-hidden rounded-lg border border-ink-700 bg-[#0a0d12] ${className}`} data-testid="terminal" data-state={state}>
      <div ref={box} className="h-full w-full p-2" />
      {state === 'connecting' && !error && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-sm text-[#7d889b]">{t('연결하는 중…')}</div>
      )}
    </div>
  )
}
