import { createContext, useContext, useEffect, useState } from 'react'
import { get, logout as apiLogout, onSessionChange, refreshSession } from '../lib/api'

const AuthContext = createContext(null)

export function AuthProvider({ children }) {
  // status: loading | setup | anon | authed
  const [state, setState] = useState({ status: 'loading', user: null })

  useEffect(() => {
    let alive = true
    const off = onSessionChange((user) => {
      if (!alive) return
      setState((s) => (user ? { status: 'authed', user } : s.status === 'setup' ? s : { status: 'anon', user: null }))
    })
    ;(async () => {
      try {
        const { needs_setup } = await get('/auth/status')
        if (needs_setup) {
          if (alive) setState({ status: 'setup', user: null })
          return
        }
      } catch {
        // 상태 조회 실패해도 로그인 화면으로
      }
      const user = await refreshSession()
      if (alive && !user) setState({ status: 'anon', user: null })
    })()
    return () => {
      alive = false
      off()
    }
  }, [])

  const value = {
    ...state,
    logout: async () => {
      await apiLogout()
      setState({ status: 'anon', user: null })
    },
  }
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  return useContext(AuthContext)
}
