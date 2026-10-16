import { useState } from 'react'
import { Link } from 'react-router'
import { genPassword } from '../components/CreateSiteModal'
import { ErrorBox, Field, Modal, PageHeader, Section, Spinner } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { t } from '../i18n'
import { del, get, post, put } from '../lib/api'
import { ago, dateTime } from '../lib/format'
import { useFetch } from '../lib/useFetch'

function AdminModal({ initial, onClose, onSaved }) {
  const editing = !!initial
  const [email, setEmail] = useState(initial?.email ?? '')
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  return (
    <Modal open onClose={onClose} title={editing ? t('관리자 수정') : t('관리자 추가')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            const r = editing ? await put(`/users/${initial.id}`, { email, password: password || undefined }) : await post('/users', { email, password })
            onSaved(r, password)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('이메일 (로그인 아이디)')} htmlFor="adm-email">
          <input id="adm-email" type="email" className="input" required value={email} onChange={(e) => setEmail(e.target.value)} />
        </Field>
        <Field
          label={editing ? t('새 비밀번호 (바꿀 때만)') : t('비밀번호')}
          htmlFor="adm-pw"
          hint={editing ? t('바꾸면 이 관리자의 모든 로그인이 바로 끊어집니다') : t('10자 이상')}
        >
          <div className="flex gap-2">
            <input
              id="adm-pw"
              className="input font-mono"
              autoComplete="new-password"
              minLength={10}
              required={!editing}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            <button type="button" className="btn-ghost shrink-0 px-2.5 text-xs whitespace-nowrap" onClick={() => setPassword(genPassword(16))}>
              {t('생성')}
            </button>
          </div>
        </Field>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary" disabled={busy}>
            {busy ? t('저장 중…') : editing ? t('저장') : t('추가')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

export default function AdminsPage() {
  const toast = useToast()
  const list = useFetch(() => get('/users'), [])
  const [editing, setEditing] = useState(null)

  return (
    <>
      <PageHeader
        title={t('관리자')}
        help={t('패널에 로그인할 수 있는 관리자 계정을 추가·수정·삭제합니다. 모든 관리자는 같은 권한을 가집니다.')}
        actions={
          <button className="btn-primary" onClick={() => setEditing({})}>
            + {t('관리자 추가')}
          </button>
        }
      />
      <div className="space-y-4">
        <Section title={t('관리자 계정')}>
          {list.loading ? <Spinner className="text-ink-400" /> : <ErrorBox error={list.error} />}
          {list.data && (
            <table className="w-full text-sm" aria-label={t('관리자 목록')}>
              <thead className="text-left text-xs text-ink-400">
                <tr>
                  <th className="py-2 pr-3 font-medium">{t('이메일')}</th>
                  <th className="px-3 py-2 font-medium">{t('마지막 로그인')}</th>
                  <th className="px-3 py-2 font-medium">{t('만든 날')}</th>
                  <th className="py-2 pl-3" />
                </tr>
              </thead>
              <tbody className="divide-y divide-ink-800">
                {list.data.users.map((u) => (
                  <tr key={u.id}>
                    <td className="py-2.5 pr-3">
                      {u.email}
                      {u.id === list.data.me && <span className="ml-2 rounded border border-signal/40 px-1.5 text-[11px] text-signal">{t('나')}</span>}
                    </td>
                    <td className="px-3 py-2.5 text-xs text-ink-400" title={dateTime(u.last_login_at)}>
                      {u.last_login_at ? ago(u.last_login_at) : t('없음')}
                    </td>
                    <td className="px-3 py-2.5 text-xs text-ink-400">{dateTime(u.created_at)}</td>
                    <td className="py-2.5 pl-3 text-right whitespace-nowrap">
                      <button className="btn-ghost mr-1.5 px-2.5 py-1 text-xs" onClick={() => setEditing(u)}>
                        {t('수정')}
                      </button>
                      {u.id !== list.data.me && (
                        <button
                          className="btn-ghost px-2.5 py-1 text-xs text-red-300"
                          aria-label={t('{email} 삭제', { email: u.email })}
                          onClick={async () => {
                            if (!window.confirm(t('관리자 {email} 을(를) 삭제할까요? 바로 로그아웃됩니다.', { email: u.email }))) return
                            try {
                              await del(`/users/${u.id}`)
                              toast.push(t('삭제했습니다'), 'success')
                              list.reload(true)
                            } catch (e) {
                              toast.push(e.message, 'error')
                            }
                          }}
                        >
                          {t('삭제')}
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Section>

        <p className="text-xs text-ink-400">
          <Link to="/logs?tab=audit" className="hover:text-ink-100">
            {t('관리자 활동 기록은 로그 메뉴에서 볼 수 있습니다 →')}
          </Link>
        </p>
      </div>
      {editing && (
        <AdminModal
          initial={editing.id ? editing : null}
          onClose={() => setEditing(null)}
          onSaved={(r) => {
            toast.push(editing.id ? t('저장했습니다') : t('관리자 {email} 을(를) 추가했습니다', { email: r.email }), 'success')
            setEditing(null)
            list.reload(true)
          }}
        />
      )}
    </>
  )
}
