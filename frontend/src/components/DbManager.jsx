// 데이터베이스 웹 관리 화면 (MariaDB) — 테이블 목록, 행 보기·추가·수정·삭제, SQL 실행
// 패널 관리자 화면과 DB 계정 로그인 화면이 같은 컴포넌트를 씁니다 (client 만 다름).
import { useEffect, useState } from 'react'
import { useFetch } from '../lib/useFetch'
import { ErrorBox, Modal, Spinner, Tabs } from './ui'
import { confirmDialog } from './Dialog'
import { useToast } from '../context/ToastContext'
import { bytes } from '../lib/format'
import { t } from '../i18n'

function Cell({ v }) {
  if (v === null || v === undefined) return <span className="text-ink-600 italic">NULL</span>
  const s = String(v)
  return <span title={s.length > 80 ? s : undefined}>{s.length > 80 ? s.slice(0, 80) + '…' : s}</span>
}

function ResultTable({ columns, rows, onEdit, onDelete }) {
  return (
    <div className="overflow-x-auto rounded-lg border border-ink-800">
      <table className="w-full text-xs" data-testid="db-rows">
        <thead className="bg-ink-900 text-left text-ink-400">
          <tr>
            {(onEdit || onDelete) && <th className="w-px px-2 py-1.5" />}
            {columns.map((c) => (
              <th key={c} className="px-2 py-1.5 font-mono font-medium whitespace-nowrap">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-ink-800 font-mono">
          {rows.length === 0 && (
            <tr>
              <td colSpan={columns.length + 1} className="px-2 py-4 text-center text-ink-500">
                {t('행이 없습니다')}
              </td>
            </tr>
          )}
          {rows.map((r, i) => (
            <tr key={i} className="hover:bg-ink-900/60">
              {(onEdit || onDelete) && (
                <td className="px-2 py-1 whitespace-nowrap">
                  {onEdit && (
                    <button className="text-signal hover:underline" onClick={() => onEdit(r)}>
                      {t('수정')}
                    </button>
                  )}
                  {onDelete && (
                    <button className="ml-2 text-red-300 hover:underline" onClick={() => onDelete(r)}>
                      {t('삭제')}
                    </button>
                  )}
                </td>
              )}
              {r.map((v, j) => (
                <td key={j} className="max-w-xs px-2 py-1 whitespace-nowrap">
                  <Cell v={v} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

/** 행 추가·수정 창 */
function RowModal({ table, columns, row, onClose, onSave }) {
  const init = {}
  const nulls = {}
  columns.forEach((c, i) => {
    const v = row ? row[i] : null
    init[c.name] = v === null || v === undefined ? '' : String(v)
    nulls[c.name] = row ? v === null : false
  })
  const [vals, setVals] = useState(init)
  const [isNull, setIsNull] = useState(nulls)
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)
  return (
    <Modal open wide onClose={onClose} title={row ? t('{table} 행 수정', { table }) : t('{table} 행 추가', { table })}>
      <form
        className="space-y-3"
        onSubmit={async (e) => {
          e.preventDefault()
          const values = {}
          columns.forEach((c, i) => {
            const v = isNull[c.name] ? null : vals[c.name]
            if (row) {
              // 바뀐 값만 보냄
              const old = row[i] === undefined || row[i] === null ? null : String(row[i])
              if (v !== old) values[c.name] = v
            } else if (v !== '') {
              // 비워 둔 칸은 보내지 않음 → 기본값·자동 증가
              values[c.name] = v
            }
          })
          setBusy(true)
          setError(null)
          try {
            await onSave(values)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <div className="max-h-[55vh] space-y-2.5 overflow-y-auto pr-1">
          {columns.map((c) => (
            <div key={c.name} className="grid gap-1 sm:grid-cols-[10rem_1fr] sm:items-center">
              <label className="truncate font-mono text-xs" htmlFor={`col-${c.name}`} title={c.type}>
                {c.name}
                {c.key === 'PRI' && <span className="ml-1 text-amber-300">PK</span>}
                <span className="ml-1 text-ink-500">{c.type}</span>
              </label>
              <div className="flex items-center gap-2">
                <input
                  id={`col-${c.name}`}
                  className="input font-mono text-xs"
                  value={isNull[c.name] ? '' : vals[c.name]}
                  disabled={isNull[c.name]}
                  placeholder={String(c.extra || '').includes('auto_increment') ? 'auto' : c.default != null ? `default: ${c.default}` : ''}
                  onChange={(e) => setVals({ ...vals, [c.name]: e.target.value })}
                />
                {c.nullable && (
                  <label className="flex items-center gap-1 text-xs whitespace-nowrap text-ink-400">
                    <input type="checkbox" checked={!!isNull[c.name]} onChange={(e) => setIsNull({ ...isNull, [c.name]: e.target.checked })} />
                    NULL
                  </label>
                )}
              </div>
            </div>
          ))}
        </div>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className="btn-primary" disabled={busy}>
            {row ? t('저장') : t('추가')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

function Browse({ client, table }) {
  const [page, setPage] = useState(1)
  const [order, setOrder] = useState(null)
  const [desc, setDesc] = useState(false)
  const [editing, setEditing] = useState(null) // null | 'new' | row
  const [view, setView] = useState('rows')
  const toast = useToast()
  const size = 50
  const d = useFetch(() => client.browse(table, { page, size, order, desc }), [table, page, order, desc])
  useEffect(() => {
    setPage(1)
    setOrder(null)
    setDesc(false)
    setView('rows')
  }, [table])
  if (d.loading && !d.data) return <Spinner className="text-ink-400" />
  if (d.error) return <ErrorBox error={d.error} />
  const b = d.data
  const pk = b.primary_key || []
  const names = b.columns.map((c) => c.name)
  const keyOf = (r) => Object.fromEntries(pk.map((k) => [k, r[names.indexOf(k)]]))
  const pages = Math.max(1, Math.ceil(b.total / b.size))
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <Tabs
          tabs={[
            { id: 'rows', label: t('데이터') },
            { id: 'structure', label: t('구조') },
          ]}
          active={view}
          onChange={setView}
        />
        <span className="text-xs text-ink-400">{t('{n}행', { n: b.total })}</span>
        <span className="ml-auto flex gap-1.5">
          <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => d.reload(true)}>
            {t('새로고침')}
          </button>
          <button className="btn-primary px-2.5 py-1 text-xs" onClick={() => setEditing('new')}>
            {t('+ 행 추가')}
          </button>
        </span>
      </div>
      {view === 'structure' ? (
        <ResultTable
          columns={[t('컬럼'), t('형식'), 'NULL', t('키'), t('기본값'), t('추가'), t('설명')]}
          rows={b.columns.map((c) => [c.name, c.type, c.nullable ? 'YES' : 'NO', c.key || '', c.default, c.extra || '', c.comment || ''])}
        />
      ) : (
        <>
          {pk.length === 0 && <p className="text-xs text-amber-300">{t('기본 키(PRIMARY KEY)가 없는 테이블은 화면에서 수정·삭제할 수 없습니다 — SQL 실행을 쓰세요')}</p>}
          <div className="flex flex-wrap items-center gap-2 text-xs text-ink-400">
            {t('정렬')}
            <select
              className="input w-auto py-1 text-xs"
              value={b.order || ''}
              onChange={(e) => {
                setOrder(e.target.value || null)
                setPage(1)
              }}
            >
              {names.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
            <label className="flex items-center gap-1">
              <input type="checkbox" checked={desc} onChange={(e) => setDesc(e.target.checked)} />
              {t('내림차순')}
            </label>
          </div>
          <ResultTable
            columns={names}
            rows={b.rows}
            onEdit={pk.length ? (r) => setEditing(r) : null}
            onDelete={
              pk.length
                ? async (r) => {
                    const key = keyOf(r)
                    const ok = await confirmDialog(t('이 행을 삭제합니다. 되돌릴 수 없습니다.') + '\n' + JSON.stringify(key), { title: t('행 삭제'), danger: true })
                    if (!ok) return
                    try {
                      await client.row(table, 'delete', { key })
                      toast.push(t('삭제했습니다'), 'success')
                      d.reload(true)
                    } catch (e) {
                      toast.push(e.message, 'error')
                    }
                  }
                : null
            }
          />
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
        <RowModal
          table={table}
          columns={b.columns}
          row={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
          onSave={async (values) => {
            if (editing === 'new') {
              await client.row(table, 'insert', { values })
              toast.push(t('추가했습니다'), 'success')
            } else {
              if (Object.keys(values).length === 0) {
                setEditing(null)
                return
              }
              await client.row(table, 'update', { key: keyOf(editing), values })
              toast.push(t('저장했습니다'), 'success')
            }
            setEditing(null)
            d.reload(true)
          }}
        />
      )}
    </div>
  )
}

function SqlRunner({ client, onChanged }) {
  const [sql, setSql] = useState('')
  const [res, setRes] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const run = async () => {
    setBusy(true)
    setError(null)
    try {
      const r = await client.query(sql)
      setRes(r)
      if (!r.error && !r.columns?.length) onChanged?.()
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="space-y-3">
      <textarea
        className="input min-h-32 font-mono text-xs"
        placeholder="SELECT * FROM … LIMIT 100"
        value={sql}
        data-testid="sql-input"
        onChange={(e) => setSql(e.target.value)}
        onKeyDown={(e) => {
          if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') run()
        }}
      />
      <div className="flex items-center gap-3">
        <button className="btn-primary" disabled={busy || !sql.trim()} onClick={run}>
          {busy ? t('실행 중…') : t('실행')}
        </button>
        <span className="text-xs text-ink-500">{t('Ctrl+Enter 로 실행 · 결과는 최대 1000행 · 30초 제한')}</span>
      </div>
      <ErrorBox error={error} />
      {res?.error && <ErrorBox error={{ message: res.error }} />}
      {res && !res.error && (
        <div className="space-y-2">
          <p className="text-xs text-ink-400" data-testid="sql-result">
            {res.columns.length
              ? t('{n}행 · {ms}ms', { n: res.rows.length, ms: res.elapsed_ms }) + (res.truncated ? ' · ' + t('1000행까지만 표시') : '')
              : t('적용된 행 {n}개 · {ms}ms', { n: res.affected, ms: res.elapsed_ms })}
          </p>
          {res.columns.length > 0 && <ResultTable columns={res.columns} rows={res.rows} />}
        </div>
      )}
    </div>
  )
}

/**
 * client: { tables(), browse(table, q), row(table, op, body), query(sql) }
 */
export default function DbManager({ client, database, header }) {
  const d = useFetch(() => client.tables(), [database])
  const [table, setTable] = useState(null)
  const [mode, setMode] = useState('browse')
  useEffect(() => {
    setTable(null)
    setMode('browse')
  }, [database])
  const tables = d.data?.tables || []
  return (
    <div className="grid gap-4 lg:grid-cols-[14rem_1fr]">
      <aside className="card p-3">
        {header}
        <button
          className={`mb-2 w-full rounded-md px-2 py-1.5 text-left text-sm ${mode === 'sql' ? 'bg-signal/15 text-signal' : 'hover:bg-ink-800'}`}
          onClick={() => setMode('sql')}
        >
          ▶ {t('SQL 실행')}
        </button>
        <p className="px-2 pt-1 pb-1 text-xs text-ink-500">{t('테이블 {n}개', { n: tables.length })}</p>
        {d.loading && <Spinner className="text-ink-400" />}
        <ErrorBox error={d.error} />
        <ul className="max-h-[60vh] space-y-0.5 overflow-y-auto" data-testid="db-tables">
          {tables.map((tb) => (
            <li key={tb.name}>
              <button
                className={`flex w-full items-center justify-between gap-2 rounded-md px-2 py-1 text-left font-mono text-xs ${mode === 'browse' && table === tb.name ? 'bg-signal/15 text-signal' : 'hover:bg-ink-800'}`}
                onClick={() => {
                  setTable(tb.name)
                  setMode('browse')
                }}
              >
                <span className="truncate">{tb.name}</span>
                <span className="text-ink-500">{tb.size != null ? bytes(Number(tb.size)) : ''}</span>
              </button>
            </li>
          ))}
        </ul>
        {!d.loading && tables.length === 0 && !d.error && <p className="px-2 text-xs text-ink-500">{t('테이블이 없습니다')}</p>}
      </aside>
      <section className="card min-w-0 p-4">
        {mode === 'sql' ? (
          <SqlRunner client={client} onChanged={() => d.reload(true)} />
        ) : table ? (
          <>
            <h2 className="mb-3 font-mono text-sm font-medium">{table}</h2>
            <Browse client={client} table={table} />
          </>
        ) : (
          <p className="py-10 text-center text-sm text-ink-400">{t('왼쪽에서 테이블을 고르거나 SQL 을 실행하세요')}</p>
        )}
      </section>
    </div>
  )
}

/** 쿼리 문자열 */
export function qs(q) {
  const p = new URLSearchParams()
  Object.entries(q).forEach(([k, v]) => v !== null && v !== undefined && p.set(k, String(v)))
  return p.toString()
}
