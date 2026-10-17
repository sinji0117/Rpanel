import { useCallback, useEffect, useRef, useState } from 'react'

/** 간단한 데이터 로딩 훅. reload()로 다시 불러옵니다. */
export function useFetch(fn, deps = []) {
  const [data, setData] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(true)
  const fnRef = useRef(fn)
  fnRef.current = fn
  // 늦게 도착한 이전 응답이 최신 값을 덮어쓰지 않도록
  const seq = useRef(0)

  const load = useCallback(async (silent = false) => {
    const my = ++seq.current
    if (!silent) setLoading(true)
    try {
      const d = await fnRef.current()
      if (my !== seq.current) return
      setData(d)
      setError(null)
    } catch (e) {
      if (my !== seq.current) return
      setError(e)
    } finally {
      if (my === seq.current) setLoading(false)
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
