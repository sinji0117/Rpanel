import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { post } from '../lib/api'
import { CopyButton, ErrorBox, Field, Modal } from './ui'

export const SITE_TYPES = [
  { id: 'rust', label: 'Rust', desc: '빌드한 실행 파일을 올리면 서비스로 실행' },
  { id: 'python', label: 'Python', desc: 'Flask·FastAPI·Django 등 — 가상환경 자동' },
  { id: 'html', label: 'HTML', desc: '정적 사이트 — 파일을 올리면 바로 서비스' },
  { id: 'proxy', label: '리버스 프록시', desc: '다른 주소(내부 서버·포트)로 연결' },
]

export const DEFAULT_START = { rust: './app', python: 'python app.py' }

/** 서버와 같은 규칙: www.beripick.co.kr → beripick */
export function userFromDomain(domain) {
  const d = (domain || '').trim().toLowerCase().split(/[\s,]+/)[0] || ''
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
      <Modal open={open} onClose={close} title="사이트를 만들었습니다">
        <div className="space-y-4 text-sm">
          <p>
            <b className="font-mono">{done.app.domains.split(' ')[0]}</b> 설정을 적용하고 있습니다.
          </p>
          {done.app.kind === 'rust' && (
            <p className="rounded-lg border border-signal/30 bg-signal/5 p-3 text-xs leading-relaxed text-ink-300" data-testid="rust-next">
              다음 단계: 내 컴퓨터에서 <span className="kbd">cargo build --release</span> → <span className="font-mono">target/release/</span> 의 실행 파일을{' '}
              <b>파일</b> 탭(또는 SFTP)으로 <span className="font-mono">{done.app.start_cmd?.split(' ')[0]?.replace('./', '') || 'app'}</span> 이름으로 올리고 <b>적용</b>을 누르세요.
              <br />
              서버와 같은 리눅스용(보통 x86_64)으로 빌드해야 합니다 — Mac·Windows 에서는 <span className="font-mono">cross</span> 또는 <span className="font-mono">--target x86_64-unknown-linux-gnu</span> 를 쓰세요.
            </p>
          )}
          {done.app.kind === 'python' && (
            <p className="rounded-lg border border-signal/30 bg-signal/5 p-3 text-xs leading-relaxed text-ink-300" data-testid="python-next">
              예시 <span className="font-mono">app.py</span> 가 실행 중입니다. 내 프로그램 파일과 <span className="font-mono">requirements.txt</span> 를 <b>파일</b> 탭(또는 SFTP)으로 올리고, 필요하면 설정 탭에서 시작 명령을
              바꾼 뒤 <b>적용</b>을 누르세요. 적용할 때마다 <span className="font-mono">.venv</span> 가상환경에 패키지를 설치합니다.
            </p>
          )}
          {done.site_user && (
            <div className="rounded-lg border border-ink-700 bg-ink-950 p-4">
              <p className="mb-2 text-xs text-ink-400">SSH · SFTP 접속 정보 (파일 업로드용)</p>
              <dl className="grid grid-cols-[90px_1fr] gap-y-1.5 font-mono text-xs">
                <dt className="text-ink-400">호스트</dt>
                <dd>{host}</dd>
                <dt className="text-ink-400">포트</dt>
                <dd>22</dd>
                <dt className="text-ink-400">사용자</dt>
                <dd>{done.site_user}</dd>
                <dt className="text-ink-400">비밀번호</dt>
                <dd className="flex items-center gap-2">
                  {done.generated_password ? (
                    <>
                      <span data-testid="generated-password">{done.generated_password}</span>
                      <CopyButton text={done.generated_password} />
                    </>
                  ) : (
                    <span className="font-sans text-ink-400">입력한 비밀번호</span>
                  )}
                </dd>
                <dt className="text-ink-400">문서 루트</dt>
                <dd className="break-all">{done.app.site_root}</dd>
              </dl>
              {done.generated_password && <p className="mt-3 text-xs text-warn">비밀번호는 지금만 보입니다. 잊으면 SSH·SFTP 탭에서 바꿀 수 있습니다.</p>}
            </div>
          )}
          <div className="flex justify-end gap-2">
            {done.deployment_id && (
              <Link to={`/deployments/${done.deployment_id}`} className="btn-ghost" onClick={close}>
                적용 로그
              </Link>
            )}
            <button
              className="btn-primary"
              onClick={() => {
                close()
                nav(`/apps/${done.app.id}`)
              }}
            >
              사이트로 이동
            </button>
          </div>
        </div>
      </Modal>
    )
  }

  return (
    <Modal open={open} onClose={close} title="사이트 추가" wide>
      <form className="space-y-4" onSubmit={submit}>
        <div>
          <span className="label">종류</span>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4" role="radiogroup" aria-label="사이트 종류">
            {SITE_TYPES.map((t) => (
              <button
                type="button"
                key={t.id}
                role="radio"
                aria-checked={type === t.id}
                onClick={() => setType(t.id)}
                className={`rounded-lg border px-3 py-2.5 text-left transition ${type === t.id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
              >
                <span className="block text-sm font-medium">{t.label}</span>
                <span className="mt-0.5 block text-[11px] leading-snug text-ink-400">{t.desc}</span>
              </button>
            ))}
          </div>
          <p className="mt-2 text-xs text-ink-400">
            Git 저장소에서 서버가 직접 빌드하게 하려면(cargo build · npm run build){' '}
            <Link to="/projects" className="text-signal hover:underline" onClick={close}>
              프로젝트
            </Link>
            에서 만듭니다.
          </p>
        </div>

        <Field label="도메인" htmlFor="site-domains" hint="첫 번째가 대표 도메인 (폴더 이름). www 도 함께 넣으세요 — 예: example.com www.example.com">
          <input id="site-domains" className="input font-mono" required value={v.domains} onChange={set('domains')} placeholder="example.com www.example.com" />
        </Field>

        {type !== 'proxy' && (
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="사이트 유저" htmlFor="site-user" hint="리눅스 계정 — 파일 소유자, SSH·SFTP 로그인 이름">
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
            <Field label="비밀번호" htmlFor="site-pw" hint="비우면 자동 생성해서 한 번 보여줍니다">
              <div className="flex gap-2">
                <input id="site-pw" className="input font-mono" type="text" autoComplete="new-password" value={v.password} onChange={set('password')} minLength={10} />
                <button type="button" className="btn-ghost shrink-0 px-2.5 text-xs whitespace-nowrap" onClick={() => setV((s) => ({ ...s, password: genPassword() }))}>
                  생성
                </button>
              </div>
            </Field>
          </div>
        )}

        {type === 'rust' && (
          <Field label="시작 명령" htmlFor="site-start" hint="문서 루트에 올린 실행 파일 — 예: ./app (PORT 환경변수의 포트로 127.0.0.1 에서 열어야 합니다)">
            <input id="site-start" className="input font-mono" required value={v.start_cmd} onChange={set('start_cmd')} />
          </Field>
        )}
        {type === 'python' && (
          <Field
            label="시작 명령"
            htmlFor="site-start"
            hint="예: python app.py · gunicorn -b 127.0.0.1:$PORT app:app · uvicorn main:app --host 127.0.0.1 --port $PORT — requirements.txt 는 적용할 때 자동 설치"
          >
            <input id="site-start" className="input font-mono" required value={v.start_cmd} onChange={set('start_cmd')} />
          </Field>
        )}
        {type === 'proxy' && (
          <Field label="대상 URL" htmlFor="site-target" hint="이 도메인으로 온 요청을 그대로 넘길 주소">
            <input id="site-target" className="input font-mono" required value={v.proxy_target} onChange={set('proxy_target')} placeholder="http://127.0.0.1:3000" />
          </Field>
        )}

        <ErrorBox error={error} />
        <div className="flex justify-end">
          <button className="btn-primary" disabled={busy}>
            {busy ? '만드는 중…' : '사이트 만들기'}
          </button>
        </div>
      </form>
    </Modal>
  )
}
