import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import LogViewer from '../components/LogViewer'
import { CardHeader, CopyButton, ErrorBox, Field, Modal, Spinner } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { del, download, get, post, put, uploadFile } from '../lib/api'
import { bytes, dateTime } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { t, tx } from '../i18n'
import { confirmDialog, promptDialog } from '../components/Dialog'

/** /home/<user>/htdocs/<domain> → htdocs/<domain> */
export function relRoot(app) {
  if (!app.site_root || !app.site_user) return ''
  return app.site_root.replace(`/home/${app.site_user}/`, '')
}

export function joinPath(dir, name) {
  return dir ? `${dir.replace(/\/+$/, '')}/${name}` : name
}

export function parentPath(p) {
  const i = p.lastIndexOf('/')
  return i < 0 ? '' : p.slice(0, i)
}

const TEXT_EXT = /\.(rs|toml|html?|css|js|mjs|cjs|jsx|ts|tsx|json|md|txt|xml|svg|ya?ml|ini|conf|env|htaccess|sql|sh|py|rb|vue|lock|log|csv|map)$|^\.[a-z]+$/i
const ARCHIVE = /\.(zip|tar\.gz|tgz|tar)$/i

// ───────────────────────── 파일 관리자 ─────────────────────────

export function FileManager({ app }) {
  const toast = useToast()
  const [cwd, setCwd] = useState(relRoot(app))
  const [data, setData] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(true)
  const [editing, setEditing] = useState(null) // {path, content, saving}
  const [uploads, setUploads] = useState([]) // {name, progress, error}
  const [drag, setDrag] = useState(false)
  const fileInput = useRef(null)

  const load = async (path = cwd) => {
    setLoading(true)
    try {
      const r = await get(`/apps/${app.id}/files?path=${encodeURIComponent(path)}`)
      setData(r)
      setCwd(r.path)
      setError(null)
    } catch (e) {
      setError(e)
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    load(relRoot(app))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [app.id])

  const op = async (body, ok) => {
    try {
      await post(`/apps/${app.id}/files/op`, body)
      if (ok) toast.push(ok, 'success')
      load()
    } catch (e) {
      toast.push(e.message, 'error')
    }
  }

  const openEditor = async (path) => {
    try {
      const r = await get(`/apps/${app.id}/files/content?path=${encodeURIComponent(path)}`)
      setEditing({ path, content: r.content, original: r.content })
    } catch (e) {
      toast.push(e.message, 'error')
    }
  }

  const save = async () => {
    setEditing((s) => ({ ...s, saving: true }))
    try {
      await put(`/apps/${app.id}/files/content`, { path: editing.path, content: editing.content })
      toast.push(t('저장했습니다'), 'success')
      setEditing((s) => ({ ...s, saving: false, original: s.content }))
      load()
    } catch (e) {
      toast.push(e.message, 'error')
      setEditing((s) => ({ ...s, saving: false }))
    }
  }

  const doUpload = async (files) => {
    const list = Array.from(files)
    if (!list.length) return
    setUploads(list.map((f) => ({ name: f.name, progress: 0 })))
    for (let i = 0; i < list.length; i++) {
      const f = list[i]
      try {
        await uploadFile(app.id, joinPath(cwd, f.name), f, (p) => setUploads((u) => u.map((x, j) => (j === i ? { ...x, progress: p } : x))))
        setUploads((u) => u.map((x, j) => (j === i ? { ...x, progress: 1, done: true } : x)))
      } catch (e) {
        setUploads((u) => u.map((x, j) => (j === i ? { ...x, error: e.message } : x)))
      }
    }
    load()
    setTimeout(() => setUploads((u) => u.filter((x) => x.error)), 2500)
  }

  const crumbs = cwd ? cwd.split('/') : []

  return (
    <div
      className={`card relative ${drag ? 'border-signal/70' : ''}`}
      onDragOver={(e) => {
        e.preventDefault()
        setDrag(true)
      }}
      onDragLeave={() => setDrag(false)}
      onDrop={(e) => {
        e.preventDefault()
        setDrag(false)
        doUpload(e.dataTransfer.files)
      }}
    >
      <div className="flex flex-wrap items-center gap-2 border-b border-ink-700/70 px-4 py-3">
        <nav className="flex min-w-0 flex-1 flex-wrap items-center gap-1 font-mono text-xs" aria-label={t('경로')}>
          <button className="text-ink-400 hover:text-ink-100" onClick={() => load('')}>
            ~{app.site_user}
          </button>
          {crumbs.map((c, i) => (
            <span key={i} className="flex items-center gap-1">
              <span className="text-ink-600">/</span>
              <button className="hover:text-signal" onClick={() => load(crumbs.slice(0, i + 1).join('/'))}>
                {c}
              </button>
            </span>
          ))}
        </nav>
        <button
          className="btn-ghost px-2.5 py-1 text-xs"
          onClick={async () => {
            const n = await promptDialog(t('새 폴더 이름'))
            if (n) op({ op: 'mkdir', path: joinPath(cwd, n) })
          }}
        >
          {t('새 폴더')}
        </button>
        <button
          className="btn-ghost px-2.5 py-1 text-xs"
          onClick={async () => {
            const n = await promptDialog(t('새 파일 이름 (예: index.html, config.toml)'))
            if (n) setEditing({ path: joinPath(cwd, n), content: '', original: null })
          }}
        >
          {t('새 파일')}
        </button>
        <button className="btn-primary px-2.5 py-1 text-xs" onClick={() => fileInput.current?.click()}>
          {t('업로드')}
        </button>
        <input ref={fileInput} type="file" multiple className="hidden" aria-label={t('파일 업로드')} onChange={(e) => doUpload(e.target.files)} />
        <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => load()} aria-label={t('새로고침')}>
          ↻
        </button>
      </div>

      {uploads.length > 0 && (
        <ul className="space-y-1 border-b border-ink-700/70 px-4 py-2 text-xs">
          {uploads.map((u, i) => (
            <li key={i} className="flex items-center gap-3">
              <span className="w-48 truncate font-mono">{u.name}</span>
              <div className="h-1.5 flex-1 overflow-hidden rounded bg-ink-800">
                <div className={`h-full ${u.error ? 'bg-fail' : 'bg-signal'}`} style={{ width: `${Math.round((u.progress || 0) * 100)}%` }} />
              </div>
              <span className={u.error ? 'text-fail' : 'text-ink-400'}>{u.error || (u.done ? t('완료') : `${Math.round((u.progress || 0) * 100)}%`)}</span>
            </li>
          ))}
        </ul>
      )}

      {error && (
        <div className="p-4">
          <ErrorBox error={error} />
        </div>
      )}
      {loading && !data ? (
        <div className="p-6">
          <Spinner className="text-ink-400" />
        </div>
      ) : (
        data && (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="text-left text-xs text-ink-400">
                <tr>
                  <th className="px-4 py-2 font-medium">{t('이름')}</th>
                  <th className="px-3 py-2 font-medium">{t('크기')}</th>
                  <th className="px-3 py-2 font-medium">{t('권한')}</th>
                  <th className="px-3 py-2 font-medium">{t('수정')}</th>
                  <th className="px-4 py-2" />
                </tr>
              </thead>
              <tbody className="divide-y divide-ink-800">
                {cwd && (
                  <tr className="hover:bg-ink-850">
                    <td className="px-4 py-2" colSpan={5}>
                      <button className="font-mono text-ink-400 hover:text-ink-100" onClick={() => load(parentPath(cwd))}>
                        ..
                      </button>
                    </td>
                  </tr>
                )}
                {data.entries.map((e) => {
                  const path = joinPath(cwd, e.name)
                  return (
                    <tr key={e.name} className="group hover:bg-ink-850" data-testid="file-row">
                      <td className="max-w-80 px-4 py-2">
                        {e.kind === 'dir' ? (
                          <button className="flex items-center gap-2 font-mono hover:text-signal" onClick={() => load(path)}>
                            <span className="text-warn">▸</span> {e.name}/
                          </button>
                        ) : (
                          <button
                            className="flex max-w-full items-center gap-2 truncate font-mono hover:text-signal"
                            title={e.target ? `→ ${e.target}` : ''}
                            onClick={() =>
                              TEXT_EXT.test(e.name) || e.size < 64 * 1024 ? openEditor(path) : download(`/apps/${app.id}/files/download?path=${encodeURIComponent(path)}`, e.name)
                            }
                          >
                            <span className="text-ink-600">{e.kind === 'link' ? '↪' : '·'}</span> {e.name}
                          </button>
                        )}
                      </td>
                      <td className="px-3 py-2 font-mono text-xs text-ink-400">{e.kind === 'dir' ? '' : bytes(e.size)}</td>
                      <td className="px-3 py-2">
                        <button
                          className="font-mono text-xs text-ink-400 hover:text-ink-100"
                          title={t('권한 변경')}
                          onClick={async () => {
                            const m = await promptDialog(t('{name} 권한 (예: 644, 755)', { name: e.name }), e.mode.slice(1))
                            if (m) op({ op: 'chmod', path, mode: m })
                          }}
                        >
                          {e.mode.slice(1)}
                        </button>
                      </td>
                      <td className="px-3 py-2 text-xs whitespace-nowrap text-ink-400">{dateTime(e.mtime * 1000)}</td>
                      <td className="px-4 py-2 text-right whitespace-nowrap">
                        <span className="inline-flex gap-1 opacity-70 group-hover:opacity-100">
                          {e.kind === 'file' && ARCHIVE.test(e.name) && (
                            <button
                              className="btn-ghost px-2 py-0.5 text-xs"
                              onClick={async () =>
                                (await confirmDialog(t('{name} 를 이 폴더에 풀까요? (같은 이름은 덮어씁니다)', { name: e.name }))) &&
                                op({ op: 'extract', path, to: cwd }, t('압축을 풀었습니다'))
                              }
                            >
                              {t('풀기')}
                            </button>
                          )}
                          {e.kind === 'file' && (
                            <button
                              className="btn-ghost px-2 py-0.5 text-xs"
                              onClick={() => download(`/apps/${app.id}/files/download?path=${encodeURIComponent(path)}`, e.name).catch((er) => toast.push(er.message, 'error'))}
                            >
                              {t('받기')}
                            </button>
                          )}
                          <button
                            className="btn-ghost px-2 py-0.5 text-xs"
                            onClick={async () => {
                              const n = await promptDialog(t('새 이름'), e.name)
                              if (n && n !== e.name) op({ op: 'mv', path, to: joinPath(cwd, n) })
                            }}
                          >
                            {t('이름')}
                          </button>
                          <button
                            className="btn-ghost px-2 py-0.5 text-xs text-fail"
                            aria-label={t('{name} 삭제', { name: e.name })}
                            onClick={async () =>
                              (await confirmDialog(t('{name}{v}을(를) 삭제할까요?', { name: e.name, v: e.kind === 'dir' ? t(' 폴더와 안의 모든 파일') : '' }))) &&
                              op({ op: 'rm', path }, t('삭제했습니다'))
                            }
                          >
                            {t('삭제')}
                          </button>
                        </span>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
            {data.entries.length === 0 && <p className="px-4 py-8 text-center text-sm text-ink-400">{t('빈 폴더입니다 — 파일을 끌어다 놓거나 업로드하세요')}</p>}
          </div>
        )
      )}
      <p className="border-t border-ink-700/70 px-4 py-2 text-xs text-ink-400">
        {tx('파일은 사이트 유저 {user} 권한으로 다뤄집니다. 큰 사이트는 zip 으로 올린 뒤 “풀기”를 쓰거나 SFTP 를 쓰세요.', {
          user: <span className="font-mono">{app.site_user}</span>,
        })}
      </p>

      <Modal
        open={!!editing}
        onClose={async () => (editing?.content === editing?.original || (await confirmDialog(t('저장하지 않은 변경이 있습니다. 닫을까요?')))) && setEditing(null)}
        title={editing?.path || ''}
        wide
      >
        {editing && (
          <div className="space-y-3">
            <textarea
              aria-label={t('파일 내용')}
              spellCheck={false}
              className="input h-[60vh] font-mono text-xs leading-relaxed"
              value={editing.content}
              onChange={(e) => setEditing({ ...editing, content: e.target.value })}
              onKeyDown={(e) => {
                if ((e.ctrlKey || e.metaKey) && e.key === 's') {
                  e.preventDefault()
                  save()
                }
                if (e.key === 'Tab') {
                  e.preventDefault()
                  const t = e.currentTarget
                  const { selectionStart: a, selectionEnd: b } = t
                  const next = editing.content.slice(0, a) + '    ' + editing.content.slice(b)
                  setEditing({ ...editing, content: next })
                  requestAnimationFrame(() => t.setSelectionRange(a + 4, a + 4))
                }
              }}
            />
            <div className="flex items-center justify-between">
              <span className="text-xs text-ink-400">{t('Ctrl+S 로 저장')}</span>
              <button className="btn-primary" disabled={editing.saving} onClick={save}>
                {editing.saving ? t('저장 중…') : t('저장')}
              </button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  )
}

// ───────────────────────── SSL ─────────────────────────

export function SslTab({ app, onChanged }) {
  const toast = useToast()
  const info = useFetch(() => get(`/apps/${app.id}/ssl`), [app.id])
  const [email, setEmail] = useState('')
  const [force, setForce] = useState(true)
  const [custom, setCustom] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  if (info.loading) return <Spinner className="text-ink-400" />
  const d = info.data
  const proxyMode = d?.edge_mode === 'proxy'

  const run = async (fn, msg) => {
    setBusy(true)
    setError(null)
    try {
      await fn()
      toast.push(msg, 'success')
      info.reload(true)
      onChanged?.()
      return true
    } catch (e) {
      setError(e)
      return false
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-4">
      {proxyMode && (
        <div className="rounded-lg border border-sky-400/30 bg-sky-400/10 px-4 py-3 text-sm text-sky-200">
          {tx(
            "이 서버는 {mode}로 설치되었습니다. HTTPS 인증서는 앞단 프록시(예: Nginx Proxy Manager)에서 발급·관리하세요. Let's Encrypt 확인 요청이 이 서버까지 오지 않기 때문입니다.",
            { mode: <b>{t('앞단 프록시 모드')}</b> },
          )}
        </div>
      )}
      <section className="card p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h3 className="font-medium">{t('현재 상태')}</h3>
            <p className="mt-1 text-sm text-ink-400">
              {d.mode === 'none' ? t('HTTPS 없음 (HTTP 로만 서비스)') : d.mode === 'letsencrypt' ? t("Let's Encrypt 인증서 (자동 갱신)") : t('직접 올린 인증서')}
            </p>
          </div>
          {d.mode !== 'none' && (
            <div className="flex items-center gap-3">
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={d.force_https}
                  disabled={busy}
                  onChange={(e) =>
                    run(
                      () => put(`/apps/${app.id}/ssl/force`, { force_https: e.target.checked }),
                      e.target.checked ? t('HTTP → HTTPS 리다이렉트를 켰습니다') : t('리다이렉트를 껐습니다'),
                    )
                  }
                />
                {t('HTTP → HTTPS 강제')}
              </label>
              <button
                className="btn-danger px-2.5 py-1 text-xs"
                disabled={busy}
                onClick={async () => (await confirmDialog(t('HTTPS 를 끄고 인증서를 삭제할까요?'))) && run(() => del(`/apps/${app.id}/ssl`), t('HTTPS 를 껐습니다'))}
              >
                {t('인증서 삭제')}
              </button>
            </div>
          )}
        </div>
        {d.certificates.length > 0 && (
          <ul className="mt-4 space-y-2">
            {d.certificates.map((c) => (
              <li key={c.path} className="rounded-lg border border-ink-700 bg-ink-950 px-4 py-3 text-xs">
                <p>
                  {t('만료')} <b className="font-mono">{c.not_after}</b>
                </p>
                <p className="mt-1 text-ink-400">{c.issuer}</p>
                {c.domains && <p className="mt-1 font-mono text-ink-300">{c.domains.join(', ')}</p>}
              </li>
            ))}
          </ul>
        )}
      </section>

      {!proxyMode && (
        <section className="card p-5">
          <CardHeader
            title={t("Let's Encrypt 무료 인증서")}
            help={t('도메인({domains})의 DNS 가 이 서버를 가리키고 80 포트가 열려 있어야 합니다. 발급 후 자동 갱신됩니다.', { domains: d.domains.join(', ') })}
          />
          <form
            className="mt-4 flex flex-wrap items-end gap-3"
            onSubmit={(e) => {
              e.preventDefault()
              run(() => post(`/apps/${app.id}/ssl/letsencrypt`, { email, force_https: force }), t('인증서를 발급했습니다'))
            }}
          >
            <Field label={t('이메일 (만료 알림용)')} htmlFor="le-email">
              <input id="le-email" type="email" required className="input w-72" value={email} onChange={(e) => setEmail(e.target.value)} />
            </Field>
            <label className="mb-2 flex items-center gap-2 text-sm">
              <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} /> {t('HTTP → HTTPS 강제')}
            </label>
            <button className="btn-primary" disabled={busy}>
              {busy ? t('발급 중…') : d.mode === 'letsencrypt' ? t('다시 발급') : t('발급')}
            </button>
          </form>
        </section>
      )}

      <section className="card p-5">
        <CardHeader title={t('가지고 있는 인증서 올리기')} help={t('유료 인증서·와일드카드 인증서 (PEM). 개인키는 암호가 없어야 합니다.')}>
          {!custom && (
            <button className="btn-ghost text-xs" onClick={() => setCustom({ certificate: '', private_key: '' })}>
              {t('인증서 붙여넣기')}
            </button>
          )}
        </CardHeader>
        {custom && (
          <form
            className="mt-4 space-y-3"
            onSubmit={(e) => {
              e.preventDefault()
              run(() => post(`/apps/${app.id}/ssl/custom`, { ...custom, force_https: force }), t('인증서를 설치했습니다')).then((ok) => ok && setCustom(null))
            }}
          >
            <Field label={t('인증서 (체인 포함)')} htmlFor="crt">
              <textarea
                id="crt"
                className="input h-32 font-mono text-[11px]"
                required
                placeholder="-----BEGIN CERTIFICATE-----"
                value={custom.certificate}
                onChange={(e) => setCustom({ ...custom, certificate: e.target.value })}
              />
            </Field>
            <Field label={t('개인키')} htmlFor="key">
              <textarea
                id="key"
                className="input h-32 font-mono text-[11px]"
                required
                placeholder="-----BEGIN PRIVATE KEY-----"
                value={custom.private_key}
                onChange={(e) => setCustom({ ...custom, private_key: e.target.value })}
              />
            </Field>
            <div className="flex justify-end gap-2">
              <button type="button" className="btn-ghost" onClick={() => setCustom(null)}>
                {t('취소')}
              </button>
              <button className="btn-primary" disabled={busy}>
                {t('설치')}
              </button>
            </div>
          </form>
        )}
      </section>
      {error && <ErrorBox error={error} />}
    </div>
  )
}

// ───────────────────────── SSH · SFTP ─────────────────────────

export function SiteSettings({ app, onSaved }) {
  const toast = useToast()
  const nav = useNavigate()
  const [v, setV] = useState({
    name: app.name,
    domains: app.domains,
    web_root: app.web_root,
    start_cmd: app.start_cmd ?? '',
    build_cmd: app.build_cmd ?? '',
    health_path: app.health_path,
    proxy_target: app.proxy_target ?? '',
    client_max_body_mb: app.client_max_body_mb,
  })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const set = (k) => (e) => setV({ ...v, [k]: e.target.value })

  return (
    <div className="space-y-6">
      <form
        className="card space-y-4 p-5"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            const n = await put(`/apps/${app.id}`, { ...v, client_max_body_mb: Number(v.client_max_body_mb) })
            onSaved(n)
            toast.push(t('저장했습니다 — 바로 적용합니다'), 'success')
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('이름')} htmlFor="s-name">
            <input id="s-name" className="input" required value={v.name} onChange={set('name')} />
          </Field>
          <Field label={t('업로드 크기 제한 (MB)')} htmlFor="s-body" hint="Nginx client_max_body_size">
            <input id="s-body" type="number" min="1" max="10240" className="input" value={v.client_max_body_mb} onChange={set('client_max_body_mb')} />
          </Field>
        </div>
        <Field label={t('도메인')} htmlFor="s-domains" hint={t('공백으로 구분. 대표 도메인(폴더 이름)은 바뀌지 않습니다')}>
          <input id="s-domains" className="input font-mono" required value={v.domains} onChange={set('domains')} />
        </Field>
        {app.kind === 'static' && (
          <Field label={t('웹 루트 (하위 폴더)')} htmlFor="s-webroot" hint={t('{site_root}/ 아래에서 서비스할 폴더 — 예: dist, public', { site_root: app.site_root })}>
            <input id="s-webroot" className="input font-mono" value={v.web_root} onChange={set('web_root')} placeholder={t('(문서 루트 그대로)')} />
          </Field>
        )}
        {(app.kind === 'rust' || app.kind === 'python') && (
          <div className="grid gap-4 sm:grid-cols-[1fr_180px]">
            <Field
              label={t('시작 명령')}
              htmlFor="s-start"
              hint={
                app.kind === 'python'
                  ? t('가상환경(.venv)이 켜진 상태로 실행 — 예: python app.py · gunicorn -b 127.0.0.1:$PORT app:app')
                  : t('문서 루트에 올린 실행 파일 — 예: ./app · PORT·BIND_ADDR 가 자동 주입됩니다')
              }
            >
              <input id="s-start" className="input font-mono" required value={v.start_cmd} onChange={set('start_cmd')} />
            </Field>
            <Field label={t('헬스체크 경로')} htmlFor="s-health">
              <input id="s-health" className="input font-mono" value={v.health_path} onChange={set('health_path')} />
            </Field>
          </div>
        )}
        {(app.kind === 'static' || app.kind === 'python') && (
          <Field
            label={t('적용 전 실행할 명령 (선택)')}
            htmlFor="s-build"
            hint={
              app.kind === 'python'
                ? t('패키지 설치 뒤 사이트 유저 권한으로 실행 — 예: python manage.py migrate && python manage.py collectstatic --noinput')
                : t('사이트 유저 권한으로 문서 루트에서 실행 — 예: npm ci && npm run build')
            }
          >
            <input id="s-build" className="input font-mono" value={v.build_cmd} onChange={set('build_cmd')} />
          </Field>
        )}
        {app.kind === 'proxy' && (
          <Field label={t('대상 URL')} htmlFor="s-target">
            <input id="s-target" className="input font-mono" required value={v.proxy_target} onChange={set('proxy_target')} />
          </Field>
        )}
        <ErrorBox error={error} />
        <div className="flex justify-end">
          <button className="btn-primary" disabled={busy}>
            {busy ? t('저장 중…') : t('저장하고 적용')}
          </button>
        </div>
      </form>
      <div className="card border-fail/30 p-5">
        <h3 className="font-medium text-fail">{t('사이트 삭제')}</h3>
        <p className="mt-1 text-sm text-ink-400">
          {app.site_user
            ? tx('Nginx 설정, 인증서, 사이트 유저 {user}와 홈 폴더의 모든 파일을 지웁니다. 백업 파일은 남습니다. 되돌릴 수 없습니다.', {
                user: <b className="font-mono">{app.site_user}</b>,
              })
            : t('Nginx 설정과 인증서를 지웁니다. 되돌릴 수 없습니다.')}
        </p>
        <button
          className="btn-danger mt-4"
          onClick={async () => {
            const ok = await confirmDialog(
              t('사이트 {slug} 을(를) 삭제합니다. Nginx 설정·인증서·사이트 파일이 지워지며 되돌릴 수 없습니다 (백업 파일은 남습니다).', { slug: app.slug }),
              {
                title: t('사이트 삭제'),
                requireText: app.slug,
                danger: true,
              },
            )
            if (!ok) return
            try {
              await del(`/apps/${app.id}`)
              toast.push(t('{slug} 삭제됨', { slug: app.slug }), 'success')
              nav('/sites')
            } catch (e) {
              toast.push(e.message, 'error')
            }
          }}
        >
          {t('사이트 삭제')}
        </button>
      </div>
    </div>
  )
}
