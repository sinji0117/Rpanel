import { useEffect, useRef, useState } from 'react'
import { openSocket } from './api'

/** seq 기준으로 중복을 제거하며 병합 (재연결 시 백로그가 다시 와도 안전) */
export function mergeLines(prev, incoming) {
  const lastSeq = prev.length ? prev[prev.length - 1].seq : 0
  const add = incoming.filter((l) => l.seq > lastSeq)
  if (!add.length) return prev
  add.sort((a, b) => a.seq - b.seq)
  return [...prev, ...add]
}

/** 배포 로그 실시간 스트림. 끝난 배포면 백로그만 받고 종료됩니다. */
export function useDeploymentLog(deploymentId, initialStatus) {
  const [lines, setLines] = useState([])
  const [status, setStatus] = useState(initialStatus)
  const [connected, setConnected] = useState(false)
  const statusRef = useRef(initialStatus)

  useEffect(() => {
    if (!deploymentId) return
    let ws
    let closed = false
    let retry
    let attempts = 0
    setLines([])

    const connect = async () => {
      try {
        ws = await openSocket(`/ws/deployments/${deploymentId}`)
      } catch {
        if (!closed && attempts++ < 5) retry = setTimeout(connect, 2000)
        return
      }
      ws.onopen = () => setConnected(true)
      ws.onmessage = (ev) => {
        let msg
        try {
          msg = JSON.parse(ev.data)
        } catch {
          return
        }
        if (msg.type === 'line') setLines((prev) => mergeLines(prev, [msg]))
        if (msg.type === 'status') {
          statusRef.current = msg.status
          setStatus(msg.status)
        }
      }
      ws.onclose = () => {
        setConnected(false)
        const done = !['queued', 'running', undefined, null].includes(statusRef.current)
        if (!closed && !done && attempts++ < 5) retry = setTimeout(connect, 2000)
      }
    }
    connect()
    return () => {
      closed = true
      clearTimeout(retry)
      ws?.close()
    }
  }, [deploymentId])

  return { lines, status, connected }
}

/** 서버 지표 스트림 (최근 N개 보관) */
export function useMetrics(keep = 60) {
  const [history, setHistory] = useState([])
  const [connected, setConnected] = useState(false)

  useEffect(() => {
    let ws
    let closed = false
    let retry
    const connect = async () => {
      try {
        ws = await openSocket('/ws/metrics')
      } catch {
        if (!closed) retry = setTimeout(connect, 5000)
        return
      }
      ws.onopen = () => setConnected(true)
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data)
          if (msg.type === 'metrics') setHistory((h) => [...h.slice(-(keep - 1)), msg.data])
        } catch {
          /* ignore */
        }
      }
      ws.onclose = () => {
        setConnected(false)
        if (!closed) retry = setTimeout(connect, 5000)
      }
    }
    connect()
    return () => {
      closed = true
      clearTimeout(retry)
      ws?.close()
    }
  }, [keep])

  return { history, latest: history[history.length - 1] ?? null, connected }
}
