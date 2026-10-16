import { useState } from 'react'
import { ErrorBox, Field, PageHeader, Section } from '../components/ui'
import { useAuth } from '../context/AuthContext'
import { useToast } from '../context/ToastContext'
import { LANGS, setLang, t, useLang } from '../i18n'
import { post } from '../lib/api'
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
