import { useState } from 'react'
import LogViewer from '../components/LogViewer'
import { CopyButton, ErrorBox, Field, Spinner, StatusBadge } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { api, del, download, get, post, put } from '../lib/api'
import { ago, bytes, CRON_PRESETS, dateTime } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'

function Section({ title, desc, actions, children }) {
  return (
    <section className="card p-5">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="font-medium">{title}</h3>
          {desc && <p className="mt-1 max-w-2xl text-sm text-ink-400">{desc}</p>}
        </div>
        {actions && <div className="flex gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  )
}

function Secret({ value }) {
  const [show, setShow] = useState(false)
  return (
    <div className="flex items-center gap-2">
      <code className="min-w-0 flex-1 truncate rounded bg-ink-950 px-2 py-1.5 font-mono text-xs text-ink-300">
        {show ? value : value.replace(/:\/\/([^:]*):[^@]+@/, '://$1:••••••@')}
      </code>
      <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setShow((s) => !s)}>
        {show ? '숨기기' : '보기'}
      </button>
      <CopyButton text={value} />
    </div>
  )
}

// ───────────────────────── 데이터베이스 · Redis ─────────────────────────

export function DatabaseTab({ app, onChanged }) {
  const toast = useToast()
  const data = useFetch(() => get(`/apps/${app.id}/databases`), [app.id])
  const [form, setForm] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  if (data.loading) return <Spinner className="text-ink-400" />
  const d = data.data

  const act = async (fn, okMsg) => {
    setBusy(true)
    try {
      await fn()
      if (okMsg) toast.push(okMsg, 'success')
      data.reload(true)
      onChanged?.()
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-4">
      <Section
        title="MariaDB"
        desc="DB와 전용 계정을 만들고 접속 URL을 환경변수로 자동 주입합니다 (기본 DATABASE_URL). 계정은 이 DB에만 권한이 있습니다. 만든 뒤 재배포해야 앱에 반영됩니다."
        actions={
          !form && (
            <button className="btn-primary" onClick={() => setForm({ name: d.suggested_name, username: '', env_key: 'DATABASE_URL' })}>
              + 데이터베이스
            </button>
          )
        }
      >
        {form && (
          <form
            className="mb-5 grid gap-3 rounded-lg border border-ink-700 p-4 sm:grid-cols-3"
            onSubmit={async (e) => {
              e.preventDefault()
              setBusy(true)
              setError(null)
              try {
                await post(`/apps/${app.id}/databases`, form)
                toast.push(`데이터베이스 ${form.name} 생성됨 — 재배포하면 ${form.env_key} 가 주입됩니다`, 'success')
                setForm(null)
                data.reload(true)
              } catch (err) {
                setError(err)
              } finally {
                setBusy(false)
              }
            }}
          >
            <Field label="DB 이름" htmlFor="db-name">
              <input id="db-name" className="input font-mono" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </Field>
            <Field label="사용자 (비우면 DB 이름)" htmlFor="db-user">
              <input id="db-user" className="input font-mono" value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} />
            </Field>
            <Field label="환경변수 이름" htmlFor="db-env">
              <input id="db-env" className="input font-mono" value={form.env_key} onChange={(e) => setForm({ ...form, env_key: e.target.value })} />
            </Field>
            <div className="flex items-center justify-end gap-2 sm:col-span-3">
              <button type="button" className="btn-ghost" onClick={() => setForm(null)}>
                취소
              </button>
              <button className="btn-primary" disabled={busy}>
                {busy ? '만드는 중…' : '만들기'}
              </button>
            </div>
            {error && (
              <div className="sm:col-span-3">
                <ErrorBox error={error} />
              </div>
            )}
          </form>
        )}

        {d.databases.length === 0 ? (
          <p className="text-sm text-ink-400">연결된 데이터베이스가 없습니다.</p>
        ) : (
          <ul className="divide-y divide-ink-800">
            {d.databases.map((db) => (
              <li key={db.id} className="py-3">
                <div className="mb-2 flex flex-wrap items-center gap-3 text-sm">
                  <span className="font-mono font-medium">{db.name}</span>
                  <span className="text-xs text-ink-400">사용자 {db.username}</span>
                  <span className="kbd">{db.env_key}</span>
                  <span className="ml-auto flex gap-2">
                    <button
                      className="btn-ghost px-2.5 py-1 text-xs"
                      disabled={busy}
                      onClick={() =>
                        window.confirm('비밀번호를 바꾸면 재배포 전까지 앱이 DB에 접속하지 못할 수 있습니다. 계속할까요?') &&
                        act(() => post(`/apps/${app.id}/databases/${db.id}/password`), '비밀번호 교체됨 — 재배포하세요')
                      }
                    >
                      비밀번호 교체
                    </button>
                    <button
                      className="btn-danger px-2.5 py-1 text-xs"
                      disabled={busy}
                      onClick={() => {
                        const typed = window.prompt(`DB와 모든 데이터가 삭제됩니다. 삭제하려면 DB 이름을 입력하세요: ${db.name}`)
                        if (typed === db.name) act(() => del(`/apps/${app.id}/databases/${db.id}`), `${db.name} 삭제됨`)
                      }}
                    >
                      삭제
                    </button>
                  </span>
                </div>
                <Secret value={db.url} />
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section
        title="Redis"
        desc="이 앱 전용 Redis DB 번호를 배정하고 REDIS_URL 을 자동 주입합니다. 번호마다 키 공간이 분리되어 다른 앱의 FLUSHDB 에 영향받지 않습니다."
        actions={
          !d.redis && (
            <button className="btn-primary" disabled={busy} onClick={() => act(() => post(`/apps/${app.id}/redis`), 'Redis DB 배정됨 — 재배포하세요')}>
              Redis 배정
            </button>
          )
        }
      >
        {d.redis ? (
          <div className="space-y-3">
            <div className="flex flex-wrap items-center gap-3 text-sm">
              <span>
                DB <b className="font-mono">{d.redis.db}</b>
              </span>
              <span className="text-xs text-ink-400">키 {d.redis.keys ?? '?'}개</span>
              <span className="ml-auto flex gap-2">
                <button
                  className="btn-ghost px-2.5 py-1 text-xs"
                  disabled={busy}
                  onClick={() => window.confirm(`DB ${d.redis.db} 의 키를 모두 지울까요?`) && act(() => post(`/apps/${app.id}/redis/flush`), '비웠습니다')}
                >
                  비우기 (FLUSHDB)
                </button>
                <button
                  className="btn-danger px-2.5 py-1 text-xs"
                  disabled={busy}
                  onClick={() =>
                    window.confirm('배정을 해제하고 키도 지울까요?') &&
                    act(() => api(`/apps/${app.id}/redis`, { method: 'DELETE', body: { flush: true } }), '해제했습니다')
                  }
                >
                  해제
                </button>
              </span>
            </div>
            <Secret value={d.redis.url} />
          </div>
        ) : (
          <p className="text-sm text-ink-400">배정된 Redis DB가 없습니다.</p>
        )}
      </Section>
    </div>
  )
}

// ───────────────────────── 백업 ─────────────────────────

export function BackupTab({ app, onChanged }) {
  const toast = useToast()
  const list = useFetch(() => get(`/backups?app_id=${app.id}`), [app.id])
  const [schedule, setSchedule] = useState(app.backup_schedule)
  const [keep, setKeep] = useState(app.backup_keep)
  const [busy, setBusy] = useState(false)
  const running = list.data?.some((b) => b.status === 'running')
  useInterval(() => list.reload(true), 2000, running)

  return (
    <div className="space-y-4">
      <Section
        title="백업"
        desc={`연결된 DB 덤프(.sql.gz)와 ${app.site_user ? '사이트 파일(htdocs, .tar.gz)을 ' : app.kind === 'rust' || app.kind === 'node' ? '앱 data 디렉토리(.tar.gz)를 ' : ''}/var/lib/rpanel/backups/${app.slug}/ 에 저장합니다. NAS·다른 서버가 이 폴더를 가져가도록(rsync 등) 해두면 오프사이트 백업이 됩니다.`}
        actions={
          <button
            className="btn-primary"
            disabled={busy || running}
            onClick={async () => {
              setBusy(true)
              try {
                await post(`/apps/${app.id}/backups`)
                toast.push('백업을 시작했습니다')
                list.reload(true)
              } catch (e) {
                toast.push(e.message, 'error')
              } finally {
                setBusy(false)
              }
            }}
          >
            {running ? '백업 중…' : '지금 백업'}
          </button>
        }
      >
        <form
          className="flex flex-wrap items-end gap-3 rounded-lg border border-ink-700 p-4"
          onSubmit={async (e) => {
            e.preventDefault()
            try {
              const r = await put(`/apps/${app.id}/backup-settings`, { schedule, keep: Number(keep) })
              toast.push(r.schedule === 'off' ? '예약 백업을 껐습니다' : `예약 백업: ${r.schedule === 'daily' ? '매일' : '매주'} ${r.hour}시, ${r.keep}개 보관`, 'success')
              onChanged?.()
            } catch (err) {
              toast.push(err.message, 'error')
            }
          }}
        >
          <Field label="예약" htmlFor="bk-schedule">
            <select id="bk-schedule" className="input w-36" value={schedule} onChange={(e) => setSchedule(e.target.value)}>
              <option value="off">끄기</option>
              <option value="daily">매일</option>
              <option value="weekly">매주</option>
            </select>
          </Field>
          <Field label="대상별 보관 개수" htmlFor="bk-keep">
            <input id="bk-keep" type="number" min="1" max="100" className="input w-28" value={keep} onChange={(e) => setKeep(e.target.value)} />
          </Field>
          <button className="btn-ghost">저장</button>
        </form>
      </Section>
      <BackupTable rows={list.data} loading={list.loading} onChanged={() => list.reload(true)} />
    </div>
  )
}

export function BackupTable({ rows, loading, onChanged, showApp = false }) {
  const toast = useToast()
  if (loading) return <Spinner className="text-ink-400" />
  if (!rows?.length) return <p className="card px-5 py-8 text-center text-sm text-ink-400">백업이 없습니다</p>
  return (
    <div className="card overflow-x-auto">
      <table className="w-full text-sm">
        <thead className="border-b border-ink-700/70 text-left text-xs text-ink-400">
          <tr>
            {showApp && <th className="px-5 py-3 font-medium">앱</th>}
            <th className="px-5 py-3 font-medium">대상</th>
            <th className="px-3 py-3 font-medium">상태</th>
            <th className="px-3 py-3 font-medium">크기</th>
            <th className="px-3 py-3 font-medium">시각</th>
            <th className="px-5 py-3" />
          </tr>
        </thead>
        <tbody className="divide-y divide-ink-800">
          {rows.map((b) => (
            <tr key={b.id} className="hover:bg-ink-850">
              {showApp && <td className="px-5 py-3 font-medium">{b.app_name}</td>}
              <td className="px-5 py-3">
                <span className="kbd mr-2">{b.kind === 'db' ? 'DB' : b.kind === 'files' ? '파일' : 'data'}</span>
                <span className="font-mono text-xs">{b.target}</span>
                {b.trigger_by === 'schedule' && <span className="ml-2 text-[11px] text-ink-400">예약</span>}
                {b.error && <p className="mt-1 max-w-md text-xs text-red-300">{b.error}</p>}
              </td>
              <td className="px-3 py-3">
                <StatusBadge status={b.status} deployment />
              </td>
              <td className="px-3 py-3 font-mono text-xs text-ink-300">{b.status === 'success' ? bytes(b.size_bytes) : '–'}</td>
              <td className="px-3 py-3 text-xs text-ink-400" title={dateTime(b.created_at)}>
                {ago(b.created_at)}
              </td>
              <td className="px-5 py-3 text-right whitespace-nowrap">
                {b.status === 'success' && (
                  <>
                    <button
                      className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                      onClick={() => download(`/backups/${b.id}/download`, `${b.app_slug}-${b.file}`).catch((e) => toast.push(e.message, 'error'))}
                    >
                      다운로드
                    </button>
                    <button
                      className="btn-ghost mr-1.5 px-2.5 py-1 text-xs"
                      onClick={async () => {
                        const what = b.kind === 'db' ? `DB ${b.target} 의 현재 데이터를 이 백업으로 덮어씁니다` : b.kind === 'files' ? '사이트 파일(htdocs)을 이 백업으로 통째로 교체합니다' : 'data 디렉토리를 이 백업으로 교체합니다 (서비스 잠시 중지)'
                        if (!window.confirm(`${what}. 계속할까요?`)) return
                        try {
                          await post(`/backups/${b.id}/restore`)
                          toast.push('복원했습니다', 'success')
                        } catch (e) {
                          toast.push(e.message, 'error')
                        }
                      }}
                    >
                      복원
                    </button>
                  </>
                )}
                {b.status !== 'running' && (
                  <button
                    className="btn-ghost px-2.5 py-1 text-xs"
                    aria-label="백업 삭제"
                    onClick={async () => {
                      if (!window.confirm('이 백업 파일을 삭제할까요?')) return
                      try {
                        await del(`/backups/${b.id}`)
                        onChanged()
                      } catch (e) {
                        toast.push(e.message, 'error')
                      }
                    }}
                  >
                    ✕
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// ───────────────────────── 크론 ─────────────────────────

const emptyJob = { name: '', schedule: '*-*-* 03:00:00', command: '', enabled: true }

function CronForm({ initial, onSubmit, onCancel }) {
  const [v, setV] = useState(initial)
  const [check, setCheck] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  const verify = async (schedule) => {
    try {
      setCheck(await post('/cron/preview', { schedule }))
    } catch {
      setCheck(null)
    }
  }

  return (
    <form
      className="space-y-3 rounded-lg border border-ink-700 p-4"
      onSubmit={async (e) => {
        e.preventDefault()
        setBusy(true)
        setError(null)
        try {
          await onSubmit(v)
        } catch (err) {
          setError(err)
        } finally {
          setBusy(false)
        }
      }}
    >
      <div className="grid gap-3 sm:grid-cols-[1fr_1.4fr]">
        <Field label="이름" htmlFor="cron-name">
          <input id="cron-name" className="input" required value={v.name} onChange={(e) => setV({ ...v, name: e.target.value })} placeholder="오래된 세션 정리" />
        </Field>
        <Field label="일정 (systemd OnCalendar)" htmlFor="cron-schedule">
          <input
            id="cron-schedule"
            className="input font-mono"
            required
            value={v.schedule}
            onChange={(e) => {
              setV({ ...v, schedule: e.target.value })
              setCheck(null)
            }}
            onBlur={(e) => verify(e.target.value)}
          />
        </Field>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {CRON_PRESETS.map((p) => (
          <button
            type="button"
            key={p.value}
            className={`rounded-full border px-2.5 py-0.5 text-xs ${v.schedule === p.value ? 'border-signal/60 text-signal' : 'border-ink-700 text-ink-300 hover:border-ink-600'}`}
            onClick={() => {
              setV({ ...v, schedule: p.value })
              verify(p.value)
            }}
          >
            {p.label}
          </button>
        ))}
        {check && (
          <span className={`ml-2 text-xs ${check.valid ? 'text-signal' : 'text-fail'}`}>
            {check.valid ? (check.next ? `다음 실행: ${check.next}` : '형식 OK') : check.error}
          </span>
        )}
      </div>
      <Field label="명령" htmlFor="cron-cmd" hint="앱의 현재 릴리스 폴더에서, 앱과 같은 환경변수로 rpanel-app 사용자가 실행합니다">
        <input id="cron-cmd" className="input font-mono" required value={v.command} onChange={(e) => setV({ ...v, command: e.target.value })} placeholder="./beripick-api --task cleanup" />
      </Field>
      <label className="flex items-center gap-2 text-sm text-ink-300">
        <input type="checkbox" checked={v.enabled} onChange={(e) => setV({ ...v, enabled: e.target.checked })} /> 활성화
      </label>
      <ErrorBox error={error} />
      <div className="flex justify-end gap-2">
        <button type="button" className="btn-ghost" onClick={onCancel}>
          취소
        </button>
        <button className="btn-primary" disabled={busy}>
          {busy ? '저장 중…' : '저장'}
        </button>
      </div>
    </form>
  )
}

export function CronTab({ app }) {
  const toast = useToast()
  const jobs = useFetch(() => get(`/apps/${app.id}/cron`), [app.id])
  const [editing, setEditing] = useState(null) // 'new' | job
  const [logFor, setLogFor] = useState(null)
  const [logLines, setLogLines] = useState([])

  const showLog = async (job) => {
    setLogFor(job.id)
    try {
      const r = await get(`/cron/${job.id}/log`)
      setLogLines(r.lines.map((text, i) => ({ seq: i + 1, text })))
    } catch (e) {
      toast.push(e.message, 'error')
    }
  }

  return (
    <div className="space-y-4">
      <Section
        title="크론 작업"
        desc="systemd timer 로 등록되어 패널이 꺼져 있어도 실행됩니다. 실행 결과는 journal 에 남습니다."
        actions={
          editing === null && (
            <button className="btn-primary" disabled={!app.current_release} title={app.current_release ? '' : '먼저 배포하세요'} onClick={() => setEditing('new')}>
              + 작업
            </button>
          )
        }
      >
        {!app.current_release && <p className="mb-3 text-sm text-warn">앱을 먼저 배포해야 크론을 등록할 수 있습니다.</p>}
        {editing === 'new' && (
          <div className="mb-4">
            <CronForm
              initial={emptyJob}
              onCancel={() => setEditing(null)}
              onSubmit={async (v) => {
                await post(`/apps/${app.id}/cron`, v)
                toast.push('크론 작업을 등록했습니다', 'success')
                setEditing(null)
                jobs.reload(true)
              }}
            />
          </div>
        )}
        {jobs.loading ? (
          <Spinner className="text-ink-400" />
        ) : !jobs.data?.length ? (
          editing === null && <p className="text-sm text-ink-400">등록된 작업이 없습니다.</p>
        ) : (
          <ul className="divide-y divide-ink-800">
            {jobs.data.map((j) =>
              editing?.id === j.id ? (
                <li key={j.id} className="py-3">
                  <CronForm
                    initial={{ name: j.name, schedule: j.schedule, command: j.command, enabled: j.enabled }}
                    onCancel={() => setEditing(null)}
                    onSubmit={async (v) => {
                      await put(`/cron/${j.id}`, v)
                      toast.push('저장했습니다', 'success')
                      setEditing(null)
                      jobs.reload(true)
                    }}
                  />
                </li>
              ) : (
                <li key={j.id} className="py-3">
                  <div className="flex flex-wrap items-center gap-3 text-sm">
                    <span className={`h-2 w-2 rounded-full ${j.enabled ? 'bg-signal' : 'bg-ink-600'}`} title={j.enabled ? '활성' : '비활성'} />
                    <span className="font-medium">{j.name}</span>
                    <span className="kbd">{j.schedule}</span>
                    {j.status?.Result && (
                      <span className={`text-xs ${j.status.Result === 'success' ? 'text-signal' : 'text-fail'}`}>
                        최근 결과: {j.status.Result}
                        {j.status.ExecMainStatus && j.status.ExecMainStatus !== '0' ? ` (exit ${j.status.ExecMainStatus})` : ''}
                      </span>
                    )}
                    <span className="ml-auto flex gap-1.5">
                      <button
                        className="btn-ghost px-2.5 py-1 text-xs"
                        disabled={!j.enabled}
                        onClick={async () => {
                          try {
                            await post(`/cron/${j.id}/run`)
                            toast.push(`'${j.name}' 실행을 시작했습니다`)
                            setTimeout(() => showLog(j), 1500)
                          } catch (e) {
                            toast.push(e.message, 'error')
                          }
                        }}
                      >
                        지금 실행
                      </button>
                      <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => (logFor === j.id ? setLogFor(null) : showLog(j))}>
                        로그
                      </button>
                      <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setEditing(j)}>
                        수정
                      </button>
                      <button
                        className="btn-ghost px-2.5 py-1 text-xs"
                        aria-label={`${j.name} 삭제`}
                        onClick={async () => {
                          if (!window.confirm(`'${j.name}' 작업을 삭제할까요?`)) return
                          try {
                            await del(`/cron/${j.id}`)
                            jobs.reload(true)
                          } catch (e) {
                            toast.push(e.message, 'error')
                          }
                        }}
                      >
                        ✕
                      </button>
                    </span>
                  </div>
                  <code className="mt-1.5 block truncate font-mono text-xs text-ink-400">$ {j.command}</code>
                  {j.status?.NextElapseUSecRealtime && <p className="mt-1 text-xs text-ink-400">다음 실행: {j.status.NextElapseUSecRealtime}</p>}
                  {logFor === j.id && (
                    <div className="mt-3">
                      <LogViewer lines={logLines} height="h-64" emptyText="아직 실행 기록이 없습니다" />
                    </div>
                  )}
                </li>
              ),
            )}
          </ul>
        )}
      </Section>
    </div>
  )
}

// ───────────────────────── Nginx 로그 ─────────────────────────

export function NginxLogTab({ app }) {
  const [kind, setKind] = useState('access')
  const [n, setN] = useState(500)
  const logs = useFetch(() => get(`/apps/${app.id}/nginx-logs?kind=${kind}&lines=${n}`), [app.id, kind, n])
  const stats = logs.data?.stats
  const lines = (logs.data?.lines ?? []).map((text, i) => ({ seq: i + 1, text }))
  const total = stats?.total || 0

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        {['access', 'error'].map((k) => (
          <button
            key={k}
            className={`rounded-lg border px-3 py-1.5 text-sm ${kind === k ? 'border-signal/60 text-ink-100' : 'border-ink-700 text-ink-400'}`}
            onClick={() => setKind(k)}
          >
            {k === 'access' ? '접속 로그' : '에러 로그'}
          </button>
        ))}
        <select className="input ml-auto w-auto py-1 text-xs" value={n} onChange={(e) => setN(Number(e.target.value))} aria-label="줄 수">
          {[200, 500, 2000, 5000].map((x) => (
            <option key={x} value={x}>
              최근 {x}줄
            </option>
          ))}
        </select>
        <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => logs.reload()}>
          새로고침
        </button>
      </div>

      {kind === 'access' && stats && total > 0 && (
        <div className="grid gap-4 lg:grid-cols-3">
          <div className="card p-5">
            <p className="text-xs text-ink-400">요청 {total.toLocaleString()}건 · {bytes(stats.bytes)}</p>
            <div className="mt-3 flex h-3 overflow-hidden rounded-full bg-ink-800" aria-label="상태코드 분포">
              {['2xx', '3xx', '4xx', '5xx'].map((c) => {
                const v = stats.by_class[c] || 0
                const color = { '2xx': 'bg-signal', '3xx': 'bg-sky-400', '4xx': 'bg-warn', '5xx': 'bg-fail' }[c]
                return v ? <div key={c} className={color} style={{ width: `${(v / total) * 100}%` }} title={`${c}: ${v}`} /> : null
              })}
            </div>
            <dl className="mt-3 grid grid-cols-4 gap-2 text-center text-xs">
              {['2xx', '3xx', '4xx', '5xx'].map((c) => (
                <div key={c}>
                  <dt className="text-ink-400">{c}</dt>
                  <dd className="font-mono" data-testid={`stat-${c}`}>
                    {stats.by_class[c] || 0}
                  </dd>
                </div>
              ))}
            </dl>
          </div>
          <TopList title="많이 요청된 경로" rows={stats.top_paths} />
          <TopList title="많이 접속한 IP" rows={stats.top_ips} />
        </div>
      )}
      <ErrorBox error={logs.error} />
      <LogViewer lines={lines} height="h-[50vh]" emptyText={logs.loading ? '불러오는 중…' : '로그가 없습니다 (도메인으로 아직 요청이 없음)'} />
    </div>
  )
}

function TopList({ title, rows }) {
  const max = rows[0]?.[1] || 1
  return (
    <div className="card p-5">
      <p className="mb-3 text-xs text-ink-400">{title}</p>
      <ul className="space-y-1.5">
        {rows.slice(0, 6).map(([k, v]) => (
          <li key={k} className="relative text-xs">
            <div className="absolute inset-y-0 left-0 rounded bg-ink-800" style={{ width: `${(v / max) * 100}%` }} />
            <div className="relative flex justify-between gap-2 px-1.5 py-0.5">
              <span className="truncate font-mono">{k}</span>
              <span className="font-mono text-ink-300">{v}</span>
            </div>
          </li>
        ))}
      </ul>
    </div>
  )
}
