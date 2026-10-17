import { describe, expect, it } from 'vitest'
import { mergeLines } from './useSockets'

describe('mergeLines', () => {
  it('appends only newer seq, sorted', () => {
    let s = []
    s = mergeLines(s, [{ seq: 1, text: 'a' }])
    s = mergeLines(s, [
      { seq: 3, text: 'c' },
      { seq: 2, text: 'b' },
    ])
    s = mergeLines(s, [{ seq: 2, text: 'dup' }])
    expect(s.map((l) => l.text)).toEqual(['a', 'b', 'c'])
  })
  it('returns same array when nothing new', () => {
    const s = [{ seq: 5, text: 'x' }]
    expect(mergeLines(s, [{ seq: 4, text: 'old' }])).toBe(s)
  })
})
