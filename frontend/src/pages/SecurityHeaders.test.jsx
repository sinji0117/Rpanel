import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(async () => ({})),
  del: vi.fn(),
  download: vi.fn(),
  uploadFile: vi.fn(),
  api: vi.fn(),
}))

describe('SecurityHeaders', () => {
  it('처음 켜면 기본 정책을 채우고, 모드와 정책을 보낸다', async () => {
    const { SecurityHeaders, DEFAULT_CSP } = await import('./SiteTabs')
    const { put } = await import('../lib/api')
    const onChanged = vi.fn()
    render(<SecurityHeaders app={{ id: 7, csp_mode: 'off', csp_policy: null }} onChanged={onChanged} />)

    expect(screen.getByRole('radio', { name: '끔' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByLabelText('CSP 정책')).toHaveValue('')

    await userEvent.click(screen.getByRole('radio', { name: '점검만 (Report-Only)' }))
    expect(screen.getByLabelText('CSP 정책')).toHaveValue(DEFAULT_CSP)

    await userEvent.click(screen.getByRole('button', { name: '저장' }))
    expect(put).toHaveBeenCalledWith('/apps/7/security', { csp_mode: 'report', csp_policy: DEFAULT_CSP })
    expect(onChanged).toHaveBeenCalled()
  })

  it('저장된 값을 보여 주고, 끌 때도 정책은 그대로 보낸다', async () => {
    const { SecurityHeaders } = await import('./SiteTabs')
    const { put } = await import('../lib/api')
    put.mockClear()
    render(<SecurityHeaders app={{ id: 3, csp_mode: 'enforce', csp_policy: "default-src 'self'" }} />)
    expect(screen.getByRole('radio', { name: '적용 (막기)' })).toHaveAttribute('aria-checked', 'true')
    await userEvent.click(screen.getByRole('radio', { name: '끔' }))
    await userEvent.click(screen.getByRole('button', { name: '저장' }))
    expect(put).toHaveBeenCalledWith('/apps/3/security', { csp_mode: 'off', csp_policy: "default-src 'self'" })
  })
})
