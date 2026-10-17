// DB 웹 관리 — MongoDB(컬렉션·문서) · Redis(키)
import { useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { ErrorBox, Modal, PageHeader, Spinner } from '../components/ui'
import { confirmDialog, promptDialog } from '../components/Dialog'
import { useToast } from '../context/ToastContext'
import { get, post } from '../lib/api'
import { useFetch } from '../lib/useFetch'
import { t } from '../i18n'

const pretty = (v) => JSON.stringify(v, null, 2)

/** JSON 문서 편집 창 */
function DocModal({ title, initial, onClose, onSave }) {
  const [text, setText] = useState(pretty(initial ?? {}))
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)
  return (
    <Modal open wide title={title} onClose={onClose}>
      <form
        className="space-y-3"
        onSubmit={async (e) => {
          e.preventDefault()
          let doc
          try {
            doc = JSON.parse(text)
          } catch (err) {
            setError(new Error(t('JSON 형식 오류: {e}', { e: err.message })))
            return
          }
          setBusy(true)
          setError(null)
          try {
            await onSave(doc)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <textarea className="input min-h-72 font-mono text-xs" value={text} onChange={(e) => setText(e.target.value)} aria-label={t('문서 (JSON)')} spellCheck={false} />
        <p className="text-xs text-ink-400">{t('ObjectId 는 {"$oid": "…"}, 날짜는 {"$date": "2026-01-01T00:00:00Z"} 처럼 씁니다.')}</p>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary" disabled={busy}>
            {t('저장')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

function Collection({ base, name, onChanged }) {
  const toast = useToast()
  const [filter, setFilter] = useState('')
  const [applied, setApplied] = useState('')
  const [page, setPage] = useState(1)
  const [editing, setEditing] = useState(null) // null | 'new' | doc
  const q = new URLSearchParams({ page: String(page), size: '20', ...(applied ? { filter: applied } : {}) })
  const d = useFetch(() => get(`${base}/c/${encodeURIComponent(name)}?${q}`), [name, page, applied])
  const pages = Math.max(1, Math.ceil((d.data?.total ?? 0) / 20))
  return (
    <div className="space-y-3">
      <form
        className="flex flex-wrap gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          setPage(1)
          setApplied(filter.trim())
        }}
      >
        <input
          className="input flex-1 font-mono text-xs"
          placeholder={'{ "status": "paid" }'}
          aria-label={t('찾기 조건 (JSON)')}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <button className="btn-ghost px-3 text-xs">{t('찾기')}</button>
        <button type="button" className="btn-primary px-3 text-xs" onClick={() => setEditing('new')}>
          {t('+ 문서 추가')}
        </button>
      </form>
      <ErrorBox error={d.error} />
      {d.loading && !d.data ? (
        <Spinner className="text-ink-400" />
      ) : (
        <>
          <p className="text-xs text-ink-400">{t('문서 {n}개', { n: d.data?.total ?? 0 })}</p>
          <ul className="space-y-2" data-testid="mongo-docs">
            {(d.data?.docs ?? []).map((doc, i) => (
              <li key={i} className="rounded-lg border border-ink-800 bg-ink-950/50">
                <div className="flex items-center gap-2 border-b border-ink-800 px-3 py-1.5 text-xs">
                  <span className="font-mono text-ink-400">_id {JSON.stringify(doc._id)}</span>
                  <span className="ml-auto flex gap-2">
                    <button className="text-signal hover:underline" onClick={() => setEditing(doc)}>
                      {t('수정')}
                    </button>
                    <button
                      className="text-red-300 hover:underline"
                      onClick={async () => {
                        if (!(await confirmDialog(t('이 문서를 삭제합니다. 되돌릴 수 없습니다.'), { title: t('문서 삭제'), danger: true }))) return
                        try {
                          await post(`${base}/c/${encodeURIComponent(name)}/delete`, { id: doc._id })
                          toast.push(t('삭제했습니다'), 'success')
                          d.reload(true)
                          onChanged()
                        } catch (e) {
                          toast.push(e.message, 'error')
                        }
                      }}
                    >
                      {t('삭제')}
                    </button>
                  </span>
                </div>
                <pre className="max-h-60 overflow-auto px-3 py-2 font-mono text-[11px] whitespace-pre-wrap text-ink-300">{pretty(doc)}</pre>
              </li>
            ))}
          </ul>
          <div className="flex items-center justify-end gap-2 text-xs">
            <button className="btn-ghost px-2 py-1" disabled={page <= 1} onClick={() => setPage(page - 1)}>
              ‹
            </button>
            <span>
              {page} / {pages}
            </span>
            <button className="btn-ghost px-2 py-1" disabled={page >= pages} onClick={() => setPage(page + 1)}>
              ›
            </button>
          </div>
        </>
      )}
      {editing && (
        <DocModal
          title={editing === 'new' ? t('{c} 에 문서 추가', { c: name }) : t('{c} 문서 수정', { c: name })}
          initial={editing === 'new' ? {} : editing}
          onClose={() => setEditing(null)}
          onSave={async (doc) => {
            if (editing === 'new') {
              await post(`${base}/c/${encodeURIComponent(name)}/insert`, { doc })
              toast.push(t('추가했습니다'), 'success')
            } else {
              await post(`${base}/c/${encodeURIComponent(name)}/update`, { id: editing._id, doc })
              toast.push(t('저장했습니다'), 'success')
            }
            setEditing(null)
            d.reload(true)
            onChanged()
          }}
        />
      )}
    </div>
  )
}

export function MongoWebPage() {
  const { id } = useParams()
  const toast = useToast()
  const base = `/dbweb/mongo/${id}`
  const d = useFetch(() => get(base), [id])
  const [coll, setColl] = useState(null)
  const cols = d.data?.collections ?? []
  return (
    <>
      <PageHeader
        title={t('DB 웹 관리')}
        subtitle={d.data ? `MongoDB · ${d.data.database}` : ''}
        help={t('이 데이터베이스의 기본 계정 권한으로 접속합니다. 컬렉션의 문서를 찾아보고 추가·수정·삭제할 수 있습니다.')}
        actions={
          <Link to="/mongodb" className="btn-ghost">
            {t('← MongoDB')}
          </Link>
        }
      />
      <ErrorBox error={d.error} />
      <div className="grid gap-4 lg:grid-cols-[14rem_1fr]">
        <aside className="card p-3">
          <button
            className="mb-2 w-full rounded-md px-2 py-1.5 text-left text-sm hover:bg-ink-800"
            onClick={async () => {
              const n = await promptDialog(t('새 컬렉션 이름'), { title: t('컬렉션 만들기') })
              if (!n) return
              try {
                await post(`${base}/collections`, { name: n })
                toast.push(t('만들었습니다'), 'success')
                d.reload(true)
                setColl(n)
              } catch (e) {
                toast.push(e.message, 'error')
              }
            }}
          >
            {t('+ 컬렉션')}
          </button>
          <ul className="space-y-0.5" data-testid="mongo-collections">
            {cols.map((c) => (
              <li key={c.name}>
                <button
                  className={`flex w-full justify-between gap-2 rounded-md px-2 py-1 text-left font-mono text-xs ${coll === c.name ? 'bg-signal/15 text-signal' : 'hover:bg-ink-800'}`}
                  onClick={() => setColl(c.name)}
                >
                  <span className="truncate">{c.name}</span>
                  <span className="text-ink-500">{c.count}</span>
                </button>
              </li>
            ))}
          </ul>
          {d.data && !cols.length && <p className="px-2 text-xs text-ink-500">{t('컬렉션이 없습니다')}</p>}
        </aside>
        <section className="card min-w-0 p-4">
          {coll ? (
            <>
              <h2 className="mb-3 font-mono text-sm font-medium">{coll}</h2>
              <Collection key={coll} base={base} name={coll} onChanged={() => d.reload(true)} />
            </>
          ) : (
            <p className="py-10 text-center text-sm text-ink-400">{t('왼쪽에서 컬렉션을 고르세요')}</p>
          )}
        </section>
      </div>
    </>
  )
}

// ───────── Redis ─────────

function RedisKey({ db, k, onChanged }) {
  const toast = useToast()
  const d = useFetch(() => get(`/dbweb/redis/${db}/key?k=${encodeURIComponent(k)}`), [db, k])
  const [value, setValue] = useState(null)
  if (d.loading && !d.data) return <Spinner className="text-ink-400" />
  if (d.error) return <ErrorBox error={d.error} />
  const v = d.data
  const editable = v.type === 'string'
  return (
    <div className="space-y-3">
      <p className="flex flex-wrap items-center gap-2 text-xs text-ink-400">
        <span className="kbd">{v.type}</span>
        {v.ttl != null ? t('{s}초 뒤 만료', { s: v.ttl }) : t('만료 없음')}
        <button
          className="ml-auto text-red-300 hover:underline"
          onClick={async () => {
            if (!(await confirmDialog(t('키 {k} 를 삭제합니다.', { k }), { title: t('키 삭제'), danger: true }))) return
            await post(`/dbweb/redis/${db}/delete`, { key: k })
            toast.push(t('삭제했습니다'), 'success')
            onChanged(null)
          }}
        >
          {t('삭제')}
        </button>
      </p>
      {editable ? (
        <>
          <textarea className="input min-h-40 font-mono text-xs" aria-label={t('값')} value={value ?? v.value} onChange={(e) => setValue(e.target.value)} />
          <button
            className="btn-primary"
            disabled={value == null}
            onClick={async () => {
              try {
                await post(`/dbweb/redis/${db}/set`, { key: k, value, ttl: v.ttl ?? undefined })
                toast.push(t('저장했습니다'), 'success')
                setValue(null)
                d.reload(true)
              } catch (e) {
                toast.push(e.message, 'error')
              }
            }}
          >
            {t('저장')}
          </button>
        </>
      ) : (
        <pre className="max-h-96 overflow-auto rounded-lg bg-ink-950 p-3 font-mono text-[11px] whitespace-pre-wrap text-ink-300">{pretty(v.value)}</pre>
      )}
    </div>
  )
}

export function RedisWebPage() {
  const toast = useToast()
  const [params, setParams] = useSearchParams()
  const dbs = useFetch(() => get('/dbweb/redis'), [])
  const list = dbs.data?.databases ?? []
  const db = params.get('db') ?? (list[0] ? String(list[0].db) : null)
  const [pattern, setPattern] = useState('*')
  const [applied, setApplied] = useState('*')
  const [key, setKey] = useState(null)
  const keys = useFetch(() => (db ? get(`/dbweb/redis/${db}/keys?pattern=${encodeURIComponent(applied)}`) : Promise.resolve(null)), [db, applied])
  return (
    <>
      <PageHeader
        title={t('DB 웹 관리')}
        subtitle="Redis"
        help={t('앱에 배정한 Redis DB 의 키를 보고, 문자열 값은 고치거나 지울 수 있습니다. 패널이 쓰는 DB 는 열지 않습니다.')}
        actions={
          <Link to="/redis" className="btn-ghost">
            {t('← Redis')}
          </Link>
        }
      />
      <ErrorBox error={dbs.error} />
      {dbs.data && !list.length && <p className="text-sm text-ink-400">{t('앱에 배정한 Redis DB 가 없습니다. 앱의 데이터베이스 탭에서 배정하세요.')}</p>}
      {list.length > 0 && (
        <div className="grid gap-4 lg:grid-cols-[16rem_1fr]">
          <aside className="card space-y-2 p-3">
            <select
              className="input font-mono text-xs"
              aria-label={t('Redis DB')}
              value={db ?? ''}
              onChange={(e) => {
                setKey(null)
                setParams({ db: e.target.value }, { replace: true })
              }}
            >
              {list.map((x) => (
                <option key={x.db} value={x.db}>
                  DB {x.db} · {x.app} ({x.keys})
                </option>
              ))}
            </select>
            <form
              className="flex gap-1"
              onSubmit={(e) => {
                e.preventDefault()
                setApplied(pattern || '*')
              }}
            >
              <input className="input font-mono text-xs" aria-label={t('키 패턴')} value={pattern} onChange={(e) => setPattern(e.target.value)} />
              <button className="btn-ghost px-2 text-xs">{t('찾기')}</button>
            </form>
            <button
              className="w-full rounded-md px-2 py-1 text-left text-xs text-signal hover:bg-ink-800"
              onClick={async () => {
                const k = await promptDialog(t('새 키 이름 (문자열 값)'), { title: t('키 추가') })
                if (!k) return
                try {
                  await post(`/dbweb/redis/${db}/set`, { key: k, value: '' })
                  keys.reload(true)
                  setKey(k)
                } catch (e) {
                  toast.push(e.message, 'error')
                }
              }}
            >
              {t('+ 키 추가')}
            </button>
            <ul className="max-h-[60vh] space-y-0.5 overflow-y-auto" data-testid="redis-keys">
              {(keys.data?.keys ?? []).map((x) => (
                <li key={x.key}>
                  <button
                    className={`flex w-full justify-between gap-2 rounded-md px-2 py-1 text-left font-mono text-xs ${key === x.key ? 'bg-signal/15 text-signal' : 'hover:bg-ink-800'}`}
                    onClick={() => setKey(x.key)}
                  >
                    <span className="truncate">{x.key}</span>
                    <span className="text-ink-500">{x.type}</span>
                  </button>
                </li>
              ))}
            </ul>
            {keys.data && !keys.data.keys.length && <p className="px-2 text-xs text-ink-500">{t('키가 없습니다')}</p>}
          </aside>
          <section className="card min-w-0 p-4">
            {key ? (
              <>
                <h2 className="mb-3 font-mono text-sm font-medium break-all">{key}</h2>
                <RedisKey
                  key={`${db}:${key}`}
                  db={db}
                  k={key}
                  onChanged={() => {
                    setKey(null)
                    keys.reload(true)
                  }}
                />
              </>
            ) : (
              <p className="py-10 text-center text-sm text-ink-400">{t('왼쪽에서 키를 고르세요')}</p>
            )}
          </section>
        </div>
      )}
    </>
  )
}
