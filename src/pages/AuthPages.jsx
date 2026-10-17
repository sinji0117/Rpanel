import { useState } from 'react'
import { login, setup } from '../lib/api'
import { Logo, ThemeLangSwitch } from '../components/Layout'
import { ErrorBox, Field } from '../components/ui'
import { t } from '../i18n'

export function Shell({ title, subtitle, children }) {
  return (
    <div className="relative flex min-h-screen items-center justify-center px-4">
      <div className="absolute top-4 right-4">
        <ThemeLangSwitch compact />
      </div>
      <div className="w-full max-w-sm">
        <div className="mb-8 flex justify-center">
          <Logo />
        </div>
        <div className="card p-6">
          <h1 className="text-lg font-semibold">{title}</h1>
          {subtitle && <p className="mt-1 text-sm text-ink-400">{subtitle}</p>}
          <div className="mt-6">{children}</div>
        </div>
        <p className="mt-6 text-center text-xs text-ink-600">Rpanel — self-hosted web panel</p>
      </div>
    </div>
  )
}

export function LoginPage() {
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)

  return (
    <Shell title={t('로그인')} subtitle={t('Rpanel 관리자 계정으로 로그인하세요')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            await login(email, password)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('이메일')} htmlFor="email">
          <input id="email" type="email" autoComplete="username" required className="input" value={email} onChange={(e) => setEmail(e.target.value)} />
        </Field>
        <Field label={t('비밀번호')} htmlFor="password">
          <input id="password" type="password" autoComplete="current-password" required className="input" value={password} onChange={(e) => setPassword(e.target.value)} />
        </Field>
        <ErrorBox error={error} />
        <button className="btn-primary w-full" disabled={busy}>
          {busy ? t('로그인 중…') : t('로그인')}
        </button>
      </form>
    </Shell>
  )
}

export function SetupPage() {
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)

  return (
    <Shell title={t('초기 설정')} subtitle={t('첫 관리자 계정을 만듭니다. 이 화면은 계정이 하나도 없을 때만 나타납니다.')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          if (password !== confirm) {
            setError(new Error(t('비밀번호 확인이 일치하지 않습니다')))
            return
          }
          setBusy(true)
          setError(null)
          try {
            await setup(email, password)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('이메일')} htmlFor="email">
          <input id="email" type="email" autoComplete="username" required className="input" value={email} onChange={(e) => setEmail(e.target.value)} />
        </Field>
        <Field label={t('비밀번호')} htmlFor="password" hint={t('10자 이상')}>
          <input
            id="password"
            type="password"
            autoComplete="new-password"
            minLength={10}
            required
            className="input"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </Field>
        <Field label={t('비밀번호 확인')} htmlFor="confirm">
          <input id="confirm" type="password" autoComplete="new-password" required className="input" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        </Field>
        <ErrorBox error={error} />
        <button className="btn-primary w-full" disabled={busy}>
          {busy ? t('만드는 중…') : t('관리자 계정 만들기')}
        </button>
      </form>
    </Shell>
  )
}
