import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import en from './en'
import { setLang, t, tx } from './index'

// src 안의 t('…') / tx('…') 와 모듈 상수의 한국어 문구가 모두 영어 사전에 있는지
function sources(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) return e.name === 'i18n' ? [] : sources(p)
    return /\.(js|jsx)$/.test(e.name) && !/\.test\./.test(e.name) ? [p] : []
  })
}

describe('i18n', () => {
  it('모든 한국어 문구에 영어 번역이 있다', () => {
    const missing = []
    // 작은따옴표 문자열 + (JSX 태그를 넘지 않는) 큰따옴표 문자열
    const re = /'((?:\\.|[^'\\\n])*[가-힣](?:\\.|[^'\\\n])*)'|"((?:\\.|[^"\\\n<>])*[가-힣](?:\\.|[^"\\\n<>])*)"/g
    for (const f of sources(path.resolve(__dirname, '..'))) {
      const src = fs.readFileSync(f, 'utf8').replace(/^\s*(\/\/|\*|\/\*).*$/gm, '')
      for (const m of src.matchAll(re)) {
        const key = (m[1] ?? m[2]).replace(/\\n/g, '\n').replace(/\\(['"\\])/g, '$1')
        if (!(key in en)) missing.push(`${path.basename(f)}: ${key}`)
      }
    }
    expect(missing).toEqual([])
  })

  it('자리표시자가 원문과 번역에서 같다', () => {
    const ph = (s) => (s.match(/\{\w+\}/g) || []).sort().join(',')
    const bad = Object.entries(en).filter(([k, v]) => ph(k) !== ph(v))
    expect(bad).toEqual([])
  })

  it('언어를 바꾸면 문구가 바뀌고 자리표시자를 채운다', () => {
    setLang('ko')
    expect(t('{n}개', { n: 3 })).toBe('3개')
    setLang('en')
    expect(t('사이트')).toBe('Sites')
    expect(t('키 {n}개', { n: 5 })).toBe('5 keys')
    expect(t('없는 문구')).toBe('없는 문구')
    const parts = tx('저장 후 {redeploy}해야 반영됩니다.', { redeploy: 'X' })
    expect(parts.join('')).toBe('Changes take effect after you X.')
    setLang('ko')
  })
})
