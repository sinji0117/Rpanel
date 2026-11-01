import { useCallback, useEffect, useRef, useState } from 'react'

/**
 * 간단한 데이터 로딩 훅
 * - loading: 처음(또는 deps 가 바뀌어) 불러오는 중 → 로딩 화면
 * - refreshing: reload() 로 다시 불러오는 중 → 지금 화면은 그대로 두고 새로고침 버튼만 돎
 * - reload(true): 조용히 (주기적 갱신용, 아무 표시 없음)
 */
export function useFetch(fn, deps = []) {
  const [data, setData] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const fnRef = useRef(fn)
  fnRef.current = fn
  // 늦게 도착한 이전 응답이 최신 값을 덮어쓰지 않도록
  const seq = useRef(0)

  // mode: 'initial' | 'refresh' | 'silent'
  const run = useCallback(async (mode) => {
    const my = ++seq.current
    if (mode === 'initial') setLoading(true)
    else if (mode === 'refresh') setRefreshing(true)
    try {
      const d = await fnRef.current()
      if (my !== seq.current) return
      setData(d)
      setError(null)
    } catch (e) {
      if (my !== seq.current) return
      setError(e)
    } finally {
      if (my === seq.current) {
        setLoading(false)
        setRefreshing(false)
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)

  // onClick={reload} 처럼 이벤트가 넘어와도 예전처럼 '조용히'로 취급 (truthy)
  const reload = useCallback((silent = false) => run(silent ? 'silent' : 'refresh'), [run])

  useEffect(() => {
    run('initial')
  }, [run])

  return { data, error, loading, refreshing, reload, setData }
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
