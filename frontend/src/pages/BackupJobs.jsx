import { useState } from 'react'
import { ErrorBox, Field, Modal, Section, Spinner, StatusBadge } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { api, get, post, put } from '../lib/api'
import { ago, dateTime } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { t } from '../i18n'

export const WEEKDAYS = ['일요일', '월요일', '화요일', '수요일', '목요일', '금요일', '토요일']
const pad = (n) => String(n).padStart(2, '0')

/** 예약 주기를 사람이 읽는 문장으로: 매일 03:30 / 매주 일요일 04:00 / 매시간 05분 */
export function describeSchedule(j) {
  if (j.schedule === 'hourly') return t('매시간 {mm}분', { mm: pad(j.minute) })
  if (j.schedule === 'weekly') return t('매주 {day} {hh}:{mm}', { day: WEEKDAYS[j.weekday] ? t(WEEKDAYS[j.weekday]) : '?', hh: pad(j.hour), mm: pad(j.minute) })
  return t('매일 {hh}:{mm}', { hh: pad(j.hour), mm: pad(j.minute) })
}

const TARGET_LABEL = { app: '앱', database: 'DB', all: '전체' }

const emptyJob = (appId) => ({
  name: '',
  target: appId ? 'app' : 'all',
  app_id: appId ?? '',
  database_id: '',
  schedule: 'daily',
  hour: 3,
  minute: 0,
  weekday: 0,
  keep_count: 7,
  enabled: true,
})

/** 백업 예약 추가·수정 창 */
export function BackupJobForm({ initial, fixedAppId, onSaved, onClose }) {
  const editing = !!initial?.id
  const [v, setV] = useState(() => ({ ...emptyJob(fixedAppId), ...(initial || {}) }))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  // 앱·DB 목록 (대상 고르기)
  const opts = useFetch(async () => {
    const [apps, dbs] = await Promise.all([get('/apps'), get('/databases')])
    return { apps, databases: dbs.databases }
  }, [])
  const set = (k) => (e) => setV((s) => ({ ...s, [k]: e.target.type === 'checkbox' ? e.target.checked : e.target.value }))

  const submit = async (e) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const body = {
        name: v.name,
        target: v.target,
        app_id: v.target === 'app' ? Number(v.app_id) || null : null,
        database_id: v.target === 'database' ? Number(v.database_id) || null : null,
        schedule: v.schedule,
        hour: Number(v.hour),
        minute: Number(v.minute),
        weekday: Number(v.weekday),
        keep_count: Number(v.keep_count),
        enabled: !!v.enabled,
      }
      const r = editing ? await put(`/backup-jobs/${initial.id}`, body) : await post('/backup-jobs', body)
      onSaved(r)
    } catch (err) {
      setError(err)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal open onClose={onClose} title={editing ? t('백업 예약 수정') : t('백업 예약 추가')} wide>
      <form className="space-y-4" onSubmit={submit}>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('백업할 대상')} htmlFor="bj-target">
            <select id="bj-target" className="input" value={v.target} onChange={set('target')} disabled={!!fixedAppId && !editing}>
              <option value="app">{t('앱 하나 (연결된 DB + 파일)')}</option>
              <option value="database">{t('데이터베이스 하나')}</option>
              <option value="all">{t('전체 (모든 앱과 데이터베이스)')}</option>
            </select>
          </Field>
          {v.target === 'app' && (
            <Field label={t('앱')} htmlFor="bj-app">
              <select id="bj-app" className="input" required value={v.app_id ?? ''} onChange={set('app_id')}>
                <option value="">{t('고르세요')}</option>
                {opts.data?.apps.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            </Field>
          )}
          {v.target === 'database' && (
            <Field label={t('데이터베이스')} htmlFor="bj-db">
              <select id="bj-db" className="input" required value={v.database_id ?? ''} onChange={set('database_id')}>
                <option value="">{t('고르세요')}</option>
                {opts.data?.databases.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.name}
                  </option>
                ))}
              </select>
            </Field>
          )}
        </div>

        <div className="grid gap-4 sm:grid-cols-4">
          <Field label={t('주기')} htmlFor="bj-schedule">
            <select id="bj-schedule" className="input" value={v.schedule} onChange={set('schedule')}>
              <option value="hourly">{t('매시간')}</option>
              <option value="daily">{t('매일')}</option>
              <option value="weekly">{t('매주')}</option>
            </select>
          </Field>
          {v.schedule === 'weekly' && (
            <Field label={t('요일')} htmlFor="bj-weekday">
              <select id="bj-weekday" className="input" value={v.weekday} onChange={set('weekday')}>
                {WEEKDAYS.map((d, i) => (
                  <option key={d} value={i}>
                    {t(d)}
                  </option>
                ))}
              </select>
            </Field>
          )}
          {v.schedule !== 'hourly' && (
            <Field label={t('시')} htmlFor="bj-hour">
              <select id="bj-hour" className="input" value={v.hour} onChange={set('hour')}>
                {Array.from({ length: 24 }, (_, h) => (
                  <option key={h} value={h}>
                    {t('{pad}시', { pad: pad(h) })}
                  </option>
                ))}
              </select>
            </Field>
          )}
          <Field label={t('분')} htmlFor="bj-minute">
            <select id="bj-minute" className="input" value={v.minute} onChange={set('minute')}>
              {Array.from({ length: 12 }, (_, i) => i * 5).map((m) => (
                <option key={m} value={m}>
                  {t('{pad}분', { pad: pad(m) })}
                </option>
              ))}
            </select>
          </Field>
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('보관 개수')} htmlFor="bj-keep" hint={t('대상(DB·파일)별로 최근 몇 개를 남길지. 오래된 것부터 자동 삭제')}>
            <input id="bj-keep" type="number" min="1" max="365" className="input" required value={v.keep_count} onChange={set('keep_count')} />
          </Field>
          <Field label={t('이름 (선택)')} htmlFor="bj-name" hint={t('비우면 대상 이름으로 정합니다')}>
            <input id="bj-name" className="input" maxLength={100} value={v.name} onChange={set('name')} placeholder={t('예: 쇼핑몰 매일 백업')} />
          </Field>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={!!v.enabled} onChange={set('enabled')} /> {t('예약 켜기')}
        </label>

        <p className="text-xs text-ink-400">
          {t('미리보기: {describeSchedule} · 최근 {keep_count}개 보관 (서버 시간 기준)', {
            describeSchedule: describeSchedule({ ...v, hour: Number(v.hour), minute: Number(v.minute), weekday: Number(v.weekday) }),
            keep_count: v.keep_count,
          })}
        </p>
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

/** 백업 예약 목록 (appId 를 주면 그 앱이 대상인 예약 + 전체 예약만) */
export function BackupJobs({ appId, onRan }) {
  const toast = useToast()
  const list = useFetch(() => get(appId ? `/backup-jobs?app_id=${appId}` : '/backup-jobs'), [appId])
  const [editing, setEditing] = useState(null)
  const [busy, setBusy] = useState(false)
  const jobs = list.data?.jobs || []

  const act = async (fn, msg) => {
    setBusy(true)
    try {
      await fn()
      if (msg) toast.push(msg, 'success')
      list.reload(true)
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setBusy(false)
    }
  }

  const toBody = (j, patch) => ({
    name: j.name,
    target: j.target,
    app_id: j.app_id,
    database_id: j.database_id,
    schedule: j.schedule,
    hour: j.hour,
    minute: j.minute,
    weekday: j.weekday,
    keep_count: j.keep_count,
    enabled: j.enabled,
    ...patch,
  })

  return (
    <Section
      title={t('백업 예약')}
      desc={t('정해진 시간마다 자동으로 백업합니다. 예약을 여러 개 만들 수 있고(예: 매일 DB, 매주 전체), 예약마다 보관 개수가 따로 적용됩니다.')}
      actions={
        <button className="btn-primary" onClick={() => setEditing({})}>
          {t('+ 예약 추가')}
        </button>
      }
    >
      {list.loading ? (
        <Spinner className="text-ink-400" />
      ) : jobs.length === 0 ? (
        <p className="text-sm text-ink-400">{t('백업 예약이 없습니다. “+ 예약 추가”로 자동 백업을 켜세요.')}</p>
      ) : (
        <ul className="divide-y divide-ink-800" aria-label={t('백업 예약 목록')}>
          {jobs.map((j) => (
            <li key={j.id} className="flex flex-wrap items-center gap-x-4 gap-y-2 py-3 text-sm">
              <div className="min-w-0 flex-1">
                <p className="font-medium">
                  {j.name}
                  {!j.enabled && <span className="ml-2 rounded border border-ink-600 px-1.5 text-[11px] text-ink-400">{t('꺼짐')}</span>}
                </p>
                <p className="mt-0.5 text-xs text-ink-400">
                  <span className="kbd mr-1.5">{t(TARGET_LABEL[j.target])}</span>
                  {t('{target_name} · {describeSchedule} · {keep_count}개 보관', { target_name: j.target_name, describeSchedule: describeSchedule(j), keep_count: j.keep_count })}
                </p>
                <p className="mt-0.5 text-xs text-ink-400">
                  {j.next_run_at ? <>{t('다음 {dateTime}', { dateTime: dateTime(j.next_run_at) })}</> : t('예약 꺼짐')}
                  {j.last_run_at && <> {t('· 마지막 {ago}', { ago: ago(j.last_run_at) })}</>}
                  {j.backup_count > 0 && <> {t('· 백업 {backup_count}개', { backup_count: j.backup_count })}</>}
                </p>
              </div>
              {j.last_status && <StatusBadge status={j.last_status} deployment />}
              <div className="flex flex-wrap gap-1.5">
                <button
                  className="btn-ghost px-2.5 py-1 text-xs"
                  disabled={busy}
                  onClick={() =>
                    act(async () => {
                      const r = await post(`/backup-jobs/${j.id}/run`)
                      onRan?.()
                      toast.push(t('백업 {started}개를 시작했습니다', { started: r.started }))
                    })
                  }
                >
                  {t('지금 실행')}
                </button>
                <button
                  className="btn-ghost px-2.5 py-1 text-xs"
                  disabled={busy}
                  onClick={() => act(() => put(`/backup-jobs/${j.id}`, toBody(j, { enabled: !j.enabled })), j.enabled ? t('예약을 껐습니다') : t('예약을 켰습니다'))}
                >
                  {j.enabled ? t('끄기') : t('켜기')}
                </button>
                <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => setEditing(j)}>
                  {t('수정')}
                </button>
                <button
                  className="btn-danger px-2.5 py-1 text-xs"
                  disabled={busy}
                  onClick={() => {
                    if (!window.confirm(t('"{name}" 예약을 삭제할까요?', { name: j.name }))) return
                    const files =
                      j.backup_count > 0 &&
                      window.confirm(t('이 예약이 만든 백업 파일 {backup_count}개도 함께 지울까요?\n(취소를 누르면 파일은 남겨 둡니다)', { backup_count: j.backup_count }))
                    act(() => api(`/backup-jobs/${j.id}`, { method: 'DELETE', body: { delete_files: files } }), t('예약을 삭제했습니다'))
                  }}
                >
                  {t('삭제')}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {editing && (
        <BackupJobForm
          initial={editing.id ? editing : null}
          fixedAppId={appId}
          onClose={() => setEditing(null)}
          onSaved={() => {
            toast.push(editing.id ? t('예약을 저장했습니다') : t('예약을 추가했습니다'), 'success')
            setEditing(null)
            list.reload(true)
          }}
        />
      )}
    </Section>
  )
}
