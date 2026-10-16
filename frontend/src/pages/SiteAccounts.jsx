import { useEffect, useState } from 'react'
import { genPassword } from '../components/CreateSiteModal'
import { CopyButton, ErrorBox, Field, Modal, Section, Spinner } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { t } from '../i18n'
import { del, get, post, put } from '../lib/api'
import { dateTime } from '../lib/format'
import { useFetch } from '../lib/useFetch'

const ACCESS = {
  ssh: { label: 'SSH + SFTP', desc: '터미널 접속과 파일 전송 모두' },
  sftp: { label: 'SFTP 전용', desc: '파일 전송만 (터미널 접속 불가)' },
}

/** 접속 정보 (만든 직후·비밀번호 변경 직후 한 번 크게 보여 줌) */
function LoginInfo({ info, onClose }) {
  const host = window.location.hostname
  if (!info) return null
  return (
    <Modal open onClose={onClose} title={t('{user} 접속 정보', { user: info.username })}>
      <div className="space-y-3 text-sm">
        <dl className="grid grid-cols-[90px_1fr] gap-y-1.5 rounded-lg border border-ink-700 bg-ink-950 p-4 font-mono text-xs">
          <dt className="font-sans text-ink-400">{t('호스트')}</dt>
          <dd>{host}</dd>
          <dt className="font-sans text-ink-400">{t('포트')}</dt>
          <dd>22</dd>
          <dt className="font-sans text-ink-400">{t('사용자')}</dt>
          <dd className="flex items-center gap-2">
            {info.username} <CopyButton text={info.username} />
          </dd>
          {info.password && (
            <>
              <dt className="font-sans text-ink-400">{t('비밀번호')}</dt>
              <dd className="flex items-center gap-2">
                <span data-testid="account-password">{info.password}</span> <CopyButton text={info.password} />
              </dd>
            </>
          )}
        </dl>
        {info.password && <p className="text-xs text-warn">{t('비밀번호는 지금만 보입니다. 잊으면 “비밀번호”로 새로 만들 수 있습니다.')}</p>}
        <pre className="overflow-x-auto rounded-lg border border-ink-700 bg-ink-950 p-3 font-mono text-xs text-ink-300">{`sftp ${info.username}@${host}${info.access === 'ssh' ? `\nssh ${info.username}@${host}` : ''}`}</pre>
        <div className="flex justify-end">
          <button className="btn-primary" onClick={onClose}>
            {t('확인')}
          </button>
        </div>
      </div>
    </Modal>
  )
}

function AccessPicker({ value, onChange }) {
  return (
    <div className="grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label={t('접속 방식')}>
      {Object.entries(ACCESS).map(([id, a]) => (
        <button
          key={id}
          type="button"
          role="radio"
          aria-checked={value === id}
          onClick={() => onChange(id)}
          className={`rounded-lg border px-3 py-2 text-left transition ${value === id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
        >
          <span className="block text-sm font-medium">{t(a.label)}</span>
          <span className="block text-[11px] text-ink-400">{t(a.desc)}</span>
        </button>
      ))}
    </div>
  )
}

function KeysField({ id, value, onChange }) {
  return (
    <Field label={t('SSH 공개키 (선택)')} htmlFor={id} hint={t('한 줄에 하나씩 — 내 컴퓨터의 ~/.ssh/id_ed25519.pub 내용. 키로 접속하면 비밀번호가 필요 없습니다.')}>
      <textarea id={id} className="input h-24 font-mono text-[11px]" placeholder="ssh-ed25519 AAAA… me@laptop" value={value} onChange={(e) => onChange(e.target.value)} />
    </Field>
  )
}

function CreateAccount({ app, onClose, onCreated }) {
  const [v, setV] = useState({ username: '', access: 'sftp', password: '', ssh_keys: '', note: '' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const set = (k) => (e) => setV((s) => ({ ...s, [k]: e?.target ? e.target.value : e }))
  return (
    <Modal open onClose={onClose} title={t('사이트 계정 추가')} wide>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            const r = await post(`/apps/${app.id}/accounts`, { ...v, password: v.password || undefined, ssh_keys: v.ssh_keys || undefined, note: v.note || undefined })
            onCreated(r)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('계정 이름')} htmlFor="acc-name" hint={t('소문자로 시작, 소문자·숫자·- 3~32자')}>
            <input
              id="acc-name"
              className="input font-mono"
              required
              pattern="[a-z][a-z0-9-]{2,31}"
              value={v.username}
              onChange={set('username')}
              placeholder={`${app.site_user}-dev`}
            />
          </Field>
          <Field label={t('비밀번호')} htmlFor="acc-pw" hint={t('비우면 자동 생성 (SSH 키만 넣으면 키로만 접속)')}>
            <div className="flex gap-2">
              <input id="acc-pw" className="input font-mono" autoComplete="new-password" value={v.password} onChange={set('password')} />
              <button type="button" className="btn-ghost shrink-0 px-2.5 text-xs whitespace-nowrap" onClick={() => set('password')(genPassword(16))}>
                {t('생성')}
              </button>
            </div>
          </Field>
        </div>
        <div>
          <span className="label">{t('접속 방식')}</span>
          <AccessPicker value={v.access} onChange={set('access')} />
        </div>
        <KeysField id="acc-keys" value={v.ssh_keys} onChange={set('ssh_keys')} />
        <Field label={t('메모 (선택)')} htmlFor="acc-note">
          <input id="acc-note" className="input" maxLength={200} value={v.note} onChange={set('note')} placeholder={t('예: 외주 디자이너')} />
        </Field>
        <p className="text-xs text-ink-400">{t('추가 계정은 이 사이트의 파일(htdocs·logs)을 읽고 쓸 수 있습니다. 홈 폴더의 htdocs 가 사이트 폴더로 연결됩니다.')}</p>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary" disabled={busy}>
            {busy ? t('만드는 중…') : t('계정 만들기')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

function EditAccount({ app, account, onClose, onSaved }) {
  const [v, setV] = useState({ access: account.access, enabled: account.enabled, ssh_keys: account.ssh_keys ?? '', note: account.note ?? '' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  // 예전 버전에서 기본 계정 키를 파일로만 넣어 둔 경우 그 내용을 불러옴
  useEffect(() => {
    if (!account.is_primary || account.ssh_keys) return
    get(`/apps/${app.id}/files/content?path=${encodeURIComponent('.ssh/authorized_keys')}`)
      .then((r) => r.content?.trim() && setV((s) => (s.ssh_keys ? s : { ...s, ssh_keys: r.content.trim() })))
      .catch(() => {})
  }, [app.id, account.is_primary, account.ssh_keys])
  return (
    <Modal open onClose={onClose} title={t('계정 {user} 수정', { user: account.username })} wide>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            onSaved(await put(`/apps/${app.id}/accounts/${account.id}`, v))
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <div>
          <span className="label">{t('접속 방식')}</span>
          <AccessPicker value={v.access} onChange={(access) => setV({ ...v, access })} />
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={!v.enabled} onChange={(e) => setV({ ...v, enabled: !e.target.checked })} /> {t('잠금 (접속 막기 — 사이트는 계속 동작)')}
        </label>
        <KeysField id="edit-keys" value={v.ssh_keys} onChange={(ssh_keys) => setV({ ...v, ssh_keys })} />
        <Field label={t('메모 (선택)')} htmlFor="edit-note">
          <input id="edit-note" className="input" maxLength={200} value={v.note} onChange={(e) => setV({ ...v, note: e.target.value })} />
        </Field>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary" disabled={busy}>
            {busy ? t('저장 중…') : t('저장')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

/** 사이트 계정 탭: 기본 계정 + 추가 계정 만들기·수정·잠금·비밀번호·삭제 */
export function AccountsTab({ app }) {
  const toast = useToast()
  const list = useFetch(() => get(`/apps/${app.id}/accounts`), [app.id])
  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState(null)
  const [info, setInfo] = useState(null)
  const [busy, setBusy] = useState(false)
  const host = window.location.hostname

  const act = async (fn, msg) => {
    setBusy(true)
    try {
      const r = await fn()
      if (msg) toast.push(msg, 'success')
      list.reload(true)
      return r
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-4">
      <Section
        title={t('사이트 계정 (SSH · SFTP)')}
        desc={
          <>
            {t('FileZilla·WinSCP(SFTP)나 터미널(SSH)로 사이트 파일에 접속하는 계정입니다. 개발자·디자이너마다 계정을 따로 만들고, 필요 없어지면 잠그거나 지우세요.')}
            <br />
            <br />
            {t('기본 계정은 사이트를 지울 때 함께 지워집니다. 접속만 막으려면 “잠금”을 쓰세요.')}
          </>
        }
        actions={
          <button className="btn-primary" onClick={() => setCreating(true)}>
            + {t('계정 추가')}
          </button>
        }
      >
        <p className="mb-4 text-xs text-ink-400">
          {t('호스트')} <span className="kbd">{host}</span> · {t('포트')} <span className="kbd">22</span> · {t('문서 루트')} <span className="kbd">{app.site_root}</span>
        </p>
        {list.loading ? (
          <Spinner className="text-ink-400" />
        ) : (
          <>
            <ErrorBox error={list.error} />
            <table className="w-full text-sm" aria-label={t('사이트 계정 목록')}>
              <thead className="text-left text-xs text-ink-400">
                <tr>
                  <th className="py-2 pr-3 font-medium">{t('계정')}</th>
                  <th className="px-3 py-2 font-medium">{t('접속 방식')}</th>
                  <th className="px-3 py-2 font-medium">{t('상태')}</th>
                  <th className="px-3 py-2 font-medium">{t('SSH 키')}</th>
                  <th className="py-2 pl-3" />
                </tr>
              </thead>
              <tbody className="divide-y divide-ink-800">
                {list.data?.accounts.map((a) => (
                  <tr key={a.id}>
                    <td className="py-2.5 pr-3">
                      <span className="font-mono text-xs">{a.username}</span>
                      {a.is_primary && <span className="ml-2 rounded border border-ink-600 px-1 text-[10px] text-ink-400">{t('기본')}</span>}
                      {a.note && <p className="text-[11px] text-ink-400">{a.note}</p>}
                    </td>
                    <td className="px-3 py-2.5 text-xs text-ink-300">{t(ACCESS[a.access]?.label ?? a.access)}</td>
                    <td className="px-3 py-2.5 text-xs">{a.enabled ? <span className="text-signal">{t('사용')}</span> : <span className="text-warn">{t('잠금')}</span>}</td>
                    <td className="px-3 py-2.5 text-xs text-ink-400">{a.key_count ? t('{n}개', { n: a.key_count }) : '–'}</td>
                    <td className="py-2.5 pl-3 text-right whitespace-nowrap">
                      <button className="btn-ghost mr-1.5 px-2.5 py-1 text-xs" onClick={() => setEditing(a)}>
                        {t('수정')}
                      </button>
                      <button
                        className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                        disabled={busy}
                        onClick={() =>
                          act(
                            () => put(`/apps/${app.id}/accounts/${a.id}`, { enabled: !a.enabled }),
                            a.enabled ? t('{user} 을(를) 잠갔습니다', { user: a.username }) : t('{user} 잠금을 풀었습니다', { user: a.username }),
                          )
                        }
                      >
                        {a.enabled ? t('잠금') : t('잠금 해제')}
                      </button>
                      <button
                        className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                        disabled={busy}
                        onClick={async () => {
                          const custom = window.prompt(t('{user} 의 새 비밀번호 (비우면 자동 생성)', { user: a.username }), '')
                          if (custom === null) return
                          const r = await act(() => post(`/apps/${app.id}/accounts/${a.id}/password`, { password: custom || undefined }), t('비밀번호를 바꿨습니다'))
                          if (r?.password) setInfo({ username: a.username, access: a.access, password: r.password })
                        }}
                      >
                        {t('비밀번호')}
                      </button>
                      {!a.is_primary && (
                        <button
                          className="btn-ghost px-2.5 py-1 text-xs text-red-300"
                          disabled={busy}
                          aria-label={t('{user} 계정 삭제', { user: a.username })}
                          onClick={() =>
                            window.confirm(t('계정 {user} 을(를) 삭제할까요? 그 계정의 홈 폴더도 지워집니다 (사이트 파일은 그대로).', { user: a.username })) &&
                            act(() => del(`/apps/${app.id}/accounts/${a.id}`), t('삭제했습니다'))
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
          </>
        )}
      </Section>
      {creating && (
        <CreateAccount
          app={app}
          onClose={() => setCreating(false)}
          onCreated={(r) => {
            setCreating(false)
            toast.push(t('계정 {user} 을(를) 만들었습니다', { user: r.account.username }), 'success')
            list.reload(true)
            setInfo({ username: r.account.username, access: r.account.access, password: r.password })
          }}
        />
      )}
      {editing && (
        <EditAccount
          app={app}
          account={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null)
            toast.push(t('저장했습니다'), 'success')
            list.reload(true)
          }}
        />
      )}
      <LoginInfo info={info} onClose={() => setInfo(null)} />
      <p className="text-xs text-ink-400">{list.data?.accounts[0] && t('기본 계정 생성: {at}', { at: dateTime(list.data.accounts[0].created_at) })}</p>
    </div>
  )
}
