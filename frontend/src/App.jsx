import { BrowserRouter, Link, Route, Routes } from 'react-router'
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

function NotFound() {
  return (
    <div className="py-20 text-center">
      <p className="text-ink-400">페이지를 찾을 수 없습니다</p>
      <Link to="/" className="mt-4 inline-block text-signal">
        대시보드로
      </Link>
    </div>
  )
}

function Gate() {
  const { status } = useAuth()
  if (status === 'loading') {
    return (
      <div className="flex min-h-screen items-center justify-center text-ink-400">
        <Spinner />
      </div>
    )
  }
  if (status === 'setup') return <SetupPage />
  if (status === 'anon') return <LoginPage />
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
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
        <AuthProvider>
          <Gate />
        </AuthProvider>
      </ToastProvider>
    </BrowserRouter>
  )
}
