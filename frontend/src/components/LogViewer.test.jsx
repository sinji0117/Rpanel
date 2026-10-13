import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import LogViewer, { lineClass } from './LogViewer'

describe('LogViewer', () => {
  it('renders lines with semantic colors', () => {
    render(
      <LogViewer
        lines={[
          { seq: 1, text: '▶ 빌드' },
          { seq: 2, text: '$ cargo build --release' },
          { seq: 3, text: 'error[E0433]: failed to resolve' },
          { seq: 4, text: '✔ 배포 완료' },
        ]}
      />,
    )
    expect(screen.getByText('▶ 빌드').className).toContain('text-sky-300')
    expect(screen.getByText('error[E0433]: failed to resolve').className).toContain('text-red-300')
    expect(screen.getByText('✔ 배포 완료').className).toContain('text-signal')
  })

  it('shows empty text', () => {
    render(<LogViewer lines={[]} emptyText="비어 있음" />)
    expect(screen.getByText('비어 있음')).toBeInTheDocument()
  })

  it('lineClass', () => {
    expect(lineClass('✘ failed: x')).toContain('text-fail')
    expect(lineClass('warning: unused')).toContain('amber')
    expect(lineClass('plain')).toBe('text-ink-100')
  })
})
