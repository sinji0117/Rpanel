import { useState } from 'react'
import { Link } from 'react-router'
import { CopyButton, Empty, ErrorBox, Field, Modal, PageHeader, Section, Spinner } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { genPassword } from '../components/CreateSiteModal'
import { del, get, post, put } from '../lib/api'
import { useFetch, useInterval } from '../lib/useFetch'
import { BackupTable, DbCredentials, Secret } from './AppTabs'
import { BackupJobs } from './BackupJobs'
import { t, tx } from '../i18n'

const PRIV = { all: '읽기·쓰기', read: '읽기 전용' }

/** 계정을 만들거나 비밀번호를 바꾼 직후 접속 정보를 보여 주는 창 */
function CredentialModal({ info, onClose }) {
  if (!info) return null
  return (
    <Modal open onClose={onClose} title={info.title}>
      <div className="space-y-3 text-sm">
        <p className="text-ink-300">{t('아래 접속 주소를 프로그램 설정(환경변수)에 넣으세요. 목록에서도 “보기”로 다시 볼 수 있습니다.')}</p>
        <Secret value={info.url} />
        {info.url.startsWith('mysql') && <DbCredentials url={info.url} />}
        {info.password && (
          <p className="flex items-center gap-2 text-xs">
            {t('비밀번호')}{' '}
            <code className="rounded bg-ink-950 px-2 py-1 font-mono" data-testid="new-password">
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

// ───────────── MariaDB ─────────────

function CreateDbModal({ apps, onClose, onCreated }) {
  const [v, setV] = useState({ name: '', username: '', app_id: '', env_key: 'DATABASE_URL' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const set = (k) => (e) => setV((s) => ({ ...s, [k]: e.target.value }))
  return (
    <Modal open onClose={onClose} title={t('데이터베이스 만들기')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            const r = await post('/databases', { name: v.name, username: v.username || undefined, app_id: v.app_id ? Number(v.app_id) : null, env_key: v.env_key })
            onCreated(r)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('DB 이름')} htmlFor="ndb-name" hint={t('소문자·숫자·_ (예: shop)')}>
          <input id="ndb-name" className="input font-mono" required value={v.name} onChange={set('name')} />
        </Field>
        <Field label={t('기본 계정 이름 (선택)')} htmlFor="ndb-user" hint={t('비우면 DB 이름과 같게 만듭니다. 비밀번호는 자동 생성')}>
          <input id="ndb-user" className="input font-mono" value={v.username} onChange={set('username')} />
        </Field>
        <Field label={t('앱에 연결 (선택)')} htmlFor="ndb-app" hint={t('연결하면 배포(적용) 때 접속 주소가 환경변수로 들어갑니다')}>
          <select id="ndb-app" className="input" value={v.app_id} onChange={set('app_id')}>
            <option value="">{t('연결하지 않음')}</option>
            {apps.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </Field>
        {v.app_id && (
          <Field label={t('환경변수 이름')} htmlFor="ndb-env">
            <input id="ndb-env" className="input font-mono" value={v.env_key} onChange={set('env_key')} />
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

function LinkDbModal({ db, apps, onClose, onSaved }) {
  const [appId, setAppId] = useState(db.app_id ?? '')
  const [envKey, setEnvKey] = useState(db.env_key || 'DATABASE_URL')
  const [error, setError] = useState(null)
  return (
    <Modal open onClose={onClose} title={t('{name} 앱 연결', { name: db.name })}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          try {
            await put(`/databases/${db.id}`, { app_id: appId ? Number(appId) : null, env_key: envKey })
            onSaved()
          } catch (err) {
            setError(err)
          }
        }}
      >
        <Field label={t('앱')} htmlFor="link-app">
          <select id="link-app" className="input" value={appId} onChange={(e) => setAppId(e.target.value)}>
            <option value="">{t('연결하지 않음')}</option>
            {apps.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t('환경변수 이름')} htmlFor="link-env">
          <input id="link-env" className="input font-mono" value={envKey} onChange={(e) => setEnvKey(e.target.value)} />
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

function AddDbUser({ db, onCreated, onCancel }) {
  const [v, setV] = useState({ username: '', privileges: 'all', password: '' })
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
          const r = await post(`/databases/${db.id}/users`, { ...v, password: v.password || undefined })
          onCreated(r)
        } catch (err) {
          setError(err)
        } finally {
          setBusy(false)
        }
      }}
    >
      <Field label={t('계정 이름')} htmlFor={`u-name-${db.id}`}>
        <input
          id={`u-name-${db.id}`}
          className="input font-mono"
          required
          value={v.username}
          onChange={(e) => setV({ ...v, username: e.target.value })}
          placeholder={`${db.name}_ro`}
        />
      </Field>
      <Field label={t('권한')} htmlFor={`u-priv-${db.id}`}>
        <select id={`u-priv-${db.id}`} className="input" value={v.privileges} onChange={(e) => setV({ ...v, privileges: e.target.value })}>
          <option value="all">{t('읽기·쓰기 (전체)')}</option>
          <option value="read">{t('읽기 전용')}</option>
        </select>
      </Field>
      <Field label={t('비밀번호')} htmlFor={`u-pw-${db.id}`}>
        <PasswordInput id={`u-pw-${db.id}`} value={v.password} onChange={(password) => setV({ ...v, password })} />
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

function DatabaseCard({ db, apps, reload, showCred }) {
  const [act, busy] = useAct(reload)
  const [adding, setAdding] = useState(false)
  const [linking, setLinking] = useState(false)
  const toast = useToast()
  return (
    <li className="py-4" data-testid={`db-${db.name}`}>
      <div className="mb-2 flex flex-wrap items-center gap-3 text-sm">
        <span className="font-mono font-medium">{db.name}</span>
        {db.app_id ? (
          <span className="text-xs text-ink-400">
            {tx('{app} 에 연결 · {env}', {
              app: (
                <Link to={`/apps/${db.app_id}`} className="hover:text-signal">
                  {db.app_name}
                </Link>
              ),
              env: <span className="kbd">{db.env_key}</span>,
            })}
          </span>
        ) : (
          <span className="text-xs text-ink-400">{t('앱에 연결 안 됨')}</span>
        )}
        <span className="ml-auto flex flex-wrap gap-1.5">
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setAdding((x) => !x)}>
            {t('+ 계정')}
          </button>
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setLinking(true)}>
            {t('앱 연결')}
          </button>
          <button
            className="btn-ghost px-2.5 py-1 text-xs"
            disabled={busy}
            onClick={() => act(() => post(`/databases/${db.id}/backups`), t('백업을 시작했습니다 — 백업 화면에서 확인하세요'))}
          >
            {t('지금 백업')}
          </button>
          <button
            className="btn-danger px-2.5 py-1 text-xs"
            disabled={busy}
            onClick={() => {
              const typed = window.prompt(t('DB와 모든 데이터, 이 DB의 계정이 모두 삭제됩니다. 삭제하려면 DB 이름을 입력하세요: {name}', { name: db.name }))
              if (typed === db.name) act(() => del(`/databases/${db.id}`), t('{name} 삭제됨', { name: db.name }))
            }}
          >
            {t('삭제')}
          </button>
        </span>
      </div>
      <table className="w-full text-sm">
        <thead className="text-left text-xs text-ink-400">
          <tr>
            <th className="py-1.5 pr-3 font-medium">{t('계정')}</th>
            <th className="px-3 py-1.5 font-medium">{t('권한')}</th>
            <th className="px-3 py-1.5 font-medium">{t('접속 주소')}</th>
            <th className="py-1.5 pl-3" />
          </tr>
        </thead>
        <tbody className="divide-y divide-ink-800">
          {db.users.map((u) => (
            <tr key={u.username}>
              <td className="py-2 pr-3 font-mono text-xs">
                {u.username}
                {u.primary && <span className="ml-2 rounded border border-ink-600 px-1 text-[10px] text-ink-400">{t('기본')}</span>}
              </td>
              <td className="px-3 py-2 text-xs text-ink-300">{t(PRIV[u.privileges])}</td>
              <td className="max-w-md px-3 py-2">
                <Secret value={u.url} />
              </td>
              <td className="py-2 pl-3 text-right whitespace-nowrap">
                <button
                  className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                  disabled={busy}
                  onClick={async () => {
                    if (!window.confirm(t('{username} 의 비밀번호를 새로 만들까요? 이 계정을 쓰는 프로그램 설정도 바꿔야 합니다.', { username: u.username }))) return
                    const r = await act(() => (u.primary ? post(`/databases/${db.id}/password`) : post(`/databases/${db.id}/users/${u.id}/password`)), t('비밀번호를 바꿨습니다'))
                    if (r?.url)
                      showCred({
                        title: t('{username} 새 비밀번호', { username: u.username }),
                        url: r.url,
                        password: r.password,
                      })
                  }}
                >
                  {t('비밀번호 변경')}
                </button>
                {!u.primary && (
                  <button
                    className="btn-ghost px-2.5 py-1 text-xs text-red-300"
                    disabled={busy}
                    aria-label={t('{username} 계정 삭제', { username: u.username })}
                    onClick={() =>
                      window.confirm(t('계정 {username} 을(를) 삭제할까요?', { username: u.username })) &&
                      act(() => del(`/databases/${db.id}/users/${u.id}`), t('{username} 삭제됨', { username: u.username }))
                    }
                  >
                    {t('삭제')}
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {adding && (
        <AddDbUser
          db={db}
          onCancel={() => setAdding(false)}
          onCreated={(r) => {
            setAdding(false)
            toast.push(t('계정 {username} 을(를) 만들었습니다', { username: r.username }), 'success')
            reload()
            showCred({
              title: t('{username} 접속 정보', { username: r.username }),
              url: r.url,
              password: r.password,
            })
          }}
        />
      )}
      {linking && (
        <LinkDbModal
          db={db}
          apps={apps}
          onClose={() => setLinking(false)}
          onSaved={() => {
            setLinking(false)
            toast.push(t('연결을 바꿨습니다 — 앱은 다음 배포(적용)부터 반영됩니다'), 'success')
            reload()
          }}
        />
      )}
    </li>
  )
}

// ───────────── Redis 계정 ─────────────

function RedisUserModal({ initial, onClose, onSaved }) {
  const editing = !!initial
  const [v, setV] = useState(() => ({
    username: '',
    key_pattern: '',
    readonly: false,
    db_index: 0,
    note: '',
    password: '',
    ...(initial || {}),
    ...(initial ? { note: initial.note ?? '' } : {}),
  }))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const set = (k) => (e) => setV((s) => ({ ...s, [k]: e.target.type === 'checkbox' ? e.target.checked : e.target.value }))
  return (
    <Modal open onClose={onClose} title={editing ? t('Redis 계정 {username}', { username: initial.username }) : t('Redis 계정 만들기')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            const body = { key_pattern: v.key_pattern, readonly: !!v.readonly, db_index: Number(v.db_index), note: v.note }
            const r = editing ? await put(`/redis-users/${initial.id}`, body) : await post('/redis-users', { ...body, username: v.username, password: v.password || undefined })
            onSaved(r)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        {!editing && (
          <Field label={t('계정 이름')} htmlFor="ru-name" hint={t('소문자로 시작, 소문자·숫자·_- 3~32자')}>
            <input id="ru-name" className="input font-mono" required value={v.username} onChange={set('username')} placeholder="shop" />
          </Field>
        )}
        <Field
          label={t('쓸 수 있는 키')}
          htmlFor="ru-pattern"
          hint={t('이 패턴에 맞는 키만 읽고 쓸 수 있습니다. 비우면 "{v}:*", 전부 허용은 *', { v: v.username || t('계정이름') })}
        >
          <input id="ru-pattern" className="input font-mono" value={v.key_pattern} onChange={set('key_pattern')} placeholder={`${v.username || 'shop'}:*`} />
        </Field>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('DB 번호 (접속 주소용)')} htmlFor="ru-db" hint={t('Redis 는 DB 번호별 권한을 나눌 수 없어, 키 패턴으로 구분합니다')}>
            <input id="ru-db" type="number" min="0" max="15" className="input" value={v.db_index} onChange={set('db_index')} />
          </Field>
          <label className="mt-6 flex items-center gap-2 text-sm">
            <input type="checkbox" checked={!!v.readonly} onChange={set('readonly')} /> {t('읽기 전용')}
          </label>
        </div>
        {!editing && (
          <Field label={t('비밀번호')} htmlFor="ru-pw">
            <PasswordInput id="ru-pw" value={v.password} onChange={(password) => setV((s) => ({ ...s, password }))} />
          </Field>
        )}
        <Field label={t('메모 (선택)')} htmlFor="ru-note">
          <input id="ru-note" className="input" maxLength={200} value={v.note} onChange={set('note')} placeholder={t('예: 쇼핑몰 세션용')} />
        </Field>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary" disabled={busy}>
            {busy ? t('저장 중…') : editing ? t('저장') : t('만들기')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

function RedisUsers({ users, reload, showCred }) {
  const [act, busy] = useAct(reload)
  const [editing, setEditing] = useState(null)
  const toast = useToast()
  return (
    <Section
      title={t('Redis 계정')}
      desc={t('프로그램마다 따로 쓸 Redis 계정을 만듭니다. 계정마다 쓸 수 있는 키(예: shop:*)와 읽기 전용 여부를 정할 수 있고, 관리 명령(FLUSHALL·CONFIG 등)은 막혀 있습니다.')}
      actions={
        <button className="btn-primary" onClick={() => setEditing({})}>
          {t('+ Redis 계정')}
        </button>
      }
    >
      {users.length === 0 ? (
        <p className="text-sm text-ink-400">{t('만든 Redis 계정이 없습니다.')}</p>
      ) : (
        <table className="w-full text-sm" aria-label={t('Redis 계정 목록')}>
          <thead className="text-left text-xs text-ink-400">
            <tr>
              <th className="py-1.5 pr-3 font-medium">{t('계정')}</th>
              <th className="px-3 py-1.5 font-medium">{t('키 · 권한')}</th>
              <th className="px-3 py-1.5 font-medium">{t('접속 주소')}</th>
              <th className="py-1.5 pl-3" />
            </tr>
          </thead>
          <tbody className="divide-y divide-ink-800">
            {users.map((u) => (
              <tr key={u.id}>
                <td className="py-2 pr-3">
                  <span className="font-mono text-xs">{u.username}</span>
                  {u.note && <p className="text-[11px] text-ink-400">{u.note}</p>}
                </td>
                <td className="px-3 py-2 text-xs">
                  <span className="kbd mr-1.5">{u.key_pattern}</span>
                  <span className="text-ink-300">{u.readonly ? t('읽기 전용') : t('읽기·쓰기')}</span>
                </td>
                <td className="max-w-md px-3 py-2">
                  <Secret value={u.url} />
                </td>
                <td className="py-2 pl-3 text-right whitespace-nowrap">
                  <button className="btn-ghost mr-1.5 px-2.5 py-1 text-xs" onClick={() => setEditing(u)}>
                    {t('수정')}
                  </button>
                  <button
                    className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                    disabled={busy}
                    onClick={async () => {
                      if (!window.confirm(t('{username} 의 비밀번호를 새로 만들까요?', { username: u.username }))) return
                      const r = await act(() => post(`/redis-users/${u.id}/password`), t('비밀번호를 바꿨습니다'))
                      if (r?.url)
                        showCred({
                          title: t('{username} 새 비밀번호', { username: u.username }),
                          url: r.url,
                          password: r.password,
                        })
                    }}
                  >
                    {t('비밀번호 변경')}
                  </button>
                  <button
                    className="btn-ghost px-2.5 py-1 text-xs text-red-300"
                    disabled={busy}
                    aria-label={t('Redis 계정 {username} 삭제', { username: u.username })}
                    onClick={() =>
                      window.confirm(t('Redis 계정 {username} 을(를) 삭제할까요? (저장된 키는 남습니다)', { username: u.username })) &&
                      act(() => del(`/redis-users/${u.id}`), t('{username} 삭제됨', { username: u.username }))
                    }
                  >
                    {t('삭제')}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {editing && (
        <RedisUserModal
          initial={editing.id ? editing : null}
          onClose={() => setEditing(null)}
          onSaved={(r) => {
            const created = !editing.id
            setEditing(null)
            toast.push(created ? t('Redis 계정 {username} 을(를) 만들었습니다', { username: r.username }) : t('저장했습니다'), 'success')
            reload()
            if (created)
              showCred({
                title: t('{username} 접속 정보', { username: r.username }),
                url: r.url,
                password: r.password,
              })
          }}
        />
      )}
    </Section>
  )
}

export function MariaDBPage() {
  const d = useFetch(() => get('/databases'), [])
  const apps = useFetch(() => get('/apps'), [])
  const [creating, setCreating] = useState(false)
  const [cred, setCred] = useState(null)
  const toast = useToast()
  if (d.loading) return <Spinner className="text-ink-400" />
  if (d.error) return <ErrorBox error={d.error} />
  const { databases } = d.data
  const reload = () => d.reload(true)
  return (
    <>
      <PageHeader
        title="MariaDB"
        help={
          <>
            {t('데이터베이스와 계정을 만들고 지웁니다. 앱에 연결한 DB 는 배포(적용) 때 접속 주소가 환경변수로 들어갑니다.')}
            <br />
            {t('“+ 데이터베이스”로 만들거나, 앱의 데이터베이스 탭에서 만드세요.')}
          </>
        }
        actions={
          <button className="btn-primary" onClick={() => setCreating(true)}>
            {t('+ 데이터베이스')}
          </button>
        }
      />
      <Section
        title={t('데이터베이스')}
        desc={t('DB마다 기본 계정이 하나 있고, 필요하면 계정을 더 만들 수 있습니다 (예: 백업·통계용 읽기 전용 계정). 계정은 그 DB에만 권한이 있습니다.')}
      >
        {databases.length === 0 ? (
          <Empty title={t('데이터베이스가 없습니다')} />
        ) : (
          <ul className="divide-y divide-ink-800">
            {databases.map((db) => (
              <DatabaseCard key={db.id} db={db} apps={apps.data || []} reload={reload} showCred={setCred} />
            ))}
          </ul>
        )}
      </Section>
      {creating && (
        <CreateDbModal
          apps={apps.data || []}
          onClose={() => setCreating(false)}
          onCreated={(r) => {
            setCreating(false)
            toast.push(t('데이터베이스 {name} 을(를) 만들었습니다', { name: r.name }), 'success')
            reload()
            setCred({
              title: t('{name} 접속 정보', { name: r.name }),
              url: r.url,
            })
          }}
        />
      )}
      <CredentialModal info={cred} onClose={() => setCred(null)} />
    </>
  )
}

export function RedisPage() {
  const d = useFetch(() => get('/databases'), [])
  const [cred, setCred] = useState(null)
  if (d.loading) return <Spinner className="text-ink-400" />
  if (d.error) return <ErrorBox error={d.error} />
  const { redis, redis_users = [], redis_range, panel_redis_db } = d.data
  const reload = () => d.reload(true)
  return (
    <>
      <PageHeader title="Redis" help={t('프로그램별 Redis 계정과 앱별 DB 번호 배정을 관리합니다.')} />
      <div className="space-y-4">
        <RedisUsers users={redis_users} reload={reload} showCred={setCred} />
        <Section
          title={t('Redis DB 번호 배정')}
          desc={t('앱마다 Redis DB 번호를 하나씩 배정해 REDIS_URL 로 넣어 줍니다. 자동 배정 범위 DB {v} · 패널 DB {panel_redis_db} · 0–3 은 수동용으로 비워 둠', {
            v: redis_range.join('–'),
            panel_redis_db,
          })}
        >
          {redis.length === 0 ? (
            <p className="text-sm text-ink-400">{t('배정된 Redis DB가 없습니다 (앱의 데이터베이스 탭에서 배정).')}</p>
          ) : (
            <ul className="divide-y divide-ink-800">
              {redis.map((r) => (
                <li key={r.app_id} className="flex items-center gap-4 py-2.5 text-sm">
                  <span className="w-16 font-mono">DB {r.db}</span>
                  <Link to={`/apps/${r.app_id}`} className="flex-1 hover:text-signal">
                    {r.app_name}
                  </Link>
                  <span className="text-xs text-ink-400">{t('키 {n}개', { n: r.keys ?? '?' })}</span>
                </li>
              ))}
            </ul>
          )}
        </Section>
      </div>
      <CredentialModal info={cred} onClose={() => setCred(null)} />
    </>
  )
}

export function BackupsPage() {
  const list = useFetch(() => get('/backups?limit=300'), [])
  useInterval(() => list.reload(true), 3000, !!list.data?.some((b) => b.status === 'running'))
  return (
    <>
      <PageHeader
        title={t('백업')}
        help={
          <>
            {t('백업 예약을 추가·수정·삭제하고, 만들어진 백업 파일을 내려받거나 복원합니다 (/var/lib/rpanel/backups).')}
            <br />
            {t('예약의 “지금 실행”이나 앱·데이터베이스 화면의 “지금 백업”을 누르면 여기에 나타납니다.')}
          </>
        }
      />
      <div className="space-y-4">
        <BackupJobs onRan={() => setTimeout(() => list.reload(true), 500)} />
        <ErrorBox error={list.error} />
        {!list.loading && !list.data?.length ? (
          <Empty title={t('백업 파일이 없습니다')} />
        ) : (
          <BackupTable rows={list.data} loading={list.loading} onChanged={() => list.reload(true)} showApp />
        )}
      </div>
    </>
  )
}
