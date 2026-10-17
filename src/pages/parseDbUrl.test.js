import { describe, expect, it } from 'vitest'
import { parseDbUrl } from './AppTabs'

describe('parseDbUrl', () => {
  it('접속 URL을 항목별로 나눈다', () => {
    expect(parseDbUrl('mysql://shop_u:p%40ss@127.0.0.1:3306/shop')).toEqual({
      user: 'shop_u',
      password: 'p@ss',
      host: '127.0.0.1',
      port: '3306',
      database: 'shop',
    })
  })
  it('포트가 없으면 3306', () => {
    expect(parseDbUrl('mysql://a:b@db.local/x').port).toBe('3306')
  })
  it('형식이 다르면 null', () => {
    expect(parseDbUrl('nope')).toBeNull()
  })
})
