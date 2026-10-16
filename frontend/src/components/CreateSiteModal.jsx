import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { post } from '../lib/api'
import { CopyButton, ErrorBox, Field, Modal } from './ui'
import { t, tx } from '../i18n'

export const SITE_TYPES = [
  { id: 'rust', label: 'Rust', desc: '빌드한 실행 파일을 올리면 서비스로 실행' },
  { id: 'python', label: 'Python', desc: 'Flask·FastAPI·Django 등 — 가상환경 자동' },
  { id: 'html', label: 'HTML', desc: '정적 사이트 — 파일을 올리면 바로 서비스' },
  { id: 'proxy', label: '리버스 프록시', desc: '다른 주소(내부 서버·포트)로 연결' },
]

export const DEFAULT_START = { rust: './app', python: 'python app.py' }

/** 서버와 같은 규칙: www.beripick.co.kr → beripick */
export function userFromDomain(domain) {
  const d =
    (domain || '')
      .trim()
      .toLowerCase()
      .split(/[\s,]+/)[0] || ''
  const first = d.replace(/^www\./, '').split('.')[0] || ''
  let u = first.replace(/[^a-z0-9]/g, '-').replace(/^-+|-+$/g, '')
  if (!u) return ''
  if (!/^[a-z]/.test(u)) u = `site-${u}`
  if (u.length < 3) u = `${u}-site`
  return u.slice(0, 28).replace(/-+$/, '')
}

export function genPassword(len = 16) {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789'
  const a = new Uint32Array(len)
  crypto.getRandomValues(a)
  return Array.from(a, (x) => chars[x % chars.length]).join('')
}

export default function CreateSiteModal({ open, onClose }) {
  const nav = useNavigate()
  const [type, setTypeRaw] = useState('rust')
  const [v, setV] = useState({ domains: '', site_user: '', password: '', start_cmd: './app', proxy_target: '' })
  // 종류를 바꾸면 (직접 고치지 않은) 시작 명령도 그 종류의 기본값으로
  const setType = (t) => {
    setTypeRaw(t)
    setV((s) => (Object.values(DEFAULT_START).includes(s.start_cmd) && DEFAULT_START[t] ? { ...s, start_cmd: DEFAULT_START[t] } : s))
  }
  const [userTouched, setUserTouched] = useState(false)
  const [advanced, setAdvanced] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [done, setDone] = useState(null)

  const set = (k) => (e) => {
    const val = e.target.value
    setV((s) => {
      const n = { ...s, [k]: val }
      if (k === 'domains' && !userTouched) n.site_user = userFromDomain(val)
      return n
    })
  }

  const submit = async (e) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const body = { type, domains: v.domains }
      if (type !== 'proxy') {
        if (v.site_user) body.site_user = v.site_user
        if (v.password) body.password = v.password
      }
      if (type === 'rust' || type === 'python') body.start_cmd = v.start_cmd
      if (type === 'proxy') body.proxy_target = v.proxy_target
      const r = await post('/sites', body)
      setDone(r)
    } catch (err) {
      setError(err)
    } finally {
      setBusy(false)
    }
  }

  const close = () => {
    setDone(null)
    setError(null)
    onClose()
  }

  if (done) {
    const host = window.location.hostname
    return (
      <Modal open={open} onClose={close} title={t('사이트를 만들었습니다')}>
        <div className="space-y-4 text-sm">
          <p>
            <b className="font-mono">{done.app.domains.split(' ')[0]}</b> {t('설정을 적용하고 있습니다.')}
          </p>
          {done.app.kind === 'rust' && (
            <p className="rounded-lg border border-signal/30 bg-signal/5 p-3 text-xs leading-relaxed text-ink-300" data-testid="rust-next">
              {tx('다음 단계: 내 컴퓨터에서 {cmd} → {dir} 의 실행 파일을 {files} 탭(또는 SFTP)으로 {bin} 이름으로 올리고 {apply}을 누르세요.', {
                cmd: <span className="kbd">cargo build --release</span>,
                dir: <span className="font-mono">target/release/</span>,
                files: <b>{t('파일')}</b>,
                bin: <span className="font-mono">{done.app.start_cmd?.split(' ')[0]?.replace('./', '') || 'app'}</span>,
                apply: <b>{t('적용')}</b>,
              })}
              <br />
              {tx('서버와 같은 리눅스용(보통 x86_64)으로 빌드해야 합니다 — Mac·Windows 에서는 {cross} 또는 {target} 를 쓰세요.', {
                cross: <span className="font-mono">cross</span>,
                target: <span className="font-mono">--target x86_64-unknown-linux-gnu</span>,
              })}
            </p>
          )}
          {done.app.kind === 'python' && (
            <p className="rounded-lg border border-signal/30 bg-signal/5 p-3 text-xs leading-relaxed text-ink-300" data-testid="python-next">
              {tx(
                '예시 {app} 가 실행 중입니다. 내 프로그램 파일과 {req} 를 {files} 탭(또는 SFTP)으로 올리고, 필요하면 설정 탭에서 시작 명령을 바꾼 뒤 {apply}을 누르세요. 적용할 때마다 {venv} 가상환경에 패키지를 설치합니다.',
                {
                  app: <span className="font-mono">app.py</span>,
                  req: <span className="font-mono">requirements.txt</span>,
                  files: <b>{t('파일')}</b>,
                  apply: <b>{t('적용')}</b>,
                  venv: <span className="font-mono">.venv</span>,
                },
              )}
            </p>
          )}
          {done.site_user && (
            <div className="rounded-lg border border-ink-700 bg-ink-950 p-4">
              <p className="mb-2 text-xs text-ink-400">{t('SSH · SFTP 접속 정보 (파일 업로드용)')}</p>
              <dl className="grid grid-cols-[90px_1fr] gap-y-1.5 font-mono text-xs">
                <dt className="text-ink-400">{t('호스트')}</dt>
                <dd>{host}</dd>
                <dt className="text-ink-400">{t('포트')}</dt>
                <dd>22</dd>
                <dt className="text-ink-400">{t('사용자')}</dt>
                <dd>{done.site_user}</dd>
                <dt className="text-ink-400">{t('비밀번호')}</dt>
                <dd className="flex items-center gap-2">
                  {done.generated_password ? (
                    <>
                      <span data-testid="generated-password">{done.generated_password}</span>
                      <CopyButton text={done.generated_password} />
                    </>
                  ) : (
                    <span className="font-sans text-ink-400">{t('입력한 비밀번호')}</span>
                  )}
                </dd>
                <dt className="text-ink-400">{t('문서 루트')}</dt>
                <dd className="break-all">{done.app.site_root}</dd>
              </dl>
              {done.generated_password && <p className="mt-3 text-xs text-warn">{t('비밀번호는 지금만 보입니다. 잊으면 SSH·SFTP 탭에서 바꿀 수 있습니다.')}</p>}
            </div>
          )}
          <div className="flex justify-end gap-2">
            {done.deployment_id && (
              <Link to={`/deployments/${done.deployment_id}`} className="btn-ghost" onClick={close}>
                {t('적용 로그')}
              </Link>
            )}
            <button
              className="btn-primary"
              onClick={() => {
                close()
                nav(`/apps/${done.app.id}`)
              }}
            >
              {t('사이트로 이동')}
            </button>
          </div>
        </div>
      </Modal>
    )
  }

  return (
    <Modal open={open} onClose={close} title={t('사이트 추가')} wide>
      <form className="space-y-5" onSubmit={submit}>
        <Field label={t('1. 도메인')} htmlFor="site-domains" hint={t('www 도 함께 넣으세요 — 예: example.com www.example.com (첫 번째가 대표 도메인)')}>
          <input
            id="site-domains"
            className="input py-2.5 font-mono text-base"
            required
            autoFocus
            value={v.domains}
            onChange={set('domains')}
            placeholder="example.com www.example.com"
          />
        </Field>

        <div>
          <span className="label">{t('2. 종류')}</span>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4" role="radiogroup" aria-label={t('사이트 종류')}>
            {SITE_TYPES.map((st) => (
              <button
                type="button"
                key={st.id}
                role="radio"
                aria-checked={type === st.id}
                onClick={() => setType(st.id)}
                className={`rounded-lg border px-3 py-2.5 text-left transition ${type === st.id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
              >
                <span className="block text-sm font-medium">{t(st.label)}</span>
                <span className="mt-0.5 block text-[11px] leading-snug text-ink-400">{t(st.desc)}</span>
              </button>
            ))}
          </div>
        </div>

        {type === 'proxy' && (
          <Field label={t('3. 대상 URL')} htmlFor="site-target" hint={t('이 도메인으로 온 요청을 그대로 넘길 주소')}>
            <input id="site-target" className="input font-mono" required value={v.proxy_target} onChange={set('proxy_target')} placeholder="http://127.0.0.1:3000" />
          </Field>
        )}

        {type !== 'proxy' && (
          <p className="rounded-lg border border-ink-700 bg-ink-950 px-3 py-2 text-xs text-ink-300" data-testid="site-summary">
            {tx('사이트 계정 {user} 과(와) 폴더가 자동으로 만들어집니다.', { user: <b className="font-mono">{v.site_user || '…'}</b> })}
            {(type === 'rust' || type === 'python') && <> {tx('시작 명령: {cmd}', { cmd: <span className="font-mono">{v.start_cmd}</span> })}</>}{' '}
            {tx('나중에 사이트의 {tab} 탭에서 계정을 더 만들거나 바꿀 수 있습니다.', { tab: <b>{t('사이트 계정')}</b> })}
          </p>
        )}

        {type !== 'proxy' && (
          <details className="rounded-lg border border-ink-700 px-4 py-3" open={advanced} onToggle={(e) => setAdvanced(e.currentTarget.open)}>
            <summary className="cursor-pointer text-sm text-ink-300">{t('고급 설정 (선택) — 계정 이름·비밀번호·시작 명령')}</summary>
            <div className="mt-4 space-y-4">
              <div className="grid gap-4 sm:grid-cols-2">
                <Field label={t('사이트 유저')} htmlFor="site-user" hint={t('리눅스 계정 — 파일 소유자, SSH·SFTP 로그인 이름')}>
                  <input
                    id="site-user"
                    className="input font-mono"
                    value={v.site_user}
                    onChange={(e) => {
                      setUserTouched(true)
                      set('site_user')(e)
                    }}
                    pattern="[a-z][a-z0-9-]{2,31}"
                    placeholder="example"
                  />
                </Field>
                <Field label={t('비밀번호')} htmlFor="site-pw" hint={t('비우면 자동 생성해서 한 번 보여줍니다')}>
                  <div className="flex gap-2">
                    <input id="site-pw" className="input font-mono" type="text" autoComplete="new-password" value={v.password} onChange={set('password')} minLength={10} />
                    <button type="button" className="btn-ghost shrink-0 px-2.5 text-xs whitespace-nowrap" onClick={() => setV((s) => ({ ...s, password: genPassword() }))}>
                      {t('생성')}
                    </button>
                  </div>
                </Field>
              </div>
              {type === 'rust' && (
                <Field label={t('시작 명령')} htmlFor="site-start" hint={t('문서 루트에 올린 실행 파일 — 예: ./app (PORT 환경변수의 포트로 127.0.0.1 에서 열어야 합니다)')}>
                  <input id="site-start" className="input font-mono" required value={v.start_cmd} onChange={set('start_cmd')} />
                </Field>
              )}
              {type === 'python' && (
                <Field
                  label={t('시작 명령')}
                  htmlFor="site-start"
                  hint={t('예: python app.py · gunicorn -b 127.0.0.1:$PORT app:app · uvicorn main:app --host 127.0.0.1 --port $PORT — requirements.txt 는 적용할 때 자동 설치')}
                >
                  <input id="site-start" className="input font-mono" required value={v.start_cmd} onChange={set('start_cmd')} />
                </Field>
              )}
            </div>
          </details>
        )}

        <p className="text-xs text-ink-400">
          {tx('Git 저장소에서 서버가 직접 빌드하게 하려면(cargo build · pip · npm run build) {link}에서 만듭니다.', {
            link: (
              <Link to="/projects" className="text-signal hover:underline" onClick={close}>
                {t('프로젝트')}
              </Link>
            ),
          })}
        </p>

        <ErrorBox error={error} />
        <div className="flex justify-end">
          <button className="btn-primary" disabled={busy}>
            {busy ? t('만드는 중…') : t('사이트 만들기')}
          </button>
        </div>
      </form>
    </Modal>
  )
}
