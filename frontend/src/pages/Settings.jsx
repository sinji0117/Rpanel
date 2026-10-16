import { useState } from 'react'
import { Link } from 'react-router'
import { ErrorBox, Field, PageHeader, Section } from '../components/ui'
import { useAuth } from '../context/AuthContext'
import { useToast } from '../context/ToastContext'
import { LANGS, setLang, t, useLang } from '../i18n'
import { del, get, post, put } from '../lib/api'
import { useFetch } from '../lib/useFetch'
import { confirmDialog } from '../components/Dialog'
import { getTheme, setTheme } from '../lib/prefs'

const THEMES = [
  { id: 'light', label: '낮', desc: '밝은 화면', icon: '☀' },
  { id: 'dark', label: '밤', desc: '어두운 화면', icon: '☾' },
  { id: 'system', label: '자동', desc: '컴퓨터 설정을 따름', icon: '◐' },
]

function ChoiceCards({ name, value, options, onChange }) {
  return (
    <div className="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={name}>
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          role="radio"
          aria-checked={value === o.id}
          onClick={() => onChange(o.id)}
          className={`rounded-lg border px-3 py-2.5 text-left transition ${value === o.id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
        >
          <span className="block text-sm font-medium">
            {o.icon && <span aria-hidden="true">{o.icon} </span>}
            {o.label}
          </span>
          {o.desc && <span className="mt-0.5 block text-[11px] text-ink-400">{o.desc}</span>}
        </button>
      ))}
    </div>
  )
}

/** 패널 전용 도메인 (https://panel.example.com) */
function PanelDomain() {
  const { user } = useAuth()
  const toast = useToast()
  const d = useFetch(() => (user?.role === 'admin' ? get('/system/panel-domain') : Promise.resolve(null)), [user?.role])
  const [domain, setDomain] = useState('')
  const [email, setEmail] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  if (user?.role !== 'admin' || !d.data) return null
  const cur = d.data
  return (
    <Section
      title={t('패널 도메인')}
      desc={t('패널을 https://panel.example.com 처럼 도메인으로 엽니다. DNS 가 이 서버를 가리키고 80·443 포트가 열려 있어야 합니다. 기존 :포트 주소도 계속 쓸 수 있습니다.')}
    >
      {cur.domain ? (
        <div className="flex flex-wrap items-center gap-3 text-sm" data-testid="panel-domain">
          <a href={cur.url} className="font-mono text-signal hover:underline">
            {cur.url}
          </a>
          <button
            className="btn-danger ml-auto px-2.5 py-1 text-xs"
            disabled={busy}
            onClick={async () => {
              if (!(await confirmDialog(t('패널 도메인 {d} 연결을 해제합니다.', { d: cur.domain }), { title: t('패널 도메인 해제'), danger: true }))) return
              setBusy(true)
              try {
                await del('/system/panel-domain')
                toast.push(t('해제했습니다'), 'success')
                d.reload(true)
              } catch (e) {
                toast.push(e.message, 'error')
              } finally {
                setBusy(false)
              }
            }}
          >
            {t('해제')}
          </button>
        </div>
      ) : (
        <form
          className="space-y-3"
          onSubmit={async (e) => {
            e.preventDefault()
            setBusy(true)
            setError(null)
            try {
              await put('/system/panel-domain', { domain, email })
              toast.push(t('패널 도메인을 연결했습니다'), 'success')
              d.reload(true)
            } catch (err) {
              setError(err)
            } finally {
              setBusy(false)
            }
          }}
        >
          <Field label={t('도메인')} htmlFor="panel-domain">
            <input id="panel-domain" className="input font-mono" placeholder="panel.example.com" required value={domain} onChange={(e) => setDomain(e.target.value)} />
          </Field>
          <Field label={t('이메일 (인증서 만료 알림)')} htmlFor="panel-email">
            <input id="panel-email" type="email" className="input" required value={email} onChange={(e) => setEmail(e.target.value)} />
          </Field>
          <ErrorBox error={error} />
          <button className="btn-primary" disabled={busy}>
            {busy ? t('인증서 발급 중…') : t('연결')}
          </button>
        </form>
      )}
    </Section>
  )
}

export default function Settings() {
  const { user, logout } = useAuth()
  const toast = useToast()
  const lang = useLang()
  const [theme, setThemeState] = useState(getTheme())
  const [cur, setCur] = useState('')
  const [next, setNext] = useState('')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)

  return (
    <>
      <PageHeader title={t('설정')} subtitle={user?.email} />
      <div className="grid gap-4 lg:grid-cols-2">
        <Section title={t('화면')} desc={t('테마와 언어는 이 브라우저에 저장됩니다.')}>
          <div className="space-y-5">
            <div>
              <span className="label">{t('테마')}</span>
              <ChoiceCards
                name={t('테마')}
                value={theme}
                options={THEMES.map((o) => ({ ...o, label: t(o.label), desc: t(o.desc) }))}
                onChange={(id) => {
                  setTheme(id)
                  setThemeState(id)
                }}
              />
            </div>
            <div>
              <span className="label">{t('언어')}</span>
              <ChoiceCards name={t('언어')} value={lang} options={LANGS} onChange={setLang} />
            </div>
          </div>
        </Section>

        <Section
          title={t('알림')}
          desc={t('배포·백업 실패, 사이트 응답 없음, 디스크·메모리 부족 등을 알려 줍니다. Discord · Slack · Telegram · 웹훅으로도 보낼 수 있습니다.')}
          actions={
            <Link to="/notifications?tab=settings" className="btn-ghost">
              {t('알림 설정')}
            </Link>
          }
        >
          <p className="text-sm text-ink-400">{t('어떤 일을 알릴지와 보낼 곳을 정합니다.')}</p>
        </Section>

        <PanelDomain />

        <Section title={t('내 비밀번호 변경')} desc={t('변경하면 모든 기기의 세션이 로그아웃됩니다. 다른 관리자는 관리자 메뉴에서 관리합니다.')}>
          <form
            className="space-y-3"
            onSubmit={async (e) => {
              e.preventDefault()
              setBusy(true)
              setError(null)
              try {
                await post('/auth/password', { current_password: cur, new_password: next })
                toast.push(t('비밀번호를 변경했습니다. 다시 로그인하세요'), 'success')
                await logout()
              } catch (err) {
                setError(err)
              } finally {
                setBusy(false)
              }
            }}
          >
            <Field label={t('현재 비밀번호')} htmlFor="cur">
              <input id="cur" type="password" autoComplete="current-password" className="input" required value={cur} onChange={(e) => setCur(e.target.value)} />
            </Field>
            <Field label={t('새 비밀번호')} htmlFor="next" hint={t('10자 이상')}>
              <input id="next" type="password" autoComplete="new-password" minLength={10} className="input" required value={next} onChange={(e) => setNext(e.target.value)} />
            </Field>
            <ErrorBox error={error} />
            <button className="btn-primary" disabled={busy}>
              {t('변경')}
            </button>
          </form>
        </Section>
      </div>
    </>
  )
}
