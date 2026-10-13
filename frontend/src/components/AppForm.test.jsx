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

    await user.click(screen.getByRole('radio', { name: /Static/ }))
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

  it('edit mode locks slug and hides kind picker', () => {
    render(<AppForm mode="edit" initial={{ name: 'A', slug: 'a', kind: 'node', start_cmd: 'npm start', git_url: 'x' }} onSubmit={() => {}} />)
    expect(screen.getByLabelText('Slug')).toBeDisabled()
    expect(screen.queryByRole('radiogroup')).not.toBeInTheDocument()
    expect(screen.getByLabelText('시작 명령')).toHaveValue('npm start')
  })
})
