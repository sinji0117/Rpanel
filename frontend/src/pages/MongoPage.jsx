import { useState } from 'react'
import { Link } from 'react-router'
import { genPassword } from '../components/CreateSiteModal'
import { confirmDialog } from '../components/Dialog'
import { CopyButton, Empty, ErrorBox, Field, Modal, PageHeader, Spinner } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { t } from '../i18n'
import { del, get, post, put } from '../lib/api'
import { bytes } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { Secret } from './AppTabs'

const ROLE = { readWrite: '읽기·쓰기', read: '읽기 전용' }

function useAct(reload) {
  const toast = useToast()
  const [busy, setBusy] = useState(false)
  const act = async (fn, msg) => {
    setBusy(true)
    try {
      const r = await fn()
      if (msg) toast.push(msg, 'success')
      reload()
      return r
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setBusy(false)
    }
  }
  return [act, busy]
}

function PasswordInput({ id, value, onChange }) {
  return (
    <div className="flex gap-2">
      <input id={id} className="input font-mono" autoComplete="new-password" value={value} onChange={(e) => onChange(e.target.value)} placeholder={t('비우면 자동 생성')} />
      <button type="button" className="btn-ghost shrink-0 px-2.5 text-xs whitespace-nowrap" onClick={() => onChange(genPassword(20))}>
        {t('생성')}
      </button>
    </div>
  )
}

/** 만든 직후·비밀번호 변경 직후 접속 주소 */
function CredentialModal({ info, onClose }) {
  if (!info) return null
  return (
    <Modal open onClose={onClose} title={info.title}>
      <div className="space-y-3 text-sm">
        <p className="text-ink-300">{t('아래 접속 주소를 프로그램 설정(환경변수)에 넣으세요. 목록에서도 “보기”로 다시 볼 수 있습니다.')}</p>
        <Secret value={info.url} />
        {info.password && (
          <p className="flex items-center gap-2 text-xs">
            {t('비밀번호')}{' '}
            <code className="rounded bg-ink-950 px-2 py-1 font-mono" data-testid="mongo-password">
              {info.password}
            </code>
            <CopyButton text={info.password} />
          </p>
        )}
        <div className="flex justify-end">
          <button className="btn-primary" onClick={onClose}>
            {t('확인')}
          </button>
        </div>
      </div>
    </Modal>
  )
}

function CreateModal({ apps, onClose, onCreated }) {
  const [v, setV] = useState({ name: '', username: '', password: '', app_id: '', env_key: 'MONGODB_URL' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const set = (k) => (e) => setV((s) => ({ ...s, [k]: e.target.value }))
  return (
    <Modal open onClose={onClose} title={t('MongoDB 데이터베이스 만들기')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            onCreated(
              await post('/mongo', {
                name: v.name,
                username: v.username || undefined,
                password: v.password || undefined,
                app_id: v.app_id ? Number(v.app_id) : null,
                env_key: v.env_key,
              }),
            )
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('DB 이름')} htmlFor="mdb-name" hint={t('소문자로 시작, 소문자·숫자·_ (예: shop)')}>
          <input id="mdb-name" className="input font-mono" required value={v.name} onChange={set('name')} />
        </Field>
        <Field label={t('기본 계정 이름 (선택)')} htmlFor="mdb-user" hint={t('비우면 DB 이름과 같게 만듭니다. 이 DB 에만 읽기·쓰기 권한이 있습니다')}>
          <input id="mdb-user" className="input font-mono" value={v.username} onChange={set('username')} />
        </Field>
        <Field label={t('비밀번호')} htmlFor="mdb-pw">
          <PasswordInput id="mdb-pw" value={v.password} onChange={(password) => setV((s) => ({ ...s, password }))} />
        </Field>
        <Field label={t('앱에 연결 (선택)')} htmlFor="mdb-app" hint={t('연결하면 배포(적용) 때 접속 주소가 환경변수로 들어갑니다')}>
          <select id="mdb-app" className="input" value={v.app_id} onChange={set('app_id')}>
            <option value="">{t('연결하지 않음')}</option>
            {apps.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </Field>
        {v.app_id && (
          <Field label={t('환경변수 이름')} htmlFor="mdb-env">
            <input id="mdb-env" className="input font-mono" value={v.env_key} onChange={set('env_key')} />
          </Field>
        )}
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary" disabled={busy}>
            {busy ? t('만드는 중…') : t('만들기')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

function LinkModal({ db, apps, onClose, onSaved }) {
  const [appId, setAppId] = useState(db.app_id ?? '')
  const [envKey, setEnvKey] = useState(db.env_key || 'MONGODB_URL')
  const [error, setError] = useState(null)
  return (
    <Modal open onClose={onClose} title={t('{name} 앱 연결', { name: db.name })}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          try {
            await put(`/mongo/${db.id}`, { app_id: appId ? Number(appId) : null, env_key: envKey })
            onSaved()
          } catch (err) {
            setError(err)
          }
        }}
      >
        <Field label={t('앱')} htmlFor="mlink-app">
          <select id="mlink-app" className="input" value={appId} onChange={(e) => setAppId(e.target.value)}>
            <option value="">{t('연결하지 않음')}</option>
            {apps.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t('환경변수 이름')} htmlFor="mlink-env">
          <input id="mlink-env" className="input font-mono" value={envKey} onChange={(e) => setEnvKey(e.target.value)} />
        </Field>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary">{t('저장')}</button>
        </div>
      </form>
    </Modal>
  )
}

function AddUser({ db, onCreated, onCancel }) {
  const [v, setV] = useState({ username: '', role: 'readWrite', password: '' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  return (
    <form
      className="mt-3 grid gap-3 rounded-lg border border-ink-700 p-4 sm:grid-cols-3"
      aria-label={t('{name} 계정 추가', { name: db.name })}
      onSubmit={async (e) => {
        e.preventDefault()
        setBusy(true)
        setError(null)
        try {
          onCreated(await post(`/mongo/${db.id}/users`, { ...v, password: v.password || undefined }))
        } catch (err) {
          setError(err)
        } finally {
          setBusy(false)
        }
      }}
    >
      <Field label={t('계정 이름')} htmlFor={`mu-name-${db.id}`}>
        <input
          id={`mu-name-${db.id}`}
          className="input font-mono"
          required
          value={v.username}
          onChange={(e) => setV({ ...v, username: e.target.value })}
          placeholder={`${db.name}_reader`}
        />
      </Field>
      <Field label={t('권한')} htmlFor={`mu-role-${db.id}`}>
        <select id={`mu-role-${db.id}`} className="input" value={v.role} onChange={(e) => setV({ ...v, role: e.target.value })}>
          <option value="readWrite">{t('읽기·쓰기')}</option>
          <option value="read">{t('읽기 전용')}</option>
        </select>
      </Field>
      <Field label={t('비밀번호')} htmlFor={`mu-pw-${db.id}`}>
        <PasswordInput id={`mu-pw-${db.id}`} value={v.password} onChange={(password) => setV({ ...v, password })} />
      </Field>
      <div className="flex justify-end gap-2 sm:col-span-3">
        <button type="button" className="btn-ghost" onClick={onCancel}>
          {t('취소')}
        </button>
        <button className="btn-primary" disabled={busy}>
          {busy ? t('만드는 중…') : t('계정 만들기')}
        </button>
      </div>
      {error && (
        <div className="sm:col-span-3">
          <ErrorBox error={error} />
        </div>
      )}
    </form>
  )
}

function MongoCard({ db, apps, reload, showCred }) {
  const [act, busy] = useAct(reload)
  const [adding, setAdding] = useState(false)
  const [linking, setLinking] = useState(false)
  return (
    <li className="py-4" data-testid={`mongo-${db.name}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <p className="font-mono text-sm font-medium">{db.name}</p>
          <p className="mt-0.5 text-xs text-ink-400">
            {db.app_id ? (
              <>
                <Link to={`/apps/${db.app_id}`} className="text-signal hover:underline">
                  {db.app_name}
                </Link>{' '}
                · <span className="font-mono">{db.env_key}</span>
              </>
            ) : (
              t('앱에 연결 안 됨')
            )}
            {db.size_bytes != null && <> · {bytes(db.size_bytes)}</>}
          </p>
        </div>
        <div className="flex flex-wrap gap-1.5">
          <Link to={`/mongodb/${db.id}/web`} className="btn-ghost px-2.5 py-1 text-xs" data-testid={`mongoweb-${db.name}`}>
            {t('웹 관리')}
          </Link>
          <button className="btn-ghost px-2.5 py-1 text-xs" disabled={busy} onClick={() => setLinking(true)}>
            {t('앱 연결')}
          </button>
          <button
            className="btn-ghost px-2.5 py-1 text-xs"
            disabled={busy}
            onClick={() => act(() => post(`/mongo/${db.id}/backups`), t('{name} 백업을 시작했습니다', { name: db.name }))}
          >
            {t('지금 백업')}
          </button>
          <button
            className="btn-ghost px-2.5 py-1 text-xs"
            disabled={busy}
            onClick={async () => {
              if (!(await confirmDialog(t('{username} 의 비밀번호를 새로 만들까요? 이 계정을 쓰는 프로그램 설정도 바꿔야 합니다.', { username: db.username })))) return
              const r = await act(() => post(`/mongo/${db.id}/password`, {}), t('비밀번호를 새로 만들었습니다'))
              if (r) showCred({ title: t('{name} 접속 정보', { name: db.name }), url: r.url, password: r.password })
            }}
          >
            {t('비밀번호')}
          </button>
          <button
            className="btn-danger px-2.5 py-1 text-xs"
            disabled={busy}
            onClick={async () => {
              const ok = await confirmDialog(t('MongoDB 데이터베이스 {name} 와(과) 모든 데이터, 이 DB의 계정이 모두 삭제됩니다. 되돌릴 수 없습니다.', { name: db.name }), {
                title: t('MongoDB 데이터베이스 삭제'),
                requireText: db.name,
                danger: true,
              })
              if (ok) act(() => del(`/mongo/${db.id}`), t('{name} 삭제됨', { name: db.name }))
            }}
          >
            {t('삭제')}
          </button>
        </div>
      </div>
      <div className="mt-3">
        <Secret value={db.url} />
      </div>
      <table className="mt-3 w-full text-sm" aria-label={t('{name} 계정', { name: db.name })}>
        <tbody className="divide-y divide-ink-800">
          <tr>
            <td className="py-2 pr-3 font-mono text-xs">{db.username}</td>
            <td className="px-3 py-2 text-xs text-ink-400">
              {t('읽기·쓰기')} · {t('기본 계정')}
            </td>
            <td />
          </tr>
          {db.users.map((u) => (
            <tr key={u.id}>
              <td className="py-2 pr-3 font-mono text-xs">{u.username}</td>
              <td className="px-3 py-2 text-xs text-ink-400">{t(ROLE[u.role] ?? u.role)}</td>
              <td className="py-2 pl-3 text-right whitespace-nowrap">
                <button className="btn-ghost mr-1.5 px-2.5 py-1 text-xs" onClick={() => showCred({ title: t('{name} 접속 정보', { name: u.username }), url: u.url })}>
                  {t('보기')}
                </button>
                <button
                  className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                  disabled={busy}
                  onClick={async () => {
                    if (!(await confirmDialog(t('{username} 의 비밀번호를 새로 만들까요?', { username: u.username })))) return
                    const r = await act(() => post(`/mongo/${db.id}/users/${u.id}/password`, {}), t('비밀번호를 새로 만들었습니다'))
                    if (r) showCred({ title: t('{name} 접속 정보', { name: u.username }), url: r.url, password: r.password })
                  }}
                >
                  {t('비밀번호')}
                </button>
                <button
                  className="btn-ghost px-2.5 py-1 text-xs text-red-300"
                  disabled={busy}
                  aria-label={t('{username} 계정 삭제', { username: u.username })}
                  onClick={async () =>
                    (await confirmDialog(t('계정 {username} 을(를) 삭제할까요?', { username: u.username }))) &&
                    act(() => del(`/mongo/${db.id}/users/${u.id}`), t('{username} 삭제됨', { username: u.username }))
                  }
                >
                  {t('삭제')}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {adding ? (
        <AddUser
          db={db}
          onCancel={() => setAdding(false)}
          onCreated={(r) => {
            setAdding(false)
            reload()
            showCred({ title: t('{name} 접속 정보', { name: r.username }), url: r.url, password: r.password })
          }}
        />
      ) : (
        <button className="btn-ghost mt-2 px-2.5 py-1 text-xs" onClick={() => setAdding(true)}>
          {t('+ 계정')}
        </button>
      )}
      {linking && (
        <LinkModal
          db={db}
          apps={apps}
          onClose={() => setLinking(false)}
          onSaved={() => {
            setLinking(false)
            reload()
          }}
        />
      )}
    </li>
  )
}

export default function MongoPage() {
  const data = useFetch(() => get('/mongo'), [])
  const apps = useFetch(() => get('/apps'), [])
  const [creating, setCreating] = useState(false)
  const [cred, setCred] = useState(null)
  const d = data.data
  return (
    <>
      <PageHeader
        title="MongoDB"
        subtitle={d?.version ? t('MongoDB {v} · 127.0.0.1:27017 · 패널 로그도 여기에 저장됩니다', { v: d.version }) : undefined}
        help={t(
          '프로젝트마다 MongoDB 데이터베이스와 전용 계정을 만듭니다. 앱에 연결하면 배포(적용) 때 MONGODB_URL 이 환경변수로 들어갑니다. 계정은 그 데이터베이스에만 권한이 있습니다.',
        )}
        actions={
          <button className="btn-primary" disabled={!d?.available} onClick={() => setCreating(true)}>
            {t('+ MongoDB 데이터베이스')}
          </button>
        }
      />
      {data.loading && <Spinner className="text-ink-400" />}
      <ErrorBox error={data.error} />
      {d && !d.available && (
        <div className="mb-4 rounded-lg border border-warn/40 bg-warn/10 px-4 py-3 text-sm" data-testid="mongo-unavailable">
          <p className="font-medium">{d.configured ? t('MongoDB 에 접속하지 못했습니다') : t('MongoDB 가 설치되지 않았습니다')}</p>
          <p className="mt-1 text-ink-300">{d.error}</p>
          <p className="mt-2 text-xs text-ink-400">
            {t('서버에서 설치:')} <span className="kbd">sudo rpanel-update --force --mongodb</span> {t('(CPU 에 AVX 기능이 있어야 합니다. 가상머신이면 CPU 종류를 host 로 하세요)')}
          </p>
        </div>
      )}
      {d?.available &&
        (d.databases.length ? (
          <section className="card px-5">
            <ul className="divide-y divide-ink-800">
              {d.databases.map((db) => (
                <MongoCard key={db.id} db={db} apps={apps.data ?? []} reload={() => data.reload(true)} showCred={setCred} />
              ))}
            </ul>
          </section>
        ) : (
          <Empty title={t('MongoDB 데이터베이스가 없습니다')} />
        ))}
      {creating && (
        <CreateModal
          apps={apps.data ?? []}
          onClose={() => setCreating(false)}
          onCreated={(r) => {
            setCreating(false)
            data.reload(true)
            setCred({ title: t('{name} 접속 정보', { name: r.database.name }), url: r.url, password: r.password })
          }}
        />
      )}
      <CredentialModal info={cred} onClose={() => setCred(null)} />
    </>
  )
}
