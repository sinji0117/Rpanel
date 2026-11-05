import { useState } from 'react'
import { Link } from 'react-router'
import { t } from '../i18n'
import { newSession, TerminalSessions } from './Terminal'

/** 사이트 화면의 터미널 탭: 사이트 계정으로 셸 열기 */
export function SiteTerminalTab({ app }) {
  const [sessions, setSessions] = useState([])
  const [activeKey, setActiveKey] = useState(null)
  const open = () => {
    const s = newSession(app.site_user, { kind: 'site', app_id: app.id })
    setSessions((list) => [...list, s])
    setActiveKey(s.key)
  }
  return (
    <div className="space-y-3">
      <div className="card flex flex-wrap items-center justify-between gap-3 px-5 py-4">
        <div className="min-w-0">
          <p className="text-sm">
            {t('{user} 계정으로 사이트 폴더에서 열립니다 (SSH 로 접속한 것과 같은 권한).', { user: app.site_user })}
          </p>
          <p className="mt-0.5 text-xs text-ink-400">
            <Link to="/terminal" className="text-signal hover:underline">
              {t('터미널 화면에서 여러 개 열기')}
            </Link>
          </p>
        </div>
        <button type="button" className="btn-primary" onClick={open}>
          {t('터미널 열기')}
        </button>
      </div>
      <TerminalSessions sessions={sessions} setSessions={setSessions} activeKey={activeKey} setActiveKey={setActiveKey} height="h-[55vh]" />
    </div>
  )
}
