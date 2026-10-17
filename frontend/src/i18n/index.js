// 화면 언어 — t('한국어 원문', { 변수 }) 로 감싼 문구를 선택한 언어로 바꿉니다.
// 원문(한국어)이 곧 키입니다. 고른 언어에 번역이 없으면 영어, 영어도 없으면 원문을 보여 줍니다.
// 영어는 기본으로 들어 있고, 다른 언어 사전은 그 언어를 고를 때 따로 내려받습니다.
import { useEffect, useReducer } from 'react'
import en from './en'

const KEY = 'rpanel.lang'

/** 화면에 보이는 순서. bcp: 날짜·숫자 표시와 <html lang> 에 쓰는 코드 */
export const LANGS = [
  { id: 'en', label: 'English', bcp: 'en-US' },
  { id: 'de', label: 'Deutsch', bcp: 'de-DE' },
  { id: 'es', label: 'Español', bcp: 'es-ES' },
  { id: 'et', label: 'Eesti', bcp: 'et-EE' },
  { id: 'pt', label: 'Português (Europeu)', bcp: 'pt-PT' },
  { id: 'fr', label: 'Français', bcp: 'fr-FR' },
  { id: 'ga', label: 'Gaeilge', bcp: 'ga-IE' },
  { id: 'ja', label: '日本語', bcp: 'ja-JP' },
  { id: 'it', label: 'Italiano', bcp: 'it-IT' },
  { id: 'nl', label: 'Nederlands', bcp: 'nl-NL' },
  { id: 'pl', label: 'Polski', bcp: 'pl-PL' },
  { id: 'ru', label: 'Русский', bcp: 'ru-RU' },
  { id: 'sk', label: 'Slovenčina', bcp: 'sk-SK' },
  { id: 'cs', label: 'Čeština', bcp: 'cs-CZ' },
  { id: 'vi', label: 'Tiếng Việt', bcp: 'vi-VN' },
  { id: 'zh', label: '中文', bcp: 'zh-CN' },
  { id: 'ko', label: '한국어', bcp: 'ko-KR' },
  { id: 'bg', label: 'Български', bcp: 'bg-BG' },
  { id: 'id', label: 'Bahasa Indonesia', bcp: 'id-ID' },
  { id: 'tr', label: 'Türkçe', bcp: 'tr-TR' },
  { id: 'hu', label: 'Magyar', bcp: 'hu-HU' },
  { id: 'no', label: 'Norsk', bcp: 'nb-NO' },
]

// 언어별 사전은 따로 묶여서(코드 분할) 고를 때만 내려받습니다
const LOADERS = import.meta.glob(['./*.js', '!./index.js', '!./en.js', '!./*.test.js'])
const DICTS = { en }

const known = (id) => LANGS.some((l) => l.id === id)

/** 브라우저 언어 → 지원하는 언어 (없으면 영어) */
export function matchBrowser(list) {
  for (const raw of list) {
    const tag = String(raw || '').toLowerCase()
    const base = tag.split('-')[0]
    if (base === 'nb' || base === 'nn') return 'no'
    if (known(base)) return base
  }
  return 'en'
}

function initial() {
  try {
    const saved = localStorage.getItem(KEY)
    if (saved && known(saved)) return saved
  } catch {
    // 저장소를 쓸 수 없으면 브라우저 언어로
  }
  if (typeof navigator === 'undefined') return 'ko'
  const list = navigator.languages?.length ? navigator.languages : [navigator.language]
  return list.filter(Boolean).length ? matchBrowser(list) : 'ko'
}

let lang = initial()
const listeners = new Set()

/** 사전 내려받기 (이미 있으면 바로 끝) */
function load(id) {
  if (id === 'ko' || DICTS[id]) return Promise.resolve()
  const f = LOADERS[`./${id}.js`]
  if (!f) return Promise.resolve()
  return f()
    .then((m) => {
      DICTS[id] = m.default
    })
    .catch(() => {
      // 내려받지 못하면 영어로 보여 줌
    })
}

/** 처음 고른 언어의 사전이 준비되면 끝나는 약속 (첫 화면을 그리기 전에 기다림) */
export const ready = load(lang)

export function getLang() {
  return lang
}

const bcp = (id) => LANGS.find((l) => l.id === id)?.bcp ?? id

/** 날짜·숫자 표시용 locale */
export function locale() {
  return bcp(lang)
}

function setHtmlLang() {
  if (typeof document !== 'undefined') document.documentElement.lang = bcp(lang)
}

export function setLang(next) {
  if (!known(next) || next === lang) return Promise.resolve()
  lang = next
  try {
    localStorage.setItem(KEY, next)
  } catch {
    // 이번 방문 동안만 적용
  }
  setHtmlLang()
  // 사전이 오기 전에는 영어로 그리고, 오면 다시 그림
  listeners.forEach((f) => f())
  return load(next).then(() => {
    if (lang === next) listeners.forEach((f) => f())
  })
}

function lookup(text) {
  if (lang === 'ko') return text
  return DICTS[lang]?.[text] ?? en[text] ?? text
}

/** 문구 번역. vars: {n} 같은 자리표시자 값 */
export function t(text, vars) {
  let out = lookup(text)
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

setHtmlLang()

/**
 * 문장 안에 링크·굵은 글씨 같은 요소를 넣을 때: tx('… {cmd} …', { cmd: <b>…</b> })
 * 문장 전체가 하나의 번역 단위라 언어마다 어순이 달라도 자연스럽게 번역됩니다.
 */
export function tx(text, parts = {}) {
  const translated = lookup(text)
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
