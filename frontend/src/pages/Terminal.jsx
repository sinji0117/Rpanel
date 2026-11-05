import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { CopyButton, ErrorBox, Loading, PageHeader } from '../components/ui'
import { get } from '../lib/api'
import { useFetch } from '../lib/useFetch'
import { t } from '../i18n'

const TerminalView = lazy(() => import('../components/TerminalView'))

let seq = 0

/** 열린 터미널 묶음 (탭) — 사이트 화면의 터미널 탭에서도 씀 */
export function TerminalSessions({ sessions, setSessions, activeKey, setActiveKey, height = 'h-[62vh]' }) {
  const [full, setFull] = useState(false)
  useEffect(() => {
    if (!full) return
    const onKey = (e) => e.key === 'Escape' && e.shiftKey && setFull(false)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [full])
  if (!sessions.length) return null
  const close = (key) => {
    const rest = sessions.filter((s) => s.key !== key)
    setSessions(rest)
    if (activeKey === key) setActiveKey(rest[rest.length - 1]?.key ?? null)
  }
  const reopen = (key) => setSessions(sessions.map((s) => (s.key === key ? { ...s, key: ++seq, ended: null } : s)))
  const cur = sessions.find((s) => s.key === activeKey) ?? sessions[sessions.length - 1]
  return (
    <div className={full ? 'fixed inset-0 z-50 flex flex-col bg-ink-950 p-3' : 'card p-3'} data-testid="terminal-sessions">
      <div className="mb-2 flex flex-wrap items-center gap-1.5">
        <div role="tablist" className="flex min-w-0 flex-1 flex-wrap gap-1">
          {sessions.map((s) => (
            <span
              key={s.key}
              className={`inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-xs ${s.key === cur.key ? 'border-ink-600 bg-ink-800 text-ink-100' : 'border-ink-700 text-ink-400 hover:text-ink-100'}`}
            >
              <button type="button" role="tab" aria-selected={s.key === cur.key} onClick={() => setActiveKey(s.key)} className="inline-flex items-center gap-1.5">
                <span className={`h-1.5 w-1.5 rounded-full ${s.ended ? 'bg-ink-600' : s.root ? 'bg-fail' : 'bg-signal'}`} aria-hidden="true" />
                <span className="font-mono">{s.title}</span>
              </button>
              <button type="button" onClick={() => close(s.key)} aria-label={t('{name} 닫기', { name: s.title })} className="text-ink-400 hover:text-ink-100">
                ✕
              </button>
            </span>
          ))}
        </div>
        {cur.ended && !cur.root && (
          <button type="button" className="btn-ghost px-2.5 py-1 text-xs" onClick={() => reopen(cur.key)}>
            {t('다시 연결')}
          </button>
        )}
        <button type="button" className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setFull((f) => !f)} aria-pressed={full}>
          {full ? t('작게 보기') : t('전체 화면')}
        </button>
      </div>
      {cur.root && !cur.ended && (
        <p className="mb-2 rounded-md border border-fail/30 bg-fail/10 px-3 py-1.5 text-xs text-red-200">
          {t('root 권한입니다. 명령 하나로 서버 전체가 바뀌거나 지워질 수 있으니 확인하고 실행하세요.')}
        </p>
      )}
      <Suspense fallback={<Loading compact />}>
        {sessions.map((s) => (
          <div key={s.key} hidden={s.key !== cur.key} className={full ? 'min-h-0 flex-1' : height}>
            <TerminalView
              target={s.target}
              active={s.key === cur.key}
              className="h-full"
              onEnded={(msg) => setSessions((list) => list.map((x) => (x.key === s.key ? { ...x, ended: msg } : x)))}
            />
          </div>
        ))}
      </Suspense>
      {full && <p className="mt-1.5 text-right text-[11px] text-ink-400">{t('Shift + Esc 로 작게 보기')}</p>}
    </div>
  )
}

export function newSession(title, target, root = false) {
  return { key: ++seq, title, target, root, ended: null }
}

export default function TerminalPage() {
  const info = useFetch(() => get('/terminal'), [])
  const [params, setParams] = useSearchParams()
  const [sessions, setSessions] = useState([])
  const [activeKey, setActiveKey] = useState(null)
  const [siteId, setSiteId] = useState('')
  const [code, setCode] = useState('')
  const auto = useRef(false)
  const d = info.data

  const open = (s) => {
    setSessions((list) => [...list, s])
    setActiveKey(s.key)
  }
  const openSite = (id) => {
    const site = d?.sites.find((x) => String(x.id) === String(id))
    if (site) open(newSession(site.user, { kind: 'site', app_id: site.id }))
  }

  // /terminal?site=<id> → 바로 열기
  useEffect(() => {
    if (!d || auto.current) return
    auto.current = true
    const want = params.get('site')
    if (want && d.available) {
      setSiteId(want)
      openSite(want)
      params.delete('site')
      setParams(params, { replace: true })
    } else if (d.sites[0]) {
      setSiteId(String(d.sites[0].id))
    }
  }, [d])

  const help = t('사이트 터미널은 그 사이트의 리눅스 계정으로 열려 SSH 로 접속한 것과 같습니다. 서버 터미널(root)은 서버 전체를 다룰 수 있어 기본으로 꺼져 있고, 서버에서만 켤 수 있습니다. 여는 기록은 로그 › 감사 기록에 남습니다.')

  return (
    <>
      <PageHeader title={t('터미널')} subtitle={t('브라우저에서 바로 쓰는 서버 셸')} help={help} />
      <ErrorBox error={info.error} />
      {info.loading && <Loading />}
      {d && !d.available && (
        <p className="mb-4 rounded-lg border border-warn/40 bg-warn/10 px-4 py-2.5 text-sm text-amber-200">{t('둘러보기 모드에서는 터미널을 쓸 수 없습니다')}</p>
      )}
      {d && d.available && (
        <div className="mb-4 grid gap-4 lg:grid-cols-2">
          <section className="card p-5" aria-label={t('사이트 터미널')}>
            <h2 className="font-medium">{t('사이트 터미널')}</h2>
            <p className="mt-1 text-xs text-ink-400">{t('사이트 계정 권한으로, 사이트 폴더에서 시작합니다.')}</p>
            {d.sites.length ? (
              <form
                className="mt-4 flex flex-wrap gap-2"
                onSubmit={(e) => {
                  e.preventDefault()
                  openSite(siteId)
                }}
              >
                <select className="input min-w-0 flex-1" aria-label={t('사이트')} value={siteId} onChange={(e) => setSiteId(e.target.value)}>
                  {d.sites.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.domain || s.name} ({s.user})
                    </option>
                  ))}
                </select>
                <button className="btn-primary" type="submit">
                  {t('열기')}
                </button>
              </form>
            ) : (
              <p className="mt-4 text-sm text-ink-400">
                {t('아직 사이트가 없습니다.')}{' '}
                <Link to="/sites" className="text-signal hover:underline">
                  {t('사이트 만들기')}
                </Link>
              </p>
            )}
          </section>

          <section className="card p-5" aria-label={t('서버 터미널 (root)')}>
            <h2 className="flex items-center gap-2 font-medium">
              {t('서버 터미널 (root)')}
              <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${d.root.enabled ? 'bg-fail/15 text-fail' : 'bg-ink-800 text-ink-400'}`} data-testid="root-terminal-state">
                {d.root.enabled ? t('켜짐') : t('꺼짐')}
              </span>
            </h2>
            {!d.root.enabled ? (
              <>
                <p className="mt-1 text-xs text-ink-400">{t('보안을 위해 꺼져 있습니다. 쓰려면 서버에 SSH 로 접속해 아래 명령을 실행하세요 (패널에서는 켤 수 없습니다).')}</p>
                <div className="mt-3 flex items-center gap-2">
                  <code className="kbd flex-1 truncate py-1.5 text-xs">sudo rpanel-terminal on</code>
                  <CopyButton text="sudo rpanel-terminal on" />
                </div>
                <p className="mt-2 text-[11px] text-ink-400">{t('다 쓰면 sudo rpanel-terminal off 로 끄세요.')}</p>
              </>
            ) : !d.root.twofa ? (
              <p className="mt-2 text-sm text-ink-300">
                {t('서버 터미널은 2단계 인증을 켠 계정만 쓸 수 있습니다.')}{' '}
                <Link to="/security?tab=2fa" className="text-signal hover:underline">
                  {t('2단계 인증 켜기')}
                </Link>
              </p>
            ) : (
              <form
                className="mt-4 flex flex-wrap gap-2"
                onSubmit={(e) => {
                  e.preventDefault()
                  if (!code.trim()) return
                  open(newSession('root', { kind: 'root', code: code.trim() }, true))
                  setCode('')
                }}
              >
                <input
                  className="input w-40 flex-1 font-mono tracking-widest"
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  placeholder="123456"
                  aria-label={t('인증 코드')}
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                />
                <button className="btn-danger" type="submit" disabled={!code.trim()}>
                  {t('root 로 열기')}
                </button>
                <p className="w-full text-[11px] text-ink-400">{t('열 때마다 인증 앱의 새 코드가 필요합니다. 여는 순간 알림이 갑니다.')}</p>
              </form>
            )}
          </section>
        </div>
      )}

      <TerminalSessions sessions={sessions} setSessions={setSessions} activeKey={activeKey} setActiveKey={setActiveKey} />
      {d && d.available && !sessions.length && (
        <p className="text-center text-xs text-ink-400">
          {t('한 사람이 동시에 {n}개까지 열 수 있고, {m}분 동안 입력이 없으면 자동으로 닫힙니다.', { n: d.limits.per_user, m: d.limits.idle_minutes })}
        </p>
      )}
    </>
  )
}
