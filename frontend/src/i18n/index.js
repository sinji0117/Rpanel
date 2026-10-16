// 화면 언어 — t('한국어 원문', { 변수 }) 로 감싼 문구를 선택한 언어로 바꿉니다.
// 원문(한국어)이 곧 키이고, 번역이 없으면 원문을 그대로 보여 줍니다.
import { useEffect, useReducer } from 'react'
import en from './en'

const KEY = 'rpanel.lang'
const DICTS = { en }

export const LANGS = [
  { id: 'ko', label: '한국어' },
  { id: 'en', label: 'English' },
]

function initial() {
  try {
    const saved = localStorage.getItem(KEY)
    if (saved && LANGS.some((l) => l.id === saved)) return saved
  } catch {
    // 저장소를 쓸 수 없으면 브라우저 언어로
  }
  const nav = typeof navigator !== 'undefined' ? navigator.language || '' : ''
  return nav.toLowerCase().startsWith('ko') || !nav ? 'ko' : 'en'
}

let lang = initial()
const listeners = new Set()

export function getLang() {
  return lang
}

/** 날짜·숫자 표시용 locale */
export function locale() {
  return lang === 'ko' ? 'ko-KR' : 'en-US'
}

export function setLang(next) {
  if (!LANGS.some((l) => l.id === next) || next === lang) return
  lang = next
  try {
    localStorage.setItem(KEY, next)
  } catch {
    // 이번 방문 동안만 적용
  }
  if (typeof document !== 'undefined') document.documentElement.lang = next
  listeners.forEach((f) => f())
}

/** 문구 번역. vars: {n} 같은 자리표시자 값 */
export function t(text, vars) {
  let out = lang === 'ko' ? text : (DICTS[lang]?.[text] ?? text)
  if (vars) out = out.replace(/\{(\w+)\}/g, (m, k) => (vars[k] !== undefined && vars[k] !== null ? String(vars[k]) : m))
  return out
}

/** 언어가 바뀌면 다시 그리기 */
export function useLang() {
  const [, force] = useReducer((x) => x + 1, 0)
  useEffect(() => {
    listeners.add(force)
    return () => listeners.delete(force)
  }, [])
  return lang
}

if (typeof document !== 'undefined') document.documentElement.lang = lang

/**
 * 문장 안에 링크·굵은 글씨 같은 요소를 넣을 때: tx('… {cmd} …', { cmd: <b>…</b> })
 * 문장 전체가 하나의 번역 단위라 언어마다 어순이 달라도 자연스럽게 번역됩니다.
 */
export function tx(text, parts = {}) {
  const translated = lang === 'ko' ? text : (DICTS[lang]?.[text] ?? text)
  const out = []
  let last = 0
  translated.replace(/\{(\w+)\}/g, (m, k, idx) => {
    if (idx > last) out.push(translated.slice(last, idx))
    const v = parts[k]
    out.push(v === undefined || v === null ? m : typeof v === 'object' ? { ...v, key: `${k}-${idx}` } : String(v))
    last = idx + m.length
    return m
  })
  if (last < translated.length) out.push(translated.slice(last))
  return out
}
