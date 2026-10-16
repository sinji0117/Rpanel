import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import AppForm, { slugify, toPayload } from './AppForm'

describe('AppForm', () => {
  it('slugify', () => {
    expect(slugify('Beripick API v2!')).toBe('beripick-api-v2')
    expect(slugify('--x--')).toBe('x')
  })

  it('toPayload normalizes empty values', () => {
    const p = toPayload({ slug: ' ', build_cmd: '', api_proxy_port: '8100', binary_name: 'x' })
    expect(p).toMatchObject({ slug: null, build_cmd: null, api_proxy_port: 8100, binary_name: 'x' })
    expect(toPayload({ api_proxy_port: '' }).api_proxy_port).toBe(null)
  })

  it('auto-fills slug, switches fields by kind and submits', async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn()
    render(<AppForm onSubmit={onSubmit} />)

    await user.type(screen.getByLabelText('이름'), 'Beripick Front')
    expect(screen.getByLabelText('Slug')).toHaveValue('beripick-front')
    expect(screen.getByLabelText(/바이너리 이름/)).toBeInTheDocument()

    // 백엔드: 언어(Rust·Python)만, 프런트엔드: Static
    expect(screen.getByRole('radio', { name: /^Python/ })).toBeInTheDocument()
    expect(screen.queryByRole('radio', { name: /^Static/ })).not.toBeInTheDocument()
    await user.click(screen.getByRole('radio', { name: /프런트엔드/ }))
    expect(screen.queryByRole('radio', { name: /^Python/ })).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/바이너리 이름/)).not.toBeInTheDocument()
    expect(screen.queryByLabelText('헬스체크 경로')).not.toBeInTheDocument()
    await user.type(screen.getByLabelText('/api/ 프록시 포트'), '8100')
    await user.type(screen.getByLabelText('Git 저장소'), 'https://github.com/a/front.git')
    await user.type(screen.getByLabelText('도메인'), 'beripick.com www.beripick.com')
    await user.click(screen.getByRole('button', { name: '앱 만들기' }))

    expect(onSubmit).toHaveBeenCalledTimes(1)
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      kind: 'static',
      name: 'Beripick Front',
      slug: 'beripick-front',
      api_proxy_port: 8100,
      output_dir: 'dist',
      domains: 'beripick.com www.beripick.com',
    })
  })

  it('front end with backends picks the API from a list', async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn()
    render(<AppForm onSubmit={onSubmit} initial={{ role: 'admin', kind: 'static', name: 'Shop Admin' }} backends={[{ id: 1, name: 'Shop API', port: 8100 }]} />)
    expect(screen.getByRole('radio', { name: /관리자 화면/ })).toHaveAttribute('aria-checked', 'true')
    await user.selectOptions(screen.getByLabelText('API 연결 (백엔드)'), '8100')
    await user.type(screen.getByLabelText('Git 저장소'), 'https://github.com/a/admin.git')
    await user.click(screen.getByRole('button', { name: '앱 만들기' }))
    expect(onSubmit.mock.calls[0][0]).toMatchObject({ kind: 'static', api_proxy_port: 8100, name: 'Shop Admin' })
    expect(onSubmit.mock.calls[0][0].role).toBeUndefined()
  })

  it('edit mode locks slug and hides kind picker', () => {
    render(<AppForm mode="edit" initial={{ name: 'A', slug: 'a', kind: 'rust', binary_name: 'a-api', git_url: 'x' }} onSubmit={() => {}} />)
    expect(screen.getByLabelText('Slug')).toBeDisabled()
    expect(screen.queryByRole('radiogroup')).not.toBeInTheDocument()
    expect(screen.getByLabelText('바이너리 이름')).toHaveValue('a-api')
  })
})
