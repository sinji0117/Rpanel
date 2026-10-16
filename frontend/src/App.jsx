import { BrowserRouter, Link, Navigate, Route, Routes } from 'react-router'
import { DialogHost } from './components/Dialog'
import Layout from './components/Layout'
import { Spinner } from './components/ui'
import { AuthProvider, useAuth } from './context/AuthContext'
import { ToastProvider } from './context/ToastContext'
import AppDetail from './pages/AppDetail'
import { LoginPage, SetupPage } from './pages/AuthPages'
import Dashboard from './pages/Dashboard'
import DeploymentView from './pages/DeploymentView'
import ProjectDetail from './pages/ProjectDetail'
import Projects from './pages/Projects'
import Settings from './pages/Settings'
import Sites from './pages/Sites'
import AdminsPage from './pages/Admins'
import RuntimesPage from './pages/Runtimes'
import LogsPage from './pages/Logs'
import NotificationsPage from './pages/Notifications'
import MongoPage from './pages/MongoPage'
import { DbWebAdminPage, DbWebStandalone } from './pages/DbWeb'
import { BackupsPage, MariaDBPage, RedisPage } from './pages/DataPages'
import { t, useLang } from './i18n'

function NotFound() {
  return (
    <div className="py-20 text-center">
      <p className="text-ink-400">{t('페이지를 찾을 수 없습니다')}</p>
      <Link to="/" className="mt-4 inline-block text-signal">
        {t('대시보드로')}
      </Link>
    </div>
  )
}

function Gate() {
  const { status } = useAuth()
  const lang = useLang() // 언어를 바꾸면 화면 전체를 새 언어로 다시 그림
  if (status === 'loading') {
    return (
      <div className="flex min-h-screen items-center justify-center text-ink-400">
        <Spinner />
      </div>
    )
  }
  if (status === 'setup') return <SetupPage key={lang} />
  if (status === 'anon') return <LoginPage key={lang} />
  return (
    <Routes key={lang}>
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="sites" element={<Sites />} />
        <Route path="mariadb" element={<MariaDBPage />} />
        <Route path="mariadb/:id/web" element={<DbWebAdminPage />} />
        <Route path="redis" element={<RedisPage />} />
        <Route path="mongodb" element={<MongoPage />} />
        <Route path="db-versions" element={<Navigate to="/runtimes?tab=db" replace />} />
        <Route path="databases" element={<Navigate to="/mariadb" replace />} />
        <Route path="admins" element={<AdminsPage />} />
        <Route path="runtimes" element={<RuntimesPage />} />
        <Route path="logs" element={<LogsPage />} />
        <Route path="notifications" element={<NotificationsPage />} />
        <Route path="backups" element={<BackupsPage />} />
        <Route path="projects" element={<Projects />} />
        <Route path="projects/:id" element={<ProjectDetail />} />
        <Route path="apps/:id" element={<AppDetail />} />
        <Route path="deployments/:id" element={<DeploymentView />} />
        <Route path="settings" element={<Settings />} />
        <Route path="*" element={<NotFound />} />
      </Route>
    </Routes>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <ToastProvider>
        <Routes>
          {/* DB 계정으로만 들어가는 화면 — 패널 로그인과 별개 */}
          <Route path="/db/*" element={<DbWebStandalone />} />
          <Route
            path="*"
            element={
              <AuthProvider>
                <Gate />
              </AuthProvider>
            }
          />
        </Routes>
        <DialogHost />
      </ToastProvider>
    </BrowserRouter>
  )
}
