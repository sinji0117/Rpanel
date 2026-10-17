import { useState } from 'react'
import { Link } from 'react-router'
import { ErrorBox, Field, PageHeader, Section } from '../components/ui'
import { useAuth } from '../context/AuthContext'
import { useToast } from '../context/ToastContext'
import { t, useLang } from '../i18n'
import { LangGrid } from '../components/LangPicker'
import { del, get, post, put } from '../lib/api'
import { useFetch } from '../lib/useFetch'
import { confirmDialog } from '../components/Dialog'
import { getTheme, setTheme } from '../lib/prefs'
import { StorageSettings } from './StorageSettings'

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

/** 패널 접속 허용 IP */
function PanelAccess() {
  const { user } = useAuth()
  const toast = useToast()
  const d = useFetch(() => (user?.role === 'admin' ? get('/system/panel-access') : Promise.resolve(null)), [user?.role])
  const [text, setText] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  if (user?.role !== 'admin' || !d.data) return null
  const cur = d.data
  const value = text ?? cur.allow.join('\n')
  const list = value
    .split(/[\s,]+/)
    .map((x) => x.trim())
    .filter(Boolean)
  const save = async (allow) => {
    setBusy(true)
    setError(null)
    try {
      const r = await put('/system/panel-access', { allow })
      d.setData(r)
      setText(null)
      toast.push(r.restricted ? t('허용한 IP 에서만 패널에 접속할 수 있습니다') : t('접속 제한을 풀었습니다'), 'success')
    } catch (err) {
      setError(err)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Section
      title={t('패널 접속 IP')}
      desc={t(
        '여기에 넣은 IP·대역에서만 패널을 열 수 있습니다 (비우면 어디서나). Git 웹훅 주소는 제한하지 않습니다. 잠겼다면 서버에서 sudo /usr/local/lib/rpanel/rpanel-helper panel-access any 로 풉니다.',
      )}
    >
      <div className="space-y-3">
        <p className="flex flex-wrap items-center gap-2 text-sm">
          <span className={cur.restricted ? 'text-warn' : 'text-ink-400'} data-testid="panel-access-state">
            {cur.restricted ? t('제한 중 · {n}개', { n: cur.allow.length }) : t('어디서나 접속 가능')}
          </span>
          <span className="ml-auto text-xs text-ink-400">
            {t('지금 내 IP')} <code className="font-mono">{cur.my_ip}</code>
          </span>
          {!list.includes(cur.my_ip) && /^[0-9a-f.:]+$/i.test(cur.my_ip) && (
            <button type="button" className="btn-ghost px-2 py-0.5 text-xs" onClick={() => setText([...list, cur.my_ip].join('\n'))}>
              {t('+ 내 IP 추가')}
            </button>
          )}
        </p>
        <Field label={t('허용 IP · 대역 (한 줄에 하나)')} htmlFor="panel-allow">
          <textarea
            id="panel-allow"
            className="input min-h-24 font-mono text-xs"
            placeholder={'203.0.113.5\n10.0.0.0/24'}
            value={value}
            onChange={(e) => setText(e.target.value)}
          />
        </Field>
        <ErrorBox error={error} />
        <div className="flex gap-2">
          <button className="btn-primary" disabled={busy} onClick={() => save(list)}>
            {t('적용')}
          </button>
          {cur.restricted && (
            <button
              className="btn-ghost"
              disabled={busy}
              onClick={async () => (await confirmDialog(t('접속 IP 제한을 풀까요? 어디서나 패널 로그인 화면을 열 수 있게 됩니다.'))) && save([])}
            >
              {t('제한 풀기')}
            </button>
          )}
        </div>
      </div>
    </Section>
  )
}

export default function Settings() {
  const { user, logout } = useAuth()
  const toast = useToast()
  useLang()
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
              <LangGrid />
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
        <PanelAccess />
        <StorageSettings />

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
