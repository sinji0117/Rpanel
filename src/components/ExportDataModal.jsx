import { useState } from 'react'
import { Modal } from './ui'
import { get } from '../lib/api'
import { t } from '../i18n'

export function triggerDownload(content, filename, type) {
  const blob = new Blob([content], { type })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

function escapeCsv(val) {
  if (val === undefined || val === null) return '""'
  const str = String(val)
  if (str.includes('"') || str.includes(',') || str.includes('\n')) {
    return `"${str.replace(/"/g, '""')}"`
  }
  return `"${str}"`
}

export default function ExportDataModal({ open, onClose, latest, host }) {
  const [scope, setScope] = useState('all') // 'all' | 'resources' | 'logs'
  const [format, setFormat] = useState('json') // 'json' | 'csv'
  const [exporting, setExporting] = useState(false)
  const [done, setDone] = useState(false)

  const handleExport = async (overrideFormat) => {
    const selectedFormat = overrideFormat || format
    setExporting(true)
    setDone(false)
    try {
      // Fetch latest system events
      let events = []
      if (scope !== 'resources') {
        try {
          const res = await get('/system/events?limit=300')
          events = res?.events ?? []
        } catch {
          events = []
        }
      }

      const dateStr = new Date().toISOString().slice(0, 10)
      const filename = `rpanel-export-${dateStr}.${selectedFormat}`

      if (selectedFormat === 'json') {
        const payload = {
          exported_at: new Date().toISOString(),
          generator: 'Rpanel Dashboard',
          host: host || null,
          resources:
            scope !== 'logs'
              ? {
                  cpu: {
                    percent: latest?.cpu_percent ?? 0,
                    cores: latest?.cpu_per_core ?? [],
                    count: host?.cpu_count || 8,
                    load_avg: latest?.load ?? [],
                  },
                  memory: {
                    total_bytes: latest?.mem_total ?? 16 * 1024 ** 3,
                    used_bytes: latest?.mem_used ?? 5.4 * 1024 ** 3,
                    free_bytes: Math.max(0, (latest?.mem_total ?? 16 * 1024 ** 3) - (latest?.mem_used ?? 5.4 * 1024 ** 3)),
                    percent: latest?.mem_total ? Number(((latest.mem_used / latest.mem_total) * 100).toFixed(1)) : 0,
                  },
                  disks: latest?.disks ?? [{ mount: '/', total: 200 * 1024 ** 3, used: 38 * 1024 ** 3 }],
                }
              : undefined,
          events: scope !== 'resources' ? events : undefined,
        }
        triggerDownload(JSON.stringify(payload, null, 2), filename, 'application/json;charset=utf-8;')
      } else {
        // CSV Format
        let csvRows = []

        if (scope === 'resources' || scope === 'all') {
          if (scope === 'all') csvRows.push('# RESOURCE USAGE')
          csvRows.push(['Category', 'Metric', 'Value', 'Unit'].map(escapeCsv).join(','))
          csvRows.push(['Host', 'Hostname', host?.hostname || 'app-server', ''].map(escapeCsv).join(','))
          csvRows.push(['Host', 'OS', host?.os || '', ''].map(escapeCsv).join(','))
          csvRows.push(['CPU', 'Overall Usage', latest?.cpu_percent ?? 0, '%'].map(escapeCsv).join(','))
          csvRows.push(['CPU', 'Core Count', host?.cpu_count || 8, 'cores'].map(escapeCsv).join(','))
          if (latest?.load) {
            csvRows.push(['CPU', 'Load 1m', latest.load[0] ?? 0, ''].map(escapeCsv).join(','))
            csvRows.push(['CPU', 'Load 5m', latest.load[1] ?? 0, ''].map(escapeCsv).join(','))
            csvRows.push(['CPU', 'Load 15m', latest.load[2] ?? 0, ''].map(escapeCsv).join(','))
          }
          const memTotal = latest?.mem_total ?? 16 * 1024 ** 3
          const memUsed = latest?.mem_used ?? 5.4 * 1024 ** 3
          csvRows.push(['Memory', 'Total', memTotal, 'bytes'].map(escapeCsv).join(','))
          csvRows.push(['Memory', 'Used', memUsed, 'bytes'].map(escapeCsv).join(','))
          csvRows.push(['Memory', 'Free', Math.max(0, memTotal - memUsed), 'bytes'].map(escapeCsv).join(','))

          const disks = latest?.disks ?? [{ mount: '/', total: 200 * 1024 ** 3, used: 38 * 1024 ** 3 }]
          disks.forEach((d) => {
            csvRows.push(['Disk', `Mount ${d.mount} Total`, d.total, 'bytes'].map(escapeCsv).join(','))
            csvRows.push(['Disk', `Mount ${d.mount} Used`, d.used, 'bytes'].map(escapeCsv).join(','))
          })
          if (scope === 'all') csvRows.push('')
        }

        if (scope === 'logs' || scope === 'all') {
          if (scope === 'all') csvRows.push('# SYSTEM LOGS AND EVENTS')
          csvRows.push(['Timestamp', 'Level', 'Source', 'Message'].map(escapeCsv).join(','))
          events.forEach((e) => {
            csvRows.push([e.timestamp, e.level.toUpperCase(), e.source, e.message].map(escapeCsv).join(','))
          })
        }

        triggerDownload(csvRows.join('\r\n'), filename, 'text/csv;charset=utf-8;')
      }

      setDone(true)
      setTimeout(() => {
        setDone(false)
        onClose?.()
      }, 1200)
    } catch {
      /* ignore */
    } finally {
      setExporting(false)
    }
  }

  return (
    <Modal open={open} title={t('데이터 내보내기')} onClose={onClose}>
      <div className="space-y-5 py-1">
        <p className="text-xs text-ink-300">
          {t('현재 자원 사용량과 시스템 로그를 JSON 또는 CSV 파일로 다운로드합니다.')}
        </p>

        {/* Scope Selector */}
        <div>
          <label className="label mb-2 block">{t('내보낼 항목')}</label>
          <div className="grid gap-2 sm:grid-cols-3">
            <button
              type="button"
              onClick={() => setScope('all')}
              className={`rounded-lg border p-3 text-left transition cursor-pointer ${
                scope === 'all'
                  ? 'border-signal bg-signal/10 text-ink-100 font-medium'
                  : 'border-ink-700 bg-ink-900/60 text-ink-300 hover:border-ink-600'
              }`}
            >
              <div className="text-sm font-semibold">{t('자원 현황 및 시스템 로그 전체')}</div>
              <div className="mt-1 text-[11px] text-ink-400">CPU/RAM/디스크 + 이벤트 로그</div>
            </button>

            <button
              type="button"
              onClick={() => setScope('resources')}
              className={`rounded-lg border p-3 text-left transition cursor-pointer ${
                scope === 'resources'
                  ? 'border-signal bg-signal/10 text-ink-100 font-medium'
                  : 'border-ink-700 bg-ink-900/60 text-ink-300 hover:border-ink-600'
              }`}
            >
              <div className="text-sm font-semibold">{t('자원 현황만')}</div>
              <div className="mt-1 text-[11px] text-ink-400">CPU, 메모리, 디스크, 부하율</div>
            </button>

            <button
              type="button"
              onClick={() => setScope('logs')}
              className={`rounded-lg border p-3 text-left transition cursor-pointer ${
                scope === 'logs'
                  ? 'border-signal bg-signal/10 text-ink-100 font-medium'
                  : 'border-ink-700 bg-ink-900/60 text-ink-300 hover:border-ink-600'
              }`}
            >
              <div className="text-sm font-semibold">{t('시스템 로그만')}</div>
              <div className="mt-1 text-[11px] text-ink-400">서버 이벤트 및 오류 기록</div>
            </button>
          </div>
        </div>

        {/* Format Selector */}
        <div>
          <label className="label mb-2 block">{t('파일 형식')}</label>
          <div className="grid grid-cols-2 gap-3">
            <button
              type="button"
              onClick={() => setFormat('json')}
              className={`flex items-center gap-3 rounded-lg border p-3 transition cursor-pointer ${
                format === 'json'
                  ? 'border-signal bg-signal/10 text-ink-100'
                  : 'border-ink-700 bg-ink-900/60 text-ink-400 hover:border-ink-600'
              }`}
            >
              <span className="font-mono text-lg font-bold text-sky-400">{ }</span>
              <div className="text-left">
                <span className="block text-sm font-semibold text-ink-100">JSON</span>
                <span className="text-[11px] text-ink-400">구조화된 데이터 (.json)</span>
              </div>
            </button>

            <button
              type="button"
              onClick={() => setFormat('csv')}
              className={`flex items-center gap-3 rounded-lg border p-3 transition cursor-pointer ${
                format === 'csv'
                  ? 'border-signal bg-signal/10 text-ink-100'
                  : 'border-ink-700 bg-ink-900/60 text-ink-400 hover:border-ink-600'
              }`}
            >
              <span className="font-mono text-lg font-bold text-emerald-400">CSV</span>
              <div className="text-left">
                <span className="block text-sm font-semibold text-ink-100">CSV</span>
                <span className="text-[11px] text-ink-400">스프레드시트 호환 (.csv)</span>
              </div>
            </button>
          </div>
        </div>

        {/* Actions */}
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-ink-800 pt-4">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => handleExport('json')}
              disabled={exporting}
              className="rounded-lg border border-ink-700 bg-ink-900 px-3 py-1.5 text-xs font-medium text-ink-200 transition hover:border-ink-600 hover:text-ink-100 disabled:opacity-50 cursor-pointer"
            >
              {t('JSON 다운로드')}
            </button>
            <button
              type="button"
              onClick={() => handleExport('csv')}
              disabled={exporting}
              className="rounded-lg border border-ink-700 bg-ink-900 px-3 py-1.5 text-xs font-medium text-ink-200 transition hover:border-ink-600 hover:text-ink-100 disabled:opacity-50 cursor-pointer"
            >
              {t('CSV 다운로드')}
            </button>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              disabled={exporting}
              className="btn-ghost px-3 py-1.5 text-xs cursor-pointer"
            >
              {t('닫기')}
            </button>
            <button
              type="button"
              onClick={() => handleExport()}
              disabled={exporting}
              className="btn-primary px-4 py-1.5 text-xs cursor-pointer"
            >
              {done ? (
                `✓ ${t('다운로드 완료')}`
              ) : exporting ? (
                t('다운로드 준비 중…')
              ) : (
                `📥 ${format.toUpperCase()} ${t('다운로드')}`
              )}
            </button>
          </div>
        </div>
      </div>
    </Modal>
  )
}
