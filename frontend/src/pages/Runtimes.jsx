import { useState } from 'react'
import { Link } from 'react-router'
import { ErrorBox, Modal, PageHeader, Section, Spinner } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { t } from '../i18n'
import { del, get, post, put } from '../lib/api'
import { useFetch, useInterval } from '../lib/useFetch'

// label·desc 는 한국어 원문 (화면에는 t() 로 번역)
export const LANGS = [
  { id: 'rust', label: 'Rust', desc: 'Git 자동 배포로 빌드하는 Rust 앱에 쓰입니다 (rustup 툴체인). 직접 빌드해서 올리는 Rust 사이트와는 상관없습니다.' },
  { id: 'python', label: 'Python', desc: 'Python 사이트와 Python 앱의 가상환경(.venv)을 이 버전으로 만듭니다.' },
  { id: 'node', label: 'Node.js', desc: 'React·Vue 같은 화면을 Git 에서 빌드(npm run build)할 때 쓰입니다.' },
]

/** 화면에 보일 버전 이름 */
export function versionLabel(lang, v) {
  if (v === 'system') return t('서버 기본')
  if (lang === 'rust' && ['stable', 'beta', 'nightly'].includes(v)) return t(v === 'stable' ? '최신 안정판 (stable)' : v === 'beta' ? '베타 (beta)' : '나이틀리 (nightly)')
  return v
}

const STATUS = {
  ready: { label: '사용 가능', cls: 'text-signal' },
  installing: { label: '설치 중…', cls: 'text-warn' },
  failed: { label: '설치 실패', cls: 'text-red-300' },
}

function LogModal({ row, onClose }) {
  if (!row) return null
  return (
    <Modal open wide onClose={onClose} title={t('{lang} {v} 설치 기록', { lang: row.lang, v: row.version })}>
      <pre className="max-h-[420px] overflow-auto rounded-lg bg-ink-950 p-3 font-mono text-xs whitespace-pre-wrap text-ink-300">{row.log || t('기록이 없습니다')}</pre>
      <div className="mt-4 flex justify-end">
        <button className="btn-primary" onClick={onClose}>
          {t('닫기')}
        </button>
      </div>
    </Modal>
  )
}

function LangSection({ lang, rows, candidates, usage, onChanged }) {
  const toast = useToast()
  const def = rows.find((r) => r.is_default)
  const [pick, setPick] = useState('')
  const [custom, setCustom] = useState('')
  const [busy, setBusy] = useState(false)
  const [logRow, setLogRow] = useState(null)

  const installed = new Set(rows.map((r) => r.version))
  const options = [...new Set([...rows.map((r) => r.version), ...candidates])]
  const target = pick === '__custom' ? custom.trim() : pick || def?.version || ''
  const targetRow = rows.find((r) => r.version === target)

  const run = async (fn, msg) => {
    setBusy(true)
    try {
      await fn()
      if (msg) toast.push(msg, 'success')
      onChanged()
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setBusy(false)
    }
  }

  const apply = (makeDefault) =>
    run(
      () => post('/runtimes', { lang: lang.id, version: target, make_default: makeDefault }),
      targetRow?.status === 'ready'
        ? t('기본 버전을 {v} (으)로 바꿨습니다', { v: versionLabel(lang.id, target) })
        : makeDefault
          ? t('{v} 설치를 시작했습니다. 끝나면 기본 버전이 됩니다', { v: versionLabel(lang.id, target) })
          : t('{v} 설치를 시작했습니다', { v: versionLabel(lang.id, target) }),
    )

  return (
    <Section title={lang.label} desc={t(lang.desc)}>
      <div className="flex flex-wrap items-end gap-2" role="group" aria-label={t('{lang} 버전 고르기', { lang: lang.label })}>
        <label className="min-w-[180px]">
          <span className="label">{t('기본 버전')}</span>
          <select className="input" aria-label={t('{lang} 버전', { lang: lang.label })} value={pick || def?.version || ''} onChange={(e) => setPick(e.target.value)}>
            {options.map((v) => (
              <option key={v} value={v}>
                {versionLabel(lang.id, v)}
                {installed.has(v) ? '' : ` — ${t('설치 필요')}`}
              </option>
            ))}
            <option value="__custom">{t('직접 입력…')}</option>
          </select>
        </label>
        {pick === '__custom' && (
          <label>
            <span className="label">{t('버전')}</span>
            <input
              className="input w-32 font-mono"
              aria-label={t('{lang} 버전 직접 입력', { lang: lang.label })}
              placeholder={lang.id === 'rust' ? '1.85' : lang.id === 'python' ? '3.12' : '22'}
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
            />
          </label>
        )}
        <button
          className="btn-primary"
          disabled={busy || !target || (targetRow?.is_default && targetRow.status === 'ready') || targetRow?.status === 'installing'}
          onClick={() => apply(true)}
        >
          {targetRow?.status === 'ready' || !target ? t('기본으로 변경') : t('설치하고 기본으로 변경')}
        </button>
        {target && targetRow?.status !== 'ready' && targetRow?.status !== 'installing' && (
          <button className="btn-ghost" disabled={busy} onClick={() => apply(false)}>
            {t('설치만')}
          </button>
        )}
      </div>

      <table className="mt-5 w-full table-fixed text-sm" aria-label={t('{lang} 설치된 버전', { lang: lang.label })}>
        <thead className="text-left text-xs text-ink-400">
          <tr>
            <th className="w-[30%] py-2 pr-3 font-medium">{t('버전')}</th>
            <th className="w-[18%] px-3 py-2 font-medium">{t('실제 버전')}</th>
            <th className="w-[16%] px-3 py-2 font-medium">{t('상태')}</th>
            <th className="w-[10%] px-3 py-2 font-medium">{t('쓰는 앱')}</th>
            <th className="py-2 pl-3" />
          </tr>
        </thead>
        <tbody className="divide-y divide-ink-800">
          {rows.map((r) => {
            const st = STATUS[r.status] ?? { label: r.status, cls: '' }
            return (
              <tr key={r.id}>
                <td className="py-2.5 pr-3">
                  <span className="font-mono text-xs">{versionLabel(lang.id, r.version)}</span>
                  {r.is_default && <span className="ml-2 rounded border border-signal/40 px-1.5 text-[11px] text-signal">{t('기본')}</span>}
                </td>
                <td className="px-3 py-2.5 font-mono text-xs text-ink-300">{r.detail || '–'}</td>
                <td className={`px-3 py-2.5 text-xs ${st.cls}`}>
                  {r.status === 'installing' && <Spinner className="mr-1.5 inline-block h-3 w-3" />}
                  {t(st.label)}
                </td>
                <td className="px-3 py-2.5 text-xs text-ink-400">{usage[`${lang.id}:${r.version}`] ?? 0}</td>
                <td className="py-2.5 pl-3 text-right whitespace-nowrap">
                  {r.status === 'ready' && !r.is_default && (
                    <button
                      className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                      disabled={busy}
                      onClick={() => run(() => put(`/runtimes/${r.id}/default`), t('기본 버전을 {v} (으)로 바꿨습니다', { v: versionLabel(lang.id, r.version) }))}
                    >
                      {t('기본으로')}
                    </button>
                  )}
                  {r.status === 'failed' && !r.is_system && (
                    <button
                      className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                      disabled={busy}
                      onClick={() => run(() => post('/runtimes', { lang: lang.id, version: r.version }), t('다시 설치합니다'))}
                    >
                      {t('다시 설치')}
                    </button>
                  )}
                  {r.log && (
                    <button className="btn-ghost mr-1.5 px-2.5 py-1 text-xs" onClick={() => setLogRow(r)}>
                      {t('기록')}
                    </button>
                  )}
                  {!r.is_system && !r.is_default && r.status !== 'installing' && (
                    <button
                      className="btn-ghost px-2.5 py-1 text-xs text-red-300"
                      disabled={busy}
                      aria-label={t('{lang} {v} 삭제', { lang: lang.label, v: r.version })}
                      onClick={() =>
                        window.confirm(t('{lang} {v} 을(를) 서버에서 지울까요?', { lang: lang.label, v: r.version })) && run(() => del(`/runtimes/${r.id}`), t('삭제했습니다'))
                      }
                    >
                      {t('삭제')}
                    </button>
                  )}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
      <LogModal row={logRow} onClose={() => setLogRow(null)} />
    </Section>
  )
}

export default function RuntimesPage() {
  const data = useFetch(() => get('/runtimes'), [])
  const installing = data.data?.runtimes.some((r) => r.status === 'installing')
  useInterval(() => data.reload(true), 2000, !!installing)

  return (
    <>
      <PageHeader title={t('런타임')} subtitle={t('언어별로 여러 버전을 설치해 두고 기본 버전을 고릅니다. 앱마다 다른 버전을 쓰려면 앱의 설정 탭에서 고르세요.')} />
      {data.loading && <Spinner className="text-ink-400" />}
      <ErrorBox error={data.error} />
      {data.data && (
        <div className="space-y-4">
          {LANGS.map((lang) => (
            <LangSection
              key={lang.id}
              lang={lang}
              rows={data.data.runtimes.filter((r) => r.lang === lang.id)}
              candidates={data.data.candidates[lang.id] ?? []}
              usage={data.data.usage}
              onChanged={() => data.reload(true)}
            />
          ))}
          <p className="text-xs text-ink-400">{t('바꾼 버전은 다음 배포·적용부터 쓰입니다. Python 사이트는 버전이 바뀌면 가상환경을 새로 만들고 패키지를 다시 설치합니다.')}</p>
        </div>
      )}
    </>
  )
}

/** 앱 설정 탭: 이 앱이 쓸 버전 (기본 = 런타임 메뉴의 기본 버전) */
export function AppRuntimePicker({ app, onSaved }) {
  const toast = useToast()
  const lang = runtimeLang(app)
  const data = useFetch(() => (lang ? get('/runtimes') : Promise.resolve(null)), [lang])
  const [value, setValue] = useState(app.runtime_version ?? '')
  const [busy, setBusy] = useState(false)
  if (!lang) return null
  const meta = LANGS.find((l) => l.id === lang)
  const rows = data.data?.runtimes.filter((r) => r.lang === lang) ?? []
  const def = rows.find((r) => r.is_default)
  const ready = rows.filter((r) => r.status === 'ready')
  return (
    <Section title={t('{lang} 버전', { lang: meta.label })} desc={t('바꾸면 다음 배포·적용부터 이 버전으로 빌드·실행합니다.')}>
      <div className="flex flex-wrap items-end gap-2">
        <label className="min-w-[220px]">
          <span className="label">{t('버전')}</span>
          <select className="input" aria-label={t('앱 런타임 버전')} value={value} onChange={(e) => setValue(e.target.value)}>
            <option value="">{t('기본 버전 따르기 ({v})', { v: def ? versionLabel(lang, def.version) : '…' })}</option>
            {ready.map((r) => (
              <option key={r.id} value={r.version}>
                {versionLabel(lang, r.version)}
                {r.detail ? ` · ${r.detail}` : ''}
              </option>
            ))}
          </select>
        </label>
        <button
          className="btn-primary"
          disabled={busy || value === (app.runtime_version ?? '')}
          onClick={async () => {
            setBusy(true)
            try {
              const n = await put(`/apps/${app.id}/runtime`, { version: value || null })
              toast.push(t('버전을 저장했습니다. 다음 배포·적용부터 쓰입니다'), 'success')
              onSaved?.(n)
            } catch (e) {
              toast.push(e.message, 'error')
            } finally {
              setBusy(false)
            }
          }}
        >
          {t('저장')}
        </button>
        <Link to="/runtimes" className="text-xs text-ink-400 hover:text-ink-100">
          {t('다른 버전 설치 →')}
        </Link>
      </div>
    </Section>
  )
}

/** 앱이 쓰는 언어 (업로드형 Rust·HTML·프록시는 없음) */
export function runtimeLang(app) {
  if (app.kind === 'python') return 'python'
  if (app.source === 'git' && app.kind === 'rust') return 'rust'
  if (app.source === 'git' && app.kind === 'static') return 'node'
  return null
}
