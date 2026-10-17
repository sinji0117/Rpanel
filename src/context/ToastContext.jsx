import { createContext, useCallback, useContext, useState } from 'react'

const ToastContext = createContext({ push: () => {} })

export function ToastProvider({ children }) {
  const [items, setItems] = useState([])
  const push = useCallback((message, kind = 'info') => {
    const id = Math.random().toString(36).slice(2)
    setItems((xs) => [...xs, { id, message, kind }])
    setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== id)), kind === 'error' ? 6000 : 3500)
  }, [])
  return (
    <ToastContext.Provider value={{ push }}>
      {children}
      <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-80 flex-col gap-2" aria-live="polite">
        {items.map((t) => (
          <div
            key={t.id}
            role={t.kind === 'error' ? 'alert' : 'status'}
            className={`pointer-events-auto rounded-lg border px-4 py-3 text-sm shadow-xl shadow-black/40 ${
              t.kind === 'error'
                ? 'border-fail/40 bg-[#2a1416] text-red-200'
                : t.kind === 'success'
                  ? 'border-signal/40 bg-[#0f2a20] text-emerald-200'
                  : 'border-ink-700 bg-ink-850 text-ink-100'
            }`}
          >
            {t.message}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  )
}

export function useToast() {
  return useContext(ToastContext)
}
