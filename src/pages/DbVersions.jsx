// 데이터베이스 서버 버전 (MariaDB · MongoDB · Redis) — 지금 버전 · 더 높은 버전으로 바꾸기
import { useState } from 'react'
import { ErrorBox, Section, Spinner } from '../components/ui'
import { confirmDialog } from '../components/Dialog'
import { useToast } from '../context/ToastContext'
import { get, post } from '../lib/api'
import { dateTime } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { t } from '../i18n'

const DESC = {
  mariadb: '패널과 앱의 관계형 데이터베이스',
  mongodb: '패널 로그와 프로젝트용 문서 데이터베이스',
  redis: '세션·캐시·작업 큐',
}

function EngineCard({ e, busyAll, onStarted }) {
  const toast = useToast()
  const [busy, setBusy] = useState(false)
  const running = e.job?.status === 'running'
  const log = e.job?.log || e.last?.log
  const last = e.job ?? e.last
  return (
    <Section
      title={e.label}
      desc={t(DESC[e.engine])}
      actions={
        e.installed ? (
          <span className="rounded-full bg-ink-800 px-3 py-1 font-mono text-xs" data-testid={`engine-version-${e.engine}`}>
            {e.version}
          </span>
        ) : (
          <span className="text-xs text-ink-400">{t('설치되지 않음')}</span>
        )
      }
    >
      {e.user_dbs > 0 && (
        <p className="mb-3 rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-xs" data-testid={`engine-locked-${e.engine}`}>
          {t('이 서버에 만든 데이터베이스가 {n}개 있어 버전을 바꿀 수 없습니다. 바꾸려면 데이터베이스를 백업한 뒤 지우세요.', { n: e.user_dbs })}
        </p>
      )}
      <div className="grid gap-2 sm:grid-cols-3" data-testid={`engine-${e.engine}`}>
        {e.versions.map((v) => (
          <div key={v.version} className={`flex flex-col gap-2 rounded-lg border px-3 py-2.5 ${v.current ? 'border-signal/60 bg-signal/5' : 'border-ink-700'}`}>
            <div className="flex items-center gap-2">
              <span className="font-mono text-sm font-medium">{v.version}</span>
              {v.current && <span className="text-[11px] text-signal">{t('사용 중')}</span>}
            </div>
            {v.note && <span className="text-[11px] text-ink-400">{t(v.note)}</span>}
            {!v.current && (
              <button
                className="btn-ghost mt-auto px-2.5 py-1 text-xs"
                disabled={!v.available || running || busy || busyAll}
                title={v.reason ? t(v.reason) : undefined}
                onClick={async () => {
                  const ok = await confirmDialog(
                    v.direction === 'down'
                      ? t('{engine} 를 {from} → {to} 로 내립니다. 먼저 전체 백업을 만들고, 패널 데이터를 옮겨 담아 다시 설치합니다. 몇 분 동안 패널이 멈출 수 있습니다.', {
                          engine: e.label,
                          from: e.series,
                          to: v.version,
                        }) + (e.engine === 'redis' ? ' ' + t('Redis 를 내리면 패널에 다시 로그인해야 합니다.') : '')
                      : t('{engine} 를 {from} → {to} 로 올립니다. 먼저 전체 백업을 만들고, 바꾸는 동안 잠깐 DB 가 멈춥니다.', {
                          engine: e.label,
                          from: e.series,
                          to: v.version,
                        }),
                    { title: t('{engine} 버전 바꾸기', { engine: e.label }), requireText: `${e.engine} ${v.version}`, confirmText: t('바꾸기'), danger: true },
                  )
                  if (!ok) return
                  setBusy(true)
                  try {
                    await post(`/db-engines/${e.engine}`, { version: v.version })
                    toast.push(t('{engine} {v} 로 바꾸는 중입니다 — 몇 분 걸릴 수 있습니다', { engine: e.label, v: v.version }), 'success')
                    onStarted()
                  } catch (err) {
                    toast.push(err.message, 'error')
                  } finally {
                    setBusy(false)
                  }
                }}
              >
                {!v.available ? t('바꿀 수 없음') : v.direction === 'down' ? t('↓ 이 버전으로 내리기') : t('↑ 이 버전으로 올리기')}
              </button>
            )}
          </div>
        ))}
      </div>
      {last && (
        <div className="mt-3 text-xs">
          <p className={last.status === 'failed' ? 'text-red-300' : last.status === 'running' ? 'text-warn' : 'text-ink-400'}>
            {last.status === 'running'
              ? t('{v} 로 바꾸는 중…', { v: last.target })
              : last.status === 'done'
                ? t('{v} 로 바꿨습니다', { v: last.target })
                : t('{v} 로 바꾸지 못했습니다', { v: last.target })}
            {last.finished_at && <span className="ml-2 text-ink-600">{dateTime(last.finished_at)}</span>}
          </p>
          {log && <pre className="mt-2 max-h-56 overflow-auto rounded-lg bg-ink-950 p-3 font-mono text-[11px] whitespace-pre-wrap text-ink-300">{log}</pre>}
        </div>
      )}
    </Section>
  )
}

/** 런타임 화면의 “데이터베이스” 탭 */
export function DbEngineList() {
  const data = useFetch(() => get('/db-engines'), [])
  const engines = data.data?.engines ?? []
  const running = engines.some((e) => e.job?.status === 'running')
  useInterval(() => data.reload(true), 2000, running)
  return (
    <>
      {data.loading && <Spinner className="text-ink-400" />}
      <ErrorBox error={data.error} />
      <div className="space-y-4">
        {engines.map((e) => (
          <EngineCard key={e.engine} e={e} busyAll={running} onStarted={() => data.reload(true)} />
        ))}
      </div>
    </>
  )
}
