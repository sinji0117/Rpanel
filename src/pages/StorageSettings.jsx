// 설정 → 저장 위치 (백업 · 프로젝트 파일)
import { useState } from 'react'
import { Section } from '../components/ui'
import { confirmDialog } from '../components/Dialog'
import { useAuth } from '../context/AuthContext'
import { useToast } from '../context/ToastContext'
import { get, put } from '../lib/api'
import { bytes } from '../lib/format'
import { useFetch, useInterval } from '../lib/useFetch'
import { t } from '../i18n'

const HINT = {
  backups: '모든 백업 파일(앱 · 데이터베이스 · DB 버전 바꾸기 전 전체 백업)이 저장되는 곳입니다.',
  projects: '앱의 실행 파일 · 데이터(data) · 정적 파일 · 빌드 폴더가 저장되는 곳입니다. 파일 사이트(사이트 계정)는 /home 에 있어 옮기지 않습니다.',
}

function StorageItem({ it, busy, onStarted }) {
  const toast = useToast()
  const [path, setPath] = useState('')
  const [sending, setSending] = useState(false)
  const move = async (target) => {
    const toDefault = target === ''
    const msg = toDefault
      ? t('{what} 을(를) 기본 위치({path})로 되돌립니다.', { what: t(it.label), path: it.default_path })
      : t('{what} 을(를) {path} 로 옮깁니다.', { what: t(it.label), path: target })
    const more =
      ' ' +
      t('데이터를 복사한 뒤 원래 경로에 연결하고, 예전 위치의 데이터는 지웁니다. 새 폴더는 비어 있어야 합니다.') +
      (it.kind === 'projects' ? ' ' + t('마지막 복사 동안 실행 중인 앱이 잠깐 멈춥니다.') : '')
    const ok = await confirmDialog(msg + more, {
      title: t('{what} 저장 위치 바꾸기', { what: t(it.label) }),
      confirmText: t('옮기기'),
      danger: it.kind === 'projects',
    })
    if (!ok) return
    setSending(true)
    try {
      await put(`/system/storage/${it.kind}`, { path: target })
      toast.push(t('옮기는 중입니다 — 데이터 크기에 따라 오래 걸릴 수 있습니다'), 'success')
      setPath('')
      onStarted()
    } catch (e) {
      toast.push(e.message, 'error')
    } finally {
      setSending(false)
    }
  }
  return (
    <div className="space-y-2 rounded-lg border border-ink-700 p-3" data-testid={`storage-${it.kind}`}>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium">{t(it.label)}</span>
        <span className={`rounded-full px-2 py-0.5 text-[11px] ${it.custom ? 'bg-signal/15 text-signal' : 'bg-ink-800 text-ink-400'}`}>
          {it.custom ? t('바꾼 위치') : t('기본 위치')}
        </span>
        {it.free_bytes != null && (
          <span className="ml-auto text-[11px] text-ink-400">{t('남은 공간 {free} / {total}', { free: bytes(it.free_bytes), total: bytes(it.total_bytes) })}</span>
        )}
      </div>
      <p className="font-mono text-xs break-all" data-testid={`storage-path-${it.kind}`}>
        {it.path}
      </p>
      <p className="text-[11px] text-ink-400">{t(HINT[it.kind])}</p>
      <form
        className="flex flex-wrap gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          if (path.trim()) move(path.trim())
        }}
      >
        <input
          className="input flex-1 font-mono text-xs"
          placeholder={it.kind === 'backups' ? '/data/rpanel-backups' : '/data/rpanel'}
          aria-label={t('{what} 새 위치', { what: t(it.label) })}
          value={path}
          onChange={(e) => setPath(e.target.value)}
          disabled={busy || sending}
        />
        <button className="btn-primary px-3 text-xs" disabled={busy || sending || !path.trim()}>
          {t('옮기기')}
        </button>
        {it.custom && (
          <button type="button" className="btn-ghost px-3 text-xs" disabled={busy || sending} onClick={() => move('')}>
            {t('기본 위치로')}
          </button>
        )}
      </form>
    </div>
  )
}

export function StorageSettings() {
  const { user } = useAuth()
  const isAdmin = user?.role === 'admin'
  const d = useFetch(() => (isAdmin ? get('/system/storage') : Promise.resolve(null)), [isAdmin])
  const job = d.data?.job
  const running = job?.status === 'running'
  useInterval(() => d.reload(true), 2000, running)
  if (!isAdmin || !d.data) return null
  const last = job ?? d.data.last
  return (
    <Section
      title={t('저장 위치')}
      desc={t('백업과 프로젝트 파일을 다른 디스크·폴더에 둘 수 있습니다. 데이터를 옮긴 뒤 원래 경로에 연결하므로(bind mount) 앱 설정은 바뀌지 않고, 재부팅해도 유지됩니다.')}
    >
      <div className="space-y-3">
        {d.data.busy && !running && <p className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-xs">{t('{r}. 끝난 뒤 옮길 수 있습니다.', { r: t(d.data.busy) })}</p>}
        {d.data.items.map((it) => (
          <StorageItem key={it.kind} it={it} busy={!!d.data.busy} onStarted={() => d.reload(true)} />
        ))}
        <p className="text-[11px] text-ink-400">{t('/srv, /data, /mnt 처럼 시스템 폴더가 아닌 곳을 쓰세요. FAT·exFAT·NTFS 디스크는 권한을 저장하지 못해 쓸 수 없습니다.')}</p>
        {last?.log && (
          <details open={running} className="rounded-lg border border-ink-800" data-testid="storage-log">
            <summary className="cursor-pointer px-3 py-1.5 text-xs">{running ? t('옮기는 중…') : last.status === 'done' ? t('마지막 작업: 완료') : t('마지막 작업: 실패')}</summary>
            <pre className="max-h-64 overflow-auto px-3 py-2 font-mono text-[11px] whitespace-pre-wrap text-ink-300">{last.log}</pre>
          </details>
        )}
      </div>
    </Section>
  )
}
