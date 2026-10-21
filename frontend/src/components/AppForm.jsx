import { useState } from 'react'
import { Field } from './ui'
import { t } from '../i18n'
import { isServiceKind } from '../lib/kinds'

/** 새 앱: 먼저 백엔드·프런트엔드를 고르고, 백엔드는 언어 · 프런트엔드는 화면 용도를 고름 */
export const ROLES = [
  { id: 'back', label: '백엔드 (API 서버)', desc: 'Rust·Python·Node.js·Java 로 만든 API·웹 서버. 포트를 받아 서비스로 실행합니다', icon: '⚙' },
  { id: 'front', label: '프런트엔드 (웹 화면)', desc: 'React·Vue 등을 npm run build 로 빌드해 정적 파일로 서비스합니다', icon: '▣' },
]

/** 프런트엔드(Static) 앱의 화면 용도 — 빌드 방식은 같고, 프로젝트 화면에서 묶는 곳과 도메인만 다름 */
export const SCREENS = [
  { id: 'front', label: '사용자 화면', desc: '방문자가 보는 사이트 (예: example.com)' },
  { id: 'admin', label: '관리자 화면', desc: '운영자용 화면, 보통 다른 도메인 (예: admin.example.com)' },
]

/** 'back' | 'front' | 'admin' — app_role 이 없던 옛 앱은 이름·도메인으로 추정 */
export function roleOfApp(a) {
  if (!a?.kind || isServiceKind(a.kind)) return 'back'
  if (a.app_role === 'admin' || a.app_role === 'front') return a.app_role
  return /admin|관리/i.test(`${a.name ?? ''} ${a.slug ?? ''} ${a.domains ?? ''}`) ? 'admin' : 'front'
}

export const KINDS = [
  { id: 'rust', label: 'Rust', desc: 'cargo build → systemd 서비스 (Axum 등)' },
  { id: 'python', label: 'Python', desc: 'venv + requirements.txt → systemd 서비스' },
  { id: 'node', label: 'Node.js', desc: 'npm ci → npm run build → systemd 서비스 (Express·Next.js 등)' },
  { id: 'java', label: 'Java', desc: 'Maven·Gradle 로 jar 빌드 → systemd 서비스 (Spring Boot 등)' },
  { id: 'static', label: 'Static', desc: 'npm run build → Nginx 정적 (React·Vite)' },
]

/** https://github.com/owner/repo(.git) → git@github.com:owner/repo.git (변환할 수 없으면 null) */
export function sshUrl(url) {
  const m = /^https?:\/\/(?:[^@/]+@)?([^/:]+)\/(.+?)\/?$/.exec((url ?? '').trim())
  if (!m || !m[2].includes('/')) return null
  return `git@${m[1]}:${m[2].replace(/\.git$/, '')}.git`
}

export function slugify(name) {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40)
    .replace(/-+$/, '')
}

export function emptyApp() {
  return {
    name: '',
    slug: '',
    kind: 'rust',
    git_url: '',
    git_branch: 'main',
    root_dir: '',
    build_cmd: '',
    start_cmd: '',
    binary_name: '',
    output_dir: 'dist',
    frontend_dir: '',
    domains: '',
    api_proxy_port: '',
    health_path: '/',
  }
}

/** 서버로 보낼 payload (빈 값 정리) */
export function toPayload(v) {
  const out = { ...v }
  delete out.role
  out.api_proxy_port = v.api_proxy_port === '' || v.api_proxy_port == null ? null : Number(v.api_proxy_port)
  for (const k of ['slug', 'root_dir', 'build_cmd', 'start_cmd', 'binary_name', 'frontend_dir']) {
    if (typeof out[k] === 'string' && !out[k].trim()) out[k] = null
  }
  return out
}

const BUILD_PLACEHOLDER = {
  rust: 'cargo build --release',
  python: '(선택) python manage.py collectstatic --noinput',
  static: 'npm run build',
  node: 'npm run build --if-present',
  java: '(자동) ./mvnw package 또는 ./gradlew build',
}

const START_PLACEHOLDER = {
  python: 'gunicorn -b 127.0.0.1:$PORT app:app',
  node: 'npm start',
  java: 'java $JAVA_OPTS -jar app.jar',
}

const START_HINT = {
  python: '가상환경(.venv)이 켜진 상태로 실행 · PORT 환경변수의 포트로 127.0.0.1 에서 열어야 합니다',
  node: '릴리스 폴더(빌드한 저장소 + node_modules)에서 실행 · PORT 환경변수의 포트로 127.0.0.1 에서 열어야 합니다',
  java: '빌드한 jar 를 app.jar 로 복사해 실행 · 메모리 등은 환경변수 JAVA_OPTS (예: -Xmx512m) · PORT 환경변수의 포트로 127.0.0.1 에서 열어야 합니다',
}

export default function AppForm({ initial, mode = 'create', onSubmit, submitting, backends = [] }) {
  const [v, setV] = useState(() => {
    const init = { ...emptyApp(), ...normalize(initial) }
    // 프로젝트 화면의 '+ 추가' 는 role 로 넘어옴 (admin → 프런트엔드 + 관리자 화면)
    const r = initial?.role ?? roleOfApp(initial)
    if (init.kind === 'static' || r !== 'back') init.app_role = r === 'admin' ? 'admin' : init.app_role || 'front'
    return init
  })
  const role = v.kind === 'static' ? 'front' : 'back'
  const setRole = (r) => setV((s) => ({ ...s, kind: r === 'back' ? (s.kind === 'static' ? 'rust' : s.kind) : 'static', app_role: r === 'back' ? '' : s.app_role || 'front' }))
  const [slugTouched, setSlugTouched] = useState(mode === 'edit')
  const set = (k) => (e) => {
    const val = e.target.value
    setV((s) => {
      const next = { ...s, [k]: val }
      if (k === 'name' && !slugTouched) next.slug = slugify(val)
      return next
    })
  }
  const isService = v.kind !== 'static'

  return (
    <form
      className="space-y-4"
      onSubmit={(e) => {
        e.preventDefault()
        onSubmit(toPayload(v))
      }}
    >
      {mode === 'create' && (
        <div className="space-y-3">
          <div>
            <span className="label">{t('무엇을 만드나요?')}</span>
            <div className="grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label={t('앱 역할')}>
              {ROLES.map((r) => (
                <button
                  type="button"
                  key={r.id}
                  role="radio"
                  aria-checked={role === r.id}
                  data-testid={`role-${r.id}`}
                  onClick={() => {
                    setRole(r.id)
                    if (r.id === 'front' && !v.api_proxy_port && backends.length === 1) setV((s) => ({ ...s, api_proxy_port: String(backends[0].port ?? '') }))
                  }}
                  className={`rounded-lg border px-3 py-3 text-left transition ${role === r.id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
                >
                  <span className="flex items-center gap-2 text-sm font-medium">
                    <span aria-hidden="true" className="text-ink-400">
                      {r.icon}
                    </span>
                    {t(r.label)}
                  </span>
                  <span className="mt-1 block text-[11px] leading-snug text-ink-400">{t(r.desc)}</span>
                </button>
              ))}
            </div>
          </div>
          {role === 'back' ? (
            <div>
              <span className="label">{t('언어')}</span>
              <div className="grid grid-cols-2 gap-2" role="radiogroup" aria-label={t('앱 종류')}>
                {KINDS.filter((k) => k.id !== 'static').map((k) => (
                  <button
                    type="button"
                    key={k.id}
                    role="radio"
                    aria-checked={v.kind === k.id}
                    onClick={() => setV((s) => ({ ...s, kind: k.id }))}
                    className={`rounded-lg border px-3 py-2.5 text-left transition ${v.kind === k.id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
                  >
                    <span className="block text-sm font-medium">{k.label}</span>
                    <span className="mt-0.5 block text-[11px] leading-snug text-ink-400">{t(k.desc)}</span>
                  </button>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      )}

      {v.kind === 'static' && (
        <div>
          <span className="label">{t('화면 용도')}</span>
          <div className="grid grid-cols-2 gap-2" role="radiogroup" aria-label={t('화면 용도')}>
            {SCREENS.map((sc) => (
              <button
                type="button"
                key={sc.id}
                role="radio"
                aria-checked={(v.app_role || 'front') === sc.id}
                onClick={() => setV((s) => ({ ...s, app_role: sc.id }))}
                className={`rounded-lg border px-3 py-2.5 text-left transition ${(v.app_role || 'front') === sc.id ? 'border-signal/60 bg-signal/5' : 'border-ink-700 hover:border-ink-600'}`}
              >
                <span className="block text-sm font-medium">{t(sc.label)}</span>
                <span className="mt-0.5 block text-[11px] leading-snug text-ink-400">{t(sc.desc)}</span>
              </button>
            ))}
          </div>
          <p className="mt-1.5 text-xs text-ink-400">
            {t('두 용도 모두 npm ci → npm run build 로 빌드해 Nginx 로 서비스합니다. 용도는 프로젝트 화면에서 묶는 곳만 정하며, 나중에 설정에서 바꿀 수 있습니다.')}
          </p>
        </div>
      )}

      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t('이름')} htmlFor="app-name">
          <input id="app-name" className="input" required value={v.name} onChange={set('name')} placeholder="Beripick API" />
        </Field>
        <Field label="Slug" htmlFor="app-slug" hint={mode === 'edit' ? t('생성 후 변경 불가') : t('서비스·경로 이름 (소문자, 숫자, -)')}>
          <input
            id="app-slug"
            className="input font-mono"
            value={v.slug}
            disabled={mode === 'edit'}
            onChange={(e) => {
              setSlugTouched(true)
              set('slug')(e)
            }}
            pattern="[a-z0-9][a-z0-9-]{0,39}"
            placeholder="beripick-api"
          />
        </Field>
      </div>

      <Field
        label={t('Git 저장소')}
        htmlFor="app-git"
        hint={t(
          '공개 저장소는 https 주소 그대로 됩니다. 비공개 저장소는 SSH 주소(git@github.com:계정/저장소.git)를 넣고, 만든 뒤 Git · 웹훅 탭의 Deploy key 를 저장소에 등록하세요.',
        )}
      >
        <input id="app-git" className="input font-mono" required value={v.git_url} onChange={set('git_url')} placeholder="git@github.com:me/app.git" />
        {sshUrl(v.git_url) && (
          <p className="mt-1.5 flex flex-wrap items-center gap-2 text-xs text-warn">
            {t('비공개 저장소라면 SSH 주소를 써야 합니다.')}
            <button type="button" className="btn-ghost px-2 py-0.5 text-xs" onClick={() => setV((s) => ({ ...s, git_url: sshUrl(s.git_url) }))}>
              {t('{url} 로 바꾸기', { url: sshUrl(v.git_url) })}
            </button>
          </p>
        )}
      </Field>

      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t('브랜치')} htmlFor="app-branch">
          <input id="app-branch" className="input font-mono" value={v.git_branch} onChange={set('git_branch')} />
        </Field>
        <Field label={t('루트 디렉토리')} htmlFor="app-root" hint={t('모노레포면 하위 폴더 (예: frontend)')}>
          <input id="app-root" className="input font-mono" value={v.root_dir ?? ''} onChange={set('root_dir')} placeholder={t('(저장소 루트)')} />
        </Field>
      </div>

      <Field label={t('빌드 명령')} htmlFor="app-build">
        <input
          id="app-build"
          className="input font-mono"
          value={v.build_cmd ?? ''}
          onChange={set('build_cmd')}
          placeholder={BUILD_PLACEHOLDER[v.kind] && t(BUILD_PLACEHOLDER[v.kind])}
        />
      </Field>

      {v.kind === 'rust' && (
        <Field label={t('바이너리 이름')} htmlFor="app-bin" hint={t('target/release/<이름> — Cargo.toml의 package name (비우면 slug)')}>
          <input id="app-bin" className="input font-mono" value={v.binary_name ?? ''} onChange={set('binary_name')} placeholder={v.slug || 'hello-api'} />
        </Field>
      )}
      {v.kind === 'java' && (
        <Field label={t('jar 파일')} htmlFor="app-jar" hint={t('앱 폴더 기준 경로 — 비우면 target/ · build/libs/ 에서 실행용 jar 를 자동으로 찾습니다')}>
          <input id="app-jar" className="input font-mono" value={v.binary_name ?? ''} onChange={set('binary_name')} placeholder="target/app.jar" />
        </Field>
      )}
      {START_HINT[v.kind] && (
        <Field label={t('시작 명령')} htmlFor="app-start" hint={t(START_HINT[v.kind])}>
          <input id="app-start" className="input font-mono" value={v.start_cmd ?? ''} onChange={set('start_cmd')} placeholder={START_PLACEHOLDER[v.kind]} />
        </Field>
      )}
      {isServiceKind(v.kind) && (
        <div className="rounded-lg border border-ink-700 p-4">
          <p className="text-sm font-medium">{t('프런트엔드 (npm 빌드, 선택)')}</p>
          <p className="mt-1 text-xs text-ink-400">
            {t(
              '같은 저장소에 React·Vue 같은 화면이 있으면 폴더를 넣으세요. 배포할 때 그 폴더에서 npm ci → npm run build 를 먼저 하고, 결과물을 프로그램 옆 같은 경로로 복사합니다.',
            )}
          </p>
          <div className="mt-3 grid gap-4 sm:grid-cols-2">
            <Field label={t('프런트엔드 폴더')} htmlFor="app-front" hint={t('앱 폴더 기준 (예: frontend). 비우면 npm 빌드를 하지 않습니다')}>
              <input id="app-front" className="input font-mono" value={v.frontend_dir ?? ''} onChange={set('frontend_dir')} placeholder="frontend" />
            </Field>
            {v.frontend_dir?.trim() && (
              <Field
                label={t('빌드 결과 폴더')}
                htmlFor="app-front-out"
                hint={t('프로그램에서 {path} 경로로 읽으면 됩니다', { path: `${v.frontend_dir.trim().replace(/^\/+|\/+$/g, '')}/${v.output_dir || 'dist'}` })}
              >
                <input id="app-front-out" className="input font-mono" value={v.output_dir} onChange={set('output_dir')} placeholder="dist" />
              </Field>
            )}
          </div>
        </div>
      )}
      {v.kind === 'static' && (
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('출력 디렉토리')} htmlFor="app-out">
            <input id="app-out" className="input font-mono" value={v.output_dir} onChange={set('output_dir')} />
          </Field>
          {backends.length > 0 ? (
            <Field label={t('API 연결 (백엔드)')} htmlFor="app-api" hint={t('같은 도메인의 /api/ 요청을 이 백엔드로 보냅니다')}>
              <select id="app-api" className="input" value={v.api_proxy_port ?? ''} onChange={set('api_proxy_port')}>
                <option value="">{t('연결 안 함')}</option>
                {backends.map((b) => (
                  <option key={b.id} value={b.port ?? ''} disabled={!b.port}>
                    {b.name} (:{b.port})
                  </option>
                ))}
              </select>
            </Field>
          ) : (
            <Field label={t('/api/ 프록시 포트')} htmlFor="app-api" hint={t('같은 도메인의 /api/ 를 이 로컬 포트로 (선택)')}>
              <input
                id="app-api"
                type="number"
                min="1"
                max="65535"
                className="input font-mono"
                value={v.api_proxy_port ?? ''}
                onChange={set('api_proxy_port')}
                placeholder="8100"
              />
            </Field>
          )}
        </div>
      )}

      <div className={`grid gap-4 ${isService ? 'sm:grid-cols-[1fr_180px]' : ''}`}>
        <Field label={t('도메인')} htmlFor="app-domains" hint={t('공백/쉼표로 구분 — 네이키드와 www 둘 다 넣으세요. DNS 가 이 서버(또는 앞단 프록시)를 가리켜야 합니다')}>
          <input
            id="app-domains"
            className="input font-mono"
            value={v.domains}
            onChange={set('domains')}
            placeholder={
              v.kind === 'static' && v.app_role === 'admin'
                ? 'admin.beripick.com'
                : role === 'back'
                  ? t('(비워도 됩니다 — 프런트의 /api/ 로 연결)')
                  : 'beripick.com www.beripick.com'
            }
          />
        </Field>
        {isService && (
          <Field label={t('헬스체크 경로')} htmlFor="app-health">
            <input id="app-health" className="input font-mono" value={v.health_path} onChange={set('health_path')} />
          </Field>
        )}
      </div>

      <div className="flex justify-end pt-2">
        <button className="btn-primary" disabled={submitting}>
          {submitting ? t('저장 중…') : mode === 'create' ? t('앱 만들기') : t('저장')}
        </button>
      </div>
    </form>
  )
}

function normalize(app) {
  if (!app) return {}
  const out = {}
  for (const [k, val] of Object.entries(app)) out[k] = val ?? ''
  return out
}
