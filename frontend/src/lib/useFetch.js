import { useCallback, useEffect, useRef, useState } from 'react'

/** 간단한 데이터 로딩 훅. reload()로 다시 불러옵니다. */
export function useFetch(fn, deps = []) {
  const [data, setData] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(true)
  const fnRef = useRef(fn)
  fnRef.current = fn

  const load = useCallback(async (silent = false) => {
    if (!silent) setLoading(true)
    try {
      const d = await fnRef.current()
      setData(d)
      setError(null)
    } catch (e) {
      setError(e)
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)

  useEffect(() => {
    load()
  }, [load])

  return { data, error, loading, reload: load, setData }
}

/** 주기적 갱신 */
export function useInterval(cb, ms, enabled = true) {
  const ref = useRef(cb)
  ref.current = cb
  useEffect(() => {
    if (!enabled) return
    const id = setInterval(() => ref.current(), ms)
    return () => clearInterval(id)
  }, [ms, enabled])
}
