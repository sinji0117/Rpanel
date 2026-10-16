// DB 웹 관리 — 패널 관리자용(/mariadb/:id/web)과 DB 계정 로그인용(/db) 화면
import { useMemo, useState } from 'react'
import { Link, useParams } from 'react-router'
import DbManager, { qs } from '../components/DbManager'
import { ErrorBox, Field, PageHeader, Spinner } from '../components/ui'
import { get, post } from '../lib/api'
import { useFetch } from '../lib/useFetch'
import { Shell } from './AuthPages'
import { ThemeLangSwitch } from '../components/Layout'
import { t, useLang } from '../i18n'

/** 패널 관리자: 패널이 만든 DB 를 그 DB 기본 계정 권한으로 */
export function DbWebAdminPage() {
  const { id } = useParams()
  const list = useFetch(() => get('/databases'), [])
  const db = list.data?.databases?.find((d) => String(d.id) === String(id))
  const base = `/dbweb/admin/${id}`
  const client = useMemo(
    () => ({
      tables: () => get(base),
      browse: (table, q) => get(`${base}/table/${encodeURIComponent(table)}?${qs(q)}`),
      row: (table, op, body) => post(`${base}/table/${encodeURIComponent(table)}/${op}`, body),
      query: (sql) => post(`${base}/query`, { sql }),
    }),
    [base],
  )
  return (
    <>
      <PageHeader
        title={t('DB 웹 관리')}
        subtitle={db ? `${db.name} · ${db.users?.[0]?.username || ''}` : ''}
        help={t('이 DB 의 기본 계정 권한으로 접속합니다. 테이블 데이터를 보고 고치거나 SQL 을 실행할 수 있습니다. 개발자에게는 DB 계정으로만 들어가는 /db 주소를 알려 주세요.')}
        actions={
          <Link to="/mariadb" className="btn-ghost">
            {t('← MariaDB')}
          </Link>
        }
      />
      {list.loading ? <Spinner className="text-ink-400" /> : <DbManager client={client} database={id} />}
    </>
  )
}

// ───────── DB 계정 로그인 (패널 계정 없이) ─────────

const KEY = 'rpanel.dbweb'
function loadSession() {
  try {
    return JSON.parse(sessionStorage.getItem(KEY) || 'null')
  } catch {
    return null
  }
}
function saveSession(s) {
  try {
    if (s) sessionStorage.setItem(KEY, JSON.stringify(s))
    else sessionStorage.removeItem(KEY)
  } catch {
    /* 저장 못 해도 화면은 동작 */
  }
}

async function dbFetch(path, token, { method = 'GET', body } = {}) {
  const headers = { 'X-Requested-With': 'rpanel' }
  if (token) headers['X-DB-Token'] = token
  if (body !== undefined) headers['Content-Type'] = 'application/json'
  const res = await fetch(`/api/dbweb${path}`, { method, headers, body: body !== undefined ? JSON.stringify(body) : undefined })
  const text = await res.text()
  let data = null
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    data = { message: text }
  }
  if (!res.ok) {
    const err = new Error(t(data?.message || t('요청 실패 ({status})', { status: res.status })))
    err.status = res.status
    throw err
  }
  return data
}

function DbLogin({ onLogin }) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)
  return (
    <Shell title={t('DB 웹 관리')} subtitle={t('MariaDB 계정 이름과 비밀번호로 로그인하세요 (패널 계정이 아닙니다)')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            onLogin(await dbFetch('/login', null, { method: 'POST', body: { username, password } }))
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('DB 계정')} htmlFor="db-user">
          <input id="db-user" autoComplete="username" required className="input font-mono" value={username} onChange={(e) => setUsername(e.target.value)} />
        </Field>
        <Field label={t('비밀번호')} htmlFor="db-pass">
          <input id="db-pass" type="password" autoComplete="current-password" required className="input" value={password} onChange={(e) => setPassword(e.target.value)} />
        </Field>
        <ErrorBox error={error} />
        <button className="btn-primary w-full" disabled={busy}>
          {busy ? t('로그인 중…') : t('로그인')}
        </button>
      </form>
    </Shell>
  )
}

export function DbWebStandalone() {
  useLang()
  const [session, setSession] = useState(loadSession)
  const [db, setDb] = useState(() => session?.databases?.[0] || null)
  const set = (s) => {
    saveSession(s)
    setSession(s)
    setDb(s?.databases?.[0] || null)
  }
  const token = session?.token
  const client = useMemo(() => {
    const call = async (path, opts) => {
      try {
        return await dbFetch(path, token, opts)
      } catch (e) {
        if (e.status === 401) set(null)
        throw e
      }
    }
    const base = `/s/${encodeURIComponent(db || '')}`
    return {
      tables: () => call(base),
      browse: (table, q) => call(`${base}/table/${encodeURIComponent(table)}?${qs(q)}`),
      row: (table, op, body) => call(`${base}/table/${encodeURIComponent(table)}/${op}`, { method: 'POST', body }),
      query: (sql) => call(`${base}/query`, { method: 'POST', body: { sql } }),
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token, db])
  if (!session) return <DbLogin onLogin={set} />
  return (
    <div className="mx-auto max-w-7xl px-4 py-6">
      <header className="mb-5 flex flex-wrap items-center gap-3">
        <h1 className="text-lg font-semibold">{t('DB 웹 관리')}</h1>
        <span className="font-mono text-sm text-ink-400" data-testid="db-user">
          {session.username}
        </span>
        <span className="ml-auto flex items-center gap-2">
          <ThemeLangSwitch compact />
          <button
            className="btn-ghost"
            onClick={async () => {
              try {
                await dbFetch('/logout', token, { method: 'POST' })
              } catch {
                /* 이미 만료 */
              }
              set(null)
            }}
          >
            {t('로그아웃')}
          </button>
        </span>
      </header>
      <DbManager
        client={client}
        database={db}
        header={
          <select className="input mb-2 font-mono text-xs" value={db || ''} onChange={(e) => setDb(e.target.value)} aria-label={t('데이터베이스')}>
            {session.databases.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        }
      />
    </div>
  )
}
