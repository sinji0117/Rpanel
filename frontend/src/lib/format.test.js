import { describe, expect, it } from 'vitest'
import { ago, bytes, duration, mergeEnv, parseDotenv, percent, shortSha, uptime } from './format'

describe('format', () => {
  it('bytes', () => {
    expect(bytes(0)).toBe('0 B')
    expect(bytes(1536)).toBe('1.5 KB')
    expect(bytes(16 * 1024 ** 3)).toBe('16.0 GB')
    expect(bytes(200 * 1024 ** 3)).toBe('200 GB')
    expect(bytes(null)).toBe('–')
  })

  it('percent clamps', () => {
    expect(percent(5, 10)).toBe(50)
    expect(percent(20, 10)).toBe(100)
    expect(percent(1, 0)).toBe(0)
  })

  it('uptime / duration / ago', () => {
    expect(uptime(93784)).toBe('1일 2시간')
    expect(uptime(3700)).toBe('1시간 1분')
    expect(duration('2026-01-01T00:00:00Z', '2026-01-01T00:01:05Z')).toBe('1분 5초')
    expect(duration(null, null)).toBe('–')
    const now = Date.parse('2026-01-01T01:00:00Z')
    expect(ago('2026-01-01T00:59:58Z', now)).toBe('방금')
    expect(ago('2026-01-01T00:55:00Z', now)).toBe('5분 전')
    expect(ago('2025-12-30T01:00:00Z', now)).toBe('2일 전')
  })

  it('shortSha', () => {
    expect(shortSha('0123456789abcdef')).toBe('01234567')
    expect(shortSha(null)).toBe('–')
  })

  it('parseDotenv handles comments, export, quotes', () => {
    const text = `# db
export DATABASE_URL="mysql://u:p@127.0.0.1:3306/db"
REDIS_URL='redis://:pw@127.0.0.1:6379/1'
EMPTY=
bad line
1BAD=x
  RUST_LOG = info  `
    expect(parseDotenv(text)).toEqual([
      { key: 'DATABASE_URL', value: 'mysql://u:p@127.0.0.1:3306/db' },
      { key: 'REDIS_URL', value: 'redis://:pw@127.0.0.1:6379/1' },
      { key: 'EMPTY', value: '' },
      { key: 'RUST_LOG', value: 'info' },
    ])
  })

  it('mergeEnv overrides by key and keeps order', () => {
    const merged = mergeEnv(
      [
        { key: 'A', value: '1' },
        { key: 'B', value: '2' },
      ],
      [
        { key: 'B', value: '3' },
        { key: 'C', value: '4' },
      ],
    )
    expect(merged).toEqual([
      { key: 'A', value: '1' },
      { key: 'B', value: '3' },
      { key: 'C', value: '4' },
    ])
  })
})
