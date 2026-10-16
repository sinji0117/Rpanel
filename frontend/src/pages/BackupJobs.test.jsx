import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { DEFAULT_START } from '../components/CreateSiteModal'
import { describeSchedule } from './BackupJobs'

vi.mock('../lib/api', () => ({
  get: vi.fn(async (p) => (p === '/apps' ? [{ id: 1, name: '쇼핑몰' }] : { databases: [{ id: 9, name: 'shop' }] })),
  post: vi.fn(async (_p, body) => ({ id: 1, ...body })),
  put: vi.fn(),
  api: vi.fn(),
}))

describe('describeSchedule', () => {
  it('주기를 읽기 쉬운 문장으로', () => {
    expect(describeSchedule({ schedule: 'daily', hour: 3, minute: 5, weekday: 0 })).toBe('매일 03:05')
    expect(describeSchedule({ schedule: 'weekly', hour: 4, minute: 0, weekday: 0 })).toBe('매주 일요일 04:00')
    expect(describeSchedule({ schedule: 'weekly', hour: 23, minute: 30, weekday: 6 })).toBe('매주 토요일 23:30')
    expect(describeSchedule({ schedule: 'hourly', hour: 0, minute: 15, weekday: 0 })).toBe('매시간 15분')
  })

  it('사이트 종류별 기본 시작 명령', () => {
    expect(DEFAULT_START.rust).toBe('./app')
    expect(DEFAULT_START.python).toBe('python app.py')
  })
})

describe('BackupJobForm', () => {
  it('대상·주기에 맞는 항목만 보이고, 숫자로 보낸다', async () => {
    const { BackupJobForm } = await import('./BackupJobs')
    const { post } = await import('../lib/api')
    const onSaved = vi.fn()
    render(<BackupJobForm onSaved={onSaved} onClose={() => {}} />)
    // 기본: 전체 · 매일 → 요일 선택 없음
    expect(screen.queryByLabelText('요일')).not.toBeInTheDocument()
    await userEvent.selectOptions(screen.getByLabelText('백업할 대상'), 'database')
    await userEvent.selectOptions(await screen.findByLabelText('데이터베이스'), '9')
    await userEvent.selectOptions(screen.getByLabelText('주기'), 'weekly')
    await userEvent.selectOptions(screen.getByLabelText('요일'), '3')
    await userEvent.selectOptions(screen.getByLabelText('시'), '4')
    expect(screen.getByText(/매주 수요일 04:00/)).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '추가' }))
    expect(post).toHaveBeenCalledWith('/backup-jobs', expect.objectContaining({ target: 'database', database_id: 9, app_id: null, schedule: 'weekly', weekday: 3, hour: 4, keep_count: 7 }))
    expect(onSaved).toHaveBeenCalled()
  })
})
