import { Fragment, useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { LEVEL, notificationsChanged, notificationTitle } from '../components/NotificationBell'
import { ErrorBox, Field, PageHeader, Section, Spinner, Tabs } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { t } from '../i18n'
import { del, get, post, put } from '../lib/api'
import { dateTime } from '../lib/format'
import { useFetch } from '../lib/useFetch'
import { confirmDialog } from '../components/Dialog'

// 알림 분류 (kind 기준)
export const CATEGORIES = [
  { id: 'all', label: '전체' },
  { id: 'deploy', label: '배포', kinds: ['deploy_failed', 'deploy_success'] },
  { id: 'backup', label: '백업', kinds: ['backup_failed', 'backup_success'] },
  { id: 'server', label: '사이트·서버', kinds: ['site_down', 'disk_high', 'mem_high'] },
  { id: 'system', label: '시스템', kinds: ['update_available', 'runtime_install', 'db_engine', 'admin_login', 'test'] },
]
const LEVELS = [
  { id: 'all', label: '모든 수준' },
  { id: 'error', label: '오류' },
  { id: 'warning', label: '경고' },
  { id: 'success', label: '성공' },
  { id: 'info', label: '정보' },
]

export function categoryOf(n) {
  return CATEGORIES.find((c) => c.kinds?.includes(n.kind))?.id ?? 'system'
}

/** 오늘 · 어제 · 날짜 */
function dayLabel(iso) {
  const d = new Date(iso)
  const today = new Date()
  const y = new Date()
  y.setDate(today.getDate() - 1)
  const same = (a, b) => a.toDateString() === b.toDateString()
  if (same(d, today)) return t('오늘')
  if (same(d, y)) return t('어제')
  return d.toLocaleDateString()
}

function NotificationList() {
  const toast = useToast()
  const nav = useNavigate()
  const [unreadOnly, setUnreadOnly] = useState(false)
  const [cat, setCat] = useState('all')
  const [level, setLevel] = useState('all')
  const list = useFetch(() => get(`/notifications?limit=300${unreadOnly ? '&unread=true' : ''}`), [unreadOnly])
  const run = async (fn, msg) => {
    try {
      await fn()
      if (msg) toast.push(msg, 'success')
      list.reload(true)
      notificationsChanged()
    } catch (e) {
      toast.push(e.message, 'error')
    }
  }
  const shown = (list.data?.items ?? []).filter((n) => (cat === 'all' || categoryOf(n) === cat) && (level === 'all' || n.level === level))
  return (
    <section className="card">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink-700/70 px-5 py-3">
        <label className="flex items-center gap-2 text-sm text-ink-300">
          <input type="checkbox" checked={unreadOnly} onChange={(e) => setUnreadOnly(e.target.checked)} /> {t('안 읽은 것만')}
        </label>
        <div className="flex items-center gap-2">
          <button className="btn-ghost px-2.5 py-1 text-xs" disabled={!list.data?.unread} onClick={() => run(() => post('/notifications/read', { all: true }))}>
            {t('모두 읽음')}
          </button>
          <button
            className="btn-ghost px-2.5 py-1 text-xs text-red-300"
            disabled={!list.data?.items.length}
            onClick={async () => (await confirmDialog(t('알림을 모두 지울까요?'))) && run(() => del('/notifications'), t('알림을 지웠습니다'))}
          >
            {t('모두 지우기')}
          </button>
        </div>
      </div>
      <ErrorBox error={list.error} />
      {list.loading && <Spinner className="m-5 text-ink-400" />}
      <div className="flex flex-wrap items-center gap-2 border-b border-ink-700/70 px-5 py-2.5">
        <div className="flex flex-wrap gap-1" role="tablist" aria-label={t('알림 분류')}>
          {CATEGORIES.map((c) => {
            const n = (list.data?.items ?? []).filter((x) => c.id === 'all' || categoryOf(x) === c.id)
            const unread = n.filter((x) => !x.read_at).length
            return (
              <button
                key={c.id}
                role="tab"
                aria-selected={cat === c.id}
                onClick={() => setCat(c.id)}
                className={`rounded-full px-3 py-1 text-xs transition ${cat === c.id ? 'bg-ink-800 font-medium text-ink-100 ring-1 ring-ink-600' : 'text-ink-400 hover:bg-ink-850 hover:text-ink-100'}`}
              >
                {t(c.label)} <span className="text-ink-500">{n.length}</span>
                {unread > 0 && <span className="ml-1 rounded-full bg-signal/20 px-1.5 text-[10px] text-signal">{unread}</span>}
              </button>
            )
          })}
        </div>
        <select className="input ml-auto w-auto py-1 text-xs" aria-label={t('알림 수준')} value={level} onChange={(e) => setLevel(e.target.value)}>
          {LEVELS.map((l) => (
            <option key={l.id} value={l.id}>
              {t(l.label)}
            </option>
          ))}
        </select>
      </div>
      <ul className="divide-y divide-ink-800" aria-label={t('알림 목록')}>
        {shown.map((n, i) => (
          <Fragment key={n.id}>
            {(i === 0 || dayLabel(shown[i - 1].created_at) !== dayLabel(n.created_at)) && (
              <li className="bg-ink-900/60 px-5 py-1.5 text-[11px] font-medium text-ink-400">{dayLabel(n.created_at)}</li>
            )}
            <li key={n.id} className={`flex gap-3 px-5 py-3 ${n.read_at ? 'opacity-60' : ''}`}>
              <span className={`mt-1.5 h-2 w-2 shrink-0 rounded-full ${LEVEL[n.level]?.dot ?? 'bg-ink-400'}`} />
              <div className="min-w-0 flex-1">
                <p className="text-sm">
                  <span className="mr-2 rounded bg-ink-800 px-1.5 py-0.5 text-[10px] text-ink-400">{t(CATEGORIES.find((c) => c.id === categoryOf(n))?.label)}</span>
                  {notificationTitle(n)}
                </p>
                {n.body && <pre className="mt-1 font-sans text-xs whitespace-pre-wrap text-ink-400">{n.body}</pre>}
                <p className="mt-1 text-[11px] text-ink-600">{dateTime(n.created_at)}</p>
              </div>
              <div className="flex shrink-0 items-start gap-1.5">
                {n.link && (
                  <button
                    className="btn-ghost px-2.5 py-1 text-xs"
                    onClick={() => {
                      if (!n.read_at) post('/notifications/read', { ids: [n.id] }).catch(() => {})
                      nav(n.link)
                    }}
                  >
                    {t('보기')}
                  </button>
                )}
                {!n.read_at && (
                  <button className="btn-ghost px-2.5 py-1 text-xs" onClick={() => run(() => post('/notifications/read', { ids: [n.id] }))}>
                    {t('읽음')}
                  </button>
                )}
              </div>
            </li>
          </Fragment>
        ))}
        {list.data && !shown.length && <li className="px-5 py-10 text-center text-sm text-ink-400">{t('알림이 없습니다')}</li>}
      </ul>
    </section>
  )
}

const CHANNELS = [
  { key: 'discord_url', label: 'Discord 웹훅 주소', placeholder: 'https://discord.com/api/webhooks/…', help: 'Discord 채널 설정 → 연동 → 웹후크 → 새 웹후크 → 주소 복사' },
  { key: 'slack_url', label: 'Slack 웹훅 주소', placeholder: 'https://hooks.slack.com/services/…', help: 'Slack 앱 “Incoming Webhooks” 에서 만든 주소' },
  { key: 'telegram_token', label: 'Telegram 봇 토큰', placeholder: '123456789:AA…', help: '@BotFather 에서 /newbot 으로 봇을 만들면 나오는 토큰' },
  { key: 'telegram_chat_id', label: 'Telegram chat id', placeholder: '-1001234567890', help: '봇을 대화방에 초대한 뒤 그 방의 id (숫자) 또는 @채널이름' },
  { key: 'webhook_url', label: '웹훅 주소 (JSON POST)', placeholder: 'https://example.com/hook', help: '알림마다 kind·level·title·body·link 를 JSON 으로 보냅니다' },
]

function NotificationSettings() {
  const toast = useToast()
  const data = useFetch(() => get('/notifications/settings'), [])
  const [v, setV] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [results, setResults] = useState(null)
  useEffect(() => {
    if (data.data) setV(data.data.settings)
  }, [data.data])
  if (!v) return data.error ? <ErrorBox error={data.error} /> : <Spinner className="text-ink-400" />
  const events = data.data.events
  const setEvent = (k, on) => setV({ ...v, events: { ...v.events, [k]: on } })
  const setChannel = (k, val) => setV({ ...v, channels: { ...v.channels, [k]: val } })

  const save = async () => {
    setBusy(true)
    setError(null)
    try {
      const r = await put('/notifications/settings', v)
      setV(r.settings)
      toast.push(t('알림 설정을 저장했습니다'), 'success')
      return true
    } catch (e) {
      setError(e)
      return false
    } finally {
      setBusy(false)
    }
  }

  return (
    <form
      className="space-y-4"
      onSubmit={(e) => {
        e.preventDefault()
        save()
      }}
    >
      <Section
        title={t('알림 받을 일')}
        desc={t('켠 일만 알림 목록에 남고, 아래 채널로도 보냅니다. 사이트 응답 확인은 5분마다, 디스크·메모리는 1분마다 확인합니다 (같은 경고는 몇 시간에 한 번만).')}
      >
        <div className="grid gap-2 sm:grid-cols-2">
          {events.map((e) => (
            <label key={e.key} className="flex items-center gap-2.5 rounded-lg border border-ink-700 px-3 py-2 text-sm hover:border-ink-600">
              <input type="checkbox" checked={!!v.events[e.key]} onChange={(ev) => setEvent(e.key, ev.target.checked)} />
              {t(e.label)}
            </label>
          ))}
        </div>
        <div className="mt-4 grid gap-3 sm:grid-cols-2">
          <Field label={t('디스크 경고 기준 (%)')} htmlFor="n-disk">
            <input
              id="n-disk"
              type="number"
              min={50}
              max={99}
              className="input w-28"
              value={v.disk_threshold}
              onChange={(e) => setV({ ...v, disk_threshold: Number(e.target.value) })}
            />
          </Field>
          <Field label={t('메모리 경고 기준 (%)')} htmlFor="n-mem">
            <input
              id="n-mem"
              type="number"
              min={50}
              max={99}
              className="input w-28"
              value={v.mem_threshold}
              onChange={(e) => setV({ ...v, mem_threshold: Number(e.target.value) })}
            />
          </Field>
        </div>
      </Section>

      <Section
        title={t('보낼 곳 (선택)')}
        desc={t('비워 두면 패널 안 알림(종 아이콘)으로만 받습니다. 여러 곳을 함께 쓸 수 있습니다. 서버에서 그 주소로 인터넷 연결이 되어야 합니다.')}
        actions={
          <button
            type="button"
            className="btn-ghost"
            disabled={busy}
            onClick={async () => {
              if (!(await save())) return
              try {
                const r = await post('/notifications/test', {})
                setResults(r.results)
                notificationsChanged()
                toast.push(r.results.length ? t('시험 알림을 보냈습니다') : t('보낼 곳이 없어 패널 안 알림으로만 남겼습니다'), 'success')
              } catch (e) {
                toast.push(e.message, 'error')
              }
            }}
          >
            {t('시험 알림 보내기')}
          </button>
        }
      >
        <div className="grid gap-4 lg:grid-cols-2">
          {CHANNELS.map((c) => (
            <Field key={c.key} label={t(c.label)} htmlFor={`n-${c.key}`} hint={t(c.help)}>
              <input
                id={`n-${c.key}`}
                className="input font-mono text-xs"
                autoComplete="off"
                spellCheck={false}
                placeholder={c.placeholder}
                value={v.channels[c.key] ?? ''}
                onChange={(e) => setChannel(c.key, e.target.value)}
              />
            </Field>
          ))}
        </div>
        {results && results.length > 0 && (
          <ul className="mt-4 space-y-1 text-sm" aria-label={t('시험 결과')}>
            {results.map((r) => (
              <li key={r.channel} className={r.ok ? 'text-signal' : 'text-red-300'}>
                {r.channel}: {r.ok ? t('성공') : t('실패 — {error}', { error: r.error })}
              </li>
            ))}
          </ul>
        )}
      </Section>
      <ErrorBox error={error} />
      <div className="flex justify-end">
        <button className="btn-primary" disabled={busy}>
          {busy ? t('저장 중…') : t('저장')}
        </button>
      </div>
    </form>
  )
}

export default function NotificationsPage() {
  const [params, setParams] = useSearchParams()
  const tab = params.get('tab') === 'settings' ? 'settings' : 'list'
  return (
    <>
      <PageHeader
        title={t('알림')}
        help={t('배포 실패, 백업 실패, 사이트 응답 없음, 디스크·메모리 부족, 새 버전 같은 일을 알려 줍니다. 왼쪽 위 종 아이콘에서 바로 볼 수 있습니다.')}
      />
      <Tabs
        tabs={[
          { id: 'list', label: t('알림 목록') },
          { id: 'settings', label: t('알림 설정') },
        ]}
        active={tab}
        onChange={(id) => setParams(id === 'list' ? {} : { tab: id }, { replace: true })}
      />
      {tab === 'list' ? <NotificationList /> : <NotificationSettings />}
    </>
  )
}
