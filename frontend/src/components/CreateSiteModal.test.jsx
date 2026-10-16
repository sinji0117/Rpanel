import { describe, expect, it } from 'vitest'
import { joinPath, parentPath, relRoot } from '../pages/SiteTabs'
import { genPassword, userFromDomain } from './CreateSiteModal'

describe('site helpers', () => {
  it('userFromDomain matches server rules', () => {
    expect(userFromDomain('beripick.com')).toBe('beripick')
    expect(userFromDomain('www.bandiuri.co.kr www.x.com')).toBe('bandiuri')
    expect(userFromDomain('9shop.com')).toBe('site-9shop')
    expect(userFromDomain('ab.io')).toBe('ab-site')
    expect(userFromDomain('My_Shop.com')).toBe('my-shop')
    expect(userFromDomain('')).toBe('')
  })

  it('genPassword', () => {
    const p = genPassword()
    expect(p).toHaveLength(16)
    expect(p).toMatch(/^[A-Za-z0-9]+$/)
    expect(genPassword()).not.toBe(p)
  })

  it('paths', () => {
    expect(relRoot({ site_user: 'shop', site_root: '/home/shop/htdocs/shop.com' })).toBe('htdocs/shop.com')
    expect(relRoot({ site_user: null })).toBe('')
    expect(joinPath('htdocs/a.com', 'app')).toBe('htdocs/a.com/app')
    expect(joinPath('', '.ssh')).toBe('.ssh')
    expect(parentPath('htdocs/a.com/assets')).toBe('htdocs/a.com')
    expect(parentPath('htdocs')).toBe('')
  })
})
