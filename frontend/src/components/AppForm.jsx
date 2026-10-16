import { useState } from 'react'
import { Field } from './ui'
import { t } from '../i18n'

export const KINDS = [
  { id: 'rust', label: 'Rust', desc: 'cargo build → systemd 서비스 (Axum 등)' },
  { id: 'python', label: 'Python', desc: 'venv + requirements.txt → systemd 서비스' },
  { id: 'static', label: 'Static', desc: 'npm run build → Nginx 정적 (React·Vite)' },
]

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
    domains: '',
    api_proxy_port: '',
    health_path: '/',
  }
}

/** 서버로 보낼 payload (빈 값 정리) */
export function toPayload(v) {
  const out = { ...v }
  out.api_proxy_port = v.api_proxy_port === '' || v.api_proxy_port == null ? null : Number(v.api_proxy_port)
  for (const k of ['slug', 'root_dir', 'build_cmd', 'start_cmd', 'binary_name']) {
    if (typeof out[k] === 'string' && !out[k].trim()) out[k] = null
  }
  return out
}

const BUILD_PLACEHOLDER = {
  rust: 'cargo build --release',
  python: '(선택) python manage.py collectstatic --noinput',
  static: 'npm run build',
}

export default function AppForm({ initial, mode = 'create', onSubmit, submitting }) {
  const [v, setV] = useState(() => ({ ...emptyApp(), ...normalize(initial) }))
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
        <div>
          <span className="label">{t('종류')}</span>
          <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-label={t('앱 종류')}>
            {KINDS.map((k) => (
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

      <Field label={t('Git 저장소')} htmlFor="app-git" hint={t('private 저장소는 ssh://git@<호스트>:<포트>/그룹/저장소.git 형식 + 생성 후 Deploy key 등록')}>
        <input id="app-git" className="input font-mono" required value={v.git_url} onChange={set('git_url')} placeholder="https://github.com/me/app.git" />
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
      {v.kind === 'python' && (
        <Field label={t('시작 명령')} htmlFor="app-start" hint={t('가상환경(.venv)이 켜진 상태로 실행 · PORT 환경변수의 포트로 127.0.0.1 에서 열어야 합니다')}>
          <input id="app-start" className="input font-mono" value={v.start_cmd ?? ''} onChange={set('start_cmd')} placeholder="gunicorn -b 127.0.0.1:$PORT app:app" />
        </Field>
      )}
      {v.kind === 'static' && (
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('출력 디렉토리')} htmlFor="app-out">
            <input id="app-out" className="input font-mono" value={v.output_dir} onChange={set('output_dir')} />
          </Field>
          <Field label={t('/api/ 프록시 포트')} htmlFor="app-api" hint={t('같은 도메인의 /api/ 를 이 로컬 포트로 (선택)')}>
            <input id="app-api" type="number" min="1" max="65535" className="input font-mono" value={v.api_proxy_port ?? ''} onChange={set('api_proxy_port')} placeholder="8100" />
          </Field>
        </div>
      )}

      <div className={`grid gap-4 ${isService ? 'sm:grid-cols-[1fr_180px]' : ''}`}>
        <Field label={t('도메인')} htmlFor="app-domains" hint={t('공백/쉼표로 구분 — 네이키드와 www 둘 다 넣으세요. DNS 가 이 서버(또는 앞단 프록시)를 가리켜야 합니다')}>
          <input id="app-domains" className="input font-mono" value={v.domains} onChange={set('domains')} placeholder="beripick.com www.beripick.com" />
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
