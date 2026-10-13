import { useState } from 'react'
import { ErrorBox, Field, PageHeader } from '../components/ui'
import { useAuth } from '../context/AuthContext'
import { useToast } from '../context/ToastContext'
import { get, post } from '../lib/api'
import { dateTime } from '../lib/format'
import { useFetch } from '../lib/useFetch'

export default function Settings() {
  const { user, logout } = useAuth()
  const toast = useToast()
  const info = useFetch(() => get('/system/info'), [])
  const audit = useFetch(() => get('/audit?limit=100'), [])
  const [cur, setCur] = useState('')
  const [next, setNext] = useState('')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)

  return (
    <>
      <PageHeader title="설정" subtitle={user?.email} />
      <div className="grid gap-4 lg:grid-cols-2">
        <section className="card p-5">
          <h2 className="font-medium">비밀번호 변경</h2>
          <p className="mt-1 text-sm text-ink-400">변경하면 모든 기기의 세션이 로그아웃됩니다.</p>
          <form
            className="mt-4 space-y-3"
            onSubmit={async (e) => {
              e.preventDefault()
              setBusy(true)
              setError(null)
              try {
                await post('/auth/password', { current_password: cur, new_password: next })
                toast.push('비밀번호를 변경했습니다. 다시 로그인하세요', 'success')
                await logout()
              } catch (err) {
                setError(err)
              } finally {
                setBusy(false)
              }
            }}
          >
            <Field label="현재 비밀번호" htmlFor="cur">
              <input id="cur" type="password" autoComplete="current-password" className="input" required value={cur} onChange={(e) => setCur(e.target.value)} />
            </Field>
            <Field label="새 비밀번호" htmlFor="next" hint="10자 이상">
              <input id="next" type="password" autoComplete="new-password" minLength={10} className="input" required value={next} onChange={(e) => setNext(e.target.value)} />
            </Field>
            <ErrorBox error={error} />
            <button className="btn-primary" disabled={busy}>
              변경
            </button>
          </form>
        </section>

        <section className="card p-5">
          <h2 className="font-medium">시스템</h2>
          {info.data && (
            <dl className="mt-4 grid grid-cols-[120px_1fr] gap-y-2 text-sm">
              <dt className="text-ink-400">Rpanel</dt>
              <dd className="font-mono">v{info.data.version}</dd>
              <dt className="text-ink-400">실행 모드</dt>
              <dd className="font-mono">{info.data.exec_mode}</dd>
              <dt className="text-ink-400">호스트</dt>
              <dd className="font-mono">{info.data.host?.hostname}</dd>
              <dt className="text-ink-400">OS</dt>
              <dd>{info.data.host?.os}</dd>
              <dt className="text-ink-400">커널</dt>
              <dd className="font-mono">{info.data.host?.kernel}</dd>
              <dt className="text-ink-400">CPU</dt>
              <dd>
                {info.data.host?.cpu_brand} × {info.data.host?.cpu_count}
              </dd>
              <dt className="text-ink-400">앱 포트 범위</dt>
              <dd className="font-mono">{info.data.app_port_range?.join('–')}</dd>
            </dl>
          )}
        </section>
      </div>

      <section className="card mt-4">
        <h2 className="border-b border-ink-700/70 px-5 py-3.5 text-sm font-medium">감사 로그</h2>
        <div className="max-h-[480px] overflow-auto">
          <table className="w-full text-sm">
            <tbody className="divide-y divide-ink-800">
              {audit.data?.map((e) => (
                <tr key={e.id}>
                  <td className="px-5 py-2 text-xs whitespace-nowrap text-ink-400">{dateTime(e.created_at)}</td>
                  <td className="px-3 py-2 font-mono text-xs">{e.action}</td>
                  <td className="px-3 py-2 text-xs text-ink-300">{e.target}</td>
                  <td className="px-3 py-2 text-xs text-ink-400">{e.email ?? '—'}</td>
                  <td className="px-5 py-2 font-mono text-xs text-ink-400">{e.ip}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </>
  )
}
