import { useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { get } from '../lib/api'
import { useFetch, useInterval } from '../lib/useFetch'
import { t } from '../i18n'

const LEVELS = [
  { id: 'all', label: '전체' },
  { id: 'error', label: '오류', color: 'text-fail' },
  { id: 'warn', label: '경고', color: 'text-warn' },
  { id: 'info', label: '정보', color: 'text-sky-400' },
]

const SOURCES = [
  { id: 'all', label: '전체 서비스' },
  { id: 'rpanel', label: 'Rpanel' },
  { id: 'nginx', label: 'Nginx' },
  { id: 'mariadb', label: 'MariaDB' },
  { id: 'redis', label: 'Redis' },
  { id: 'system', label: 'System' },
  { id: 'cron', label: 'Cron' },
]

const TIMEFRAMES = [
  { id: 'all', label: '전체 기간', ms: null },
  { id: '5m', label: '최근 5분', ms: 5 * 60 * 1000 },
  { id: '15m', label: '최근 15분', ms: 15 * 60 * 1000 },
  { id: '1h', label: '최근 1시간', ms: 60 * 60 * 1000 },
  { id: '24h', label: '최근 24시간', ms: 24 * 60 * 60 * 1000 },
]

function HighlightText({ text, query }) {
  if (!query || !query.trim()) return <span>{text}</span>
  const q = query.trim()
  const regex = new RegExp(`(${q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'gi')
  const parts = text.split(regex)
  return (
    <span>
      {parts.map((part, i) =>
        regex.test(part) ? (
          <mark key={i} className="rounded bg-amber-400/30 px-0.5 font-bold text-amber-200">
            {part}
          </mark>
        ) : (
          part
        )
      )}
    </span>
  )
}

export default function SystemLogViewer({ autoRefresh = true, height = 'h-72' }) {
  const [level, setLevel] = useState('all')
  const [source, setSource] = useState('all')
  const [timeframe, setTimeframe] = useState('all')
  const [search, setSearch] = useState('')
  const [follow, setFollow] = useState(true)
  const [copied, setCopied] = useState(false)
  const logContainerRef = useRef(null)

  // Fetch real-time system events
  const eventsFetch = useFetch(() => get('/system/events?limit=150'), [])

  // Auto-refresh interval (every 4 seconds when autoRefresh is active)
  useInterval(() => autoRefresh && eventsFetch.reload(true), 4000)

  const events = eventsFetch.data?.events ?? []
  const summary = eventsFetch.data?.summary ?? { total: 0, errors: 0, warnings: 0, info: 0 }

  // Client-side filtering by keyword, error level, source, and timeframe
  const filteredEvents = useMemo(() => {
    const q = search.trim().toLowerCase()
    const now = Date.now()
    const tfObj = TIMEFRAMES.find((tf) => tf.id === timeframe)
    const cutoff = tfObj?.ms ? now - tfObj.ms : null

    return events.filter((e) => {
      // Level filter
      if (level !== 'all' && e.level !== level) return false
      // Source filter
      if (source !== 'all' && e.source !== source) return false
      // Timeframe filter
      if (cutoff) {
        const time = new Date(e.timestamp).getTime()
        if (time < cutoff) return false
      }
      // Keyword search
      if (
        q &&
        !e.message.toLowerCase().includes(q) &&
        !e.source.toLowerCase().includes(q) &&
        !e.level.toLowerCase().includes(q)
      ) {
        return false
      }
      return true
    })
  }, [events, level, source, timeframe, search])

  const hasActiveFilters = level !== 'all' || source !== 'all' || timeframe !== 'all' || search.trim() !== ''

  const handleResetFilters = () => {
    setLevel('all')
    setSource('all')
    setTimeframe('all')
    setSearch('')
  }

  // Auto-scroll to latest
  useEffect(() => {
    if (follow && logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight
    }
  }, [filteredEvents, follow])

  const handleCopy = async () => {
    try {
      const text = filteredEvents
        .map((e) => `[${e.timestamp}] [${e.level.toUpperCase()}] [${e.source}] ${e.message}`)
        .join('\n')
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      /* ignore */
    }
  }

  const formatTime = (iso) => {
    try {
      const d = new Date(iso)
      return d.toTimeString().split(' ')[0]
    } catch {
      return iso
    }
  }

  const getLevelStyle = (lvl) => {
    switch (lvl) {
      case 'error':
        return 'border-fail/40 bg-fail/10 text-fail'
      case 'warn':
        return 'border-warn/40 bg-warn/10 text-warn'
      default:
        return 'border-sky-500/30 bg-sky-500/10 text-sky-300'
    }
  }

  return (
    <section className="card overflow-hidden" aria-label={t('서버 시스템 이벤트 및 오류 로그')}>
      {/* Header */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink-700/70 bg-ink-950/40 px-5 py-3.5">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-sky-500/30 bg-sky-500/10 text-sky-400">
            <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
              <polyline points="14 2 14 8 20 8" />
              <line x1="16" y1="13" x2="8" y2="13" />
              <line x1="16" y1="17" x2="8" y2="17" />
              <polyline points="10 9 9 9 8 9" />
            </svg>
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-semibold tracking-tight text-ink-100">
                {t('서버 시스템 이벤트 및 오류 로그')}
              </h2>
              <span
                className={`inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px] font-medium tabular-nums ${
                  autoRefresh
                    ? 'border-signal/30 bg-signal/10 text-signal'
                    : 'border-warn/40 bg-warn/10 text-warn'
                }`}
              >
                <span
                  className={`h-1.5 w-1.5 rounded-full ${
                    autoRefresh ? 'bg-signal animate-pulse' : 'bg-warn'
                  }`}
                />
                {autoRefresh ? t('실시간 수신 중') : t('일시정지됨')}
              </span>
            </div>
            <p className="text-xs text-ink-400">
              {summary.errors > 0 ? (
                <span className="font-medium text-fail">{t('오류 {n}건', { n: summary.errors })}</span>
              ) : (
                <span className="text-signal">{t('정상')}</span>
              )}
              <span className="mx-1.5 text-ink-600">·</span>
              {summary.warnings > 0 && (
                <>
                  <span className="font-medium text-warn">{t('경고 {n}건', { n: summary.warnings })}</span>
                  <span className="mx-1.5 text-ink-600">·</span>
                </>
              )}
              <span>{t('{n}개', { n: summary.total })} {t('실시간 이벤트')}</span>
            </p>
          </div>
        </div>

        {/* Action Buttons */}
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={handleCopy}
            className="rounded-lg border border-ink-700 bg-ink-900 px-2.5 py-1 text-xs text-ink-300 transition hover:border-ink-600 hover:text-ink-100 cursor-pointer"
          >
            {copied ? t('복사 완료') : t('로그 복사')}
          </button>

          <button
            type="button"
            onClick={() => eventsFetch.reload()}
            className="btn-ghost px-2 py-1 text-xs cursor-pointer"
            title={t('새로고침')}
          >
            ↻
          </button>

          <Link to="/logs?tab=system" className="text-xs text-signal hover:underline">
            {t('전체 로그 보기 →')}
          </Link>
        </div>
      </div>

      {/* Advanced Search & Filter Toolbar */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink-700/70 bg-ink-950/25 px-5 py-2.5">
        {/* Left: Severity Level Filter Chips */}
        <div className="flex flex-wrap items-center gap-1.5">
          {LEVELS.map((lvl) => (
            <button
              key={lvl.id}
              type="button"
              aria-pressed={level === lvl.id}
              onClick={() => setLevel(lvl.id)}
              className={`rounded-md px-2.5 py-1 text-xs font-medium transition cursor-pointer ${
                level === lvl.id
                  ? 'bg-ink-800 text-ink-100 shadow-sm border border-ink-600'
                  : 'text-ink-400 hover:bg-ink-850 hover:text-ink-200 border border-transparent'
              } ${lvl.color ? lvl.color : ''}`}
            >
              {t(lvl.label)}
              {lvl.id === 'error' && summary.errors > 0 && (
                <span className="ml-1.5 rounded-full bg-fail/20 px-1.5 py-0.2 text-[10px] font-mono text-fail font-bold">
                  {summary.errors}
                </span>
              )}
              {lvl.id === 'warn' && summary.warnings > 0 && (
                <span className="ml-1.5 rounded-full bg-warn/20 px-1.5 py-0.2 text-[10px] font-mono text-warn font-bold">
                  {summary.warnings}
                </span>
              )}
            </button>
          ))}

          {/* Timeframe Selector Dropdown */}
          <div className="ml-1 flex items-center gap-1">
            <span className="text-[11px] text-ink-500">⏱</span>
            <select
              value={timeframe}
              onChange={(e) => setTimeframe(e.target.value)}
              className="rounded-md border border-ink-700 bg-ink-900 px-2 py-1 text-xs text-ink-300 focus:border-signal focus:outline-none"
              aria-label={t('기간')}
            >
              {TIMEFRAMES.map((tf) => (
                <option key={tf.id} value={tf.id}>
                  {t(tf.label)}
                </option>
              ))}
            </select>
          </div>
        </div>

        {/* Right: Source Service Filter, Keyword Search Input & Reset Button */}
        <div className="flex flex-wrap items-center gap-2">
          {/* Service Source Filter */}
          <select
            value={source}
            onChange={(e) => setSource(e.target.value)}
            className="rounded-md border border-ink-700 bg-ink-900 px-2 py-1 text-xs text-ink-300 focus:border-signal focus:outline-none"
            aria-label={t('서비스')}
          >
            {SOURCES.map((s) => (
              <option key={s.id} value={s.id}>
                {t(s.label)}
              </option>
            ))}
          </select>

          {/* Keyword Search Input with Clear Button */}
          <div className="relative">
            <span className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-xs text-ink-500">
              🔍
            </span>
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t('검색…')}
              aria-label={t('이벤트 검색')}
              className="w-40 rounded-md border border-ink-700 bg-ink-900 py-1 pl-7 pr-7 text-xs text-ink-100 placeholder:text-ink-500 focus:border-signal focus:outline-none sm:w-56"
            />
            {search && (
              <button
                type="button"
                onClick={() => setSearch('')}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-ink-400 hover:text-ink-100"
                title={t('초기화')}
              >
                ✕
              </button>
            )}
          </div>

          {/* Active Filter Results & Reset Button */}
          {hasActiveFilters && (
            <div className="flex items-center gap-1.5">
              <span className="text-[11px] font-mono text-ink-400 tabular-nums">
                {t('{n}개 결과 일치', { n: filteredEvents.length })}
              </span>
              <button
                type="button"
                onClick={handleResetFilters}
                className="rounded border border-ink-700 bg-ink-850 px-2 py-0.5 text-[11px] text-ink-300 hover:text-ink-100 hover:border-ink-600 transition cursor-pointer"
                title={t('필터 초기화')}
              >
                {t('필터 초기화')}
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Terminal Log Console */}
      <div className="relative">
        <div
          ref={logContainerRef}
          onScroll={(e) => {
            const el = e.currentTarget
            setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 30)
          }}
          className={`${height} overflow-y-auto overflow-x-hidden bg-ink-950 p-4 font-mono text-[12px] leading-relaxed`}
        >
          {filteredEvents.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-center text-ink-500 py-10">
              <span className="text-2xl mb-1" aria-hidden="true">🔎</span>
              <p className="font-medium text-ink-400">{t('일치하는 이벤트가 없습니다')}</p>
              <p className="mt-1 text-[11px] text-ink-500">{t('다른 검색어나 필터를 선택해 보세요')}</p>
              {hasActiveFilters && (
                <button
                  type="button"
                  onClick={handleResetFilters}
                  className="mt-3 rounded border border-ink-700 bg-ink-900 px-3 py-1 text-xs text-signal hover:bg-ink-850 transition"
                >
                  {t('필터 초기화')}
                </button>
              )}
            </div>
          ) : (
            <div className="space-y-1.5">
              {filteredEvents.map((evt) => (
                <div
                  key={evt.id}
                  className={`group flex items-start gap-2.5 rounded px-2 py-1 transition hover:bg-ink-900/60 ${
                    evt.level === 'error'
                      ? 'bg-fail/5 text-red-200'
                      : evt.level === 'warn'
                      ? 'bg-warn/5 text-amber-200'
                      : 'text-ink-200'
                  }`}
                >
                  {/* Timestamp */}
                  <span className="shrink-0 select-none text-[11px] text-ink-500 tabular-nums">
                    {formatTime(evt.timestamp)}
                  </span>

                  {/* Level Tag */}
                  <span
                    className={`inline-block shrink-0 rounded border px-1.5 py-0.2 text-[10px] font-semibold tracking-wider uppercase tabular-nums ${getLevelStyle(
                      evt.level
                    )}`}
                  >
                    {evt.level}
                  </span>

                  {/* Source Service */}
                  <span className="shrink-0 select-none text-xs text-ink-400">
                    [{evt.source}]
                  </span>

                  {/* Message with keyword highlighting */}
                  <span className="break-all font-mono text-xs">
                    <HighlightText text={evt.message} query={search} />
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Scroll to Bottom Floating Button */}
        {!follow && filteredEvents.length > 0 && (
          <button
            type="button"
            onClick={() => {
              setFollow(true)
              if (logContainerRef.current) {
                logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight
              }
            }}
            className="absolute right-4 bottom-4 rounded-full border border-ink-600 bg-ink-850 px-3 py-1 text-xs font-medium text-ink-100 shadow-lg hover:bg-ink-800 transition cursor-pointer"
          >
            ↓ {t('최신으로')}
          </button>
        )}
      </div>
    </section>
  )
}
