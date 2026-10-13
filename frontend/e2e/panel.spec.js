import { expect, test } from '@playwright/test'
import { createMockBackend } from './mockApi'

test('최초 설정 → 대시보드 실시간 지표', async ({ page }) => {
  const backend = createMockBackend({ needsSetup: true })
  await backend.install(page)
  await page.goto('/')

  await expect(page.getByRole('heading', { name: '초기 설정' })).toBeVisible()
  await page.getByLabel('이메일').fill('Admin@Example.com')
  await page.getByLabel('비밀번호', { exact: true }).fill('short')
  await page.getByLabel('비밀번호 확인').fill('short')
  // HTML minLength 검증을 우회해 서버 오류 메시지도 확인
  await page.locator('form').evaluate((f) => f.setAttribute('novalidate', ''))
  await page.getByRole('button', { name: '관리자 계정 만들기' }).click()
  await expect(page.getByRole('alert')).toContainText('10자 이상')

  await page.getByLabel('비밀번호', { exact: true }).fill('correct-horse-1')
  await page.getByLabel('비밀번호 확인').fill('correct-horse-1')
  await page.getByRole('button', { name: '관리자 계정 만들기' }).click()

  await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
  await expect(page.getByText('DRY-RUN 모드')).toBeVisible()
  await expect(page.getByTestId('cpu-value')).toHaveText('23.5%')
  await expect(page.getByText('실시간')).toBeVisible()
  await expect(page.getByRole('meter', { name: '/' })).toHaveAttribute('aria-valuenow', '21')
})

test('로그인 실패 → 성공', async ({ page }) => {
  await createMockBackend().install(page)
  await page.goto('/')
  await expect(page.getByRole('heading', { name: '로그인' })).toBeVisible()
  await page.getByLabel('이메일').fill('admin@example.com')
  await page.getByLabel('비밀번호').fill('wrong-password')
  await page.getByRole('button', { name: '로그인' }).click()
  await expect(page.getByRole('alert')).toContainText('올바르지 않습니다')
  await page.getByLabel('비밀번호').fill('correct-horse-1')
  await page.getByRole('button', { name: '로그인' }).click()
  await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
})

async function loginAs(page, backend) {
  backend.db.loggedIn = true // 리프레시 쿠키가 있는 상태로 시작
  await backend.install(page)
}

test('프로젝트 → Rust 앱 생성 → 배포 실시간 로그 → 롤백 대상 표시', async ({ page }) => {
  const backend = createMockBackend()
  await loginAs(page, backend)
  await page.goto('/projects')

  await page.getByRole('button', { name: '첫 프로젝트 만들기' }).click()
  await page.getByLabel('이름').fill('Beripick')
  await page.getByLabel('설명').fill('메인 서비스')
  await page.getByRole('button', { name: '저장' }).click()
  await expect(page.getByRole('heading', { name: 'Beripick' })).toBeVisible()

  await page.getByRole('button', { name: '+ 새 앱' }).click()
  const dialog = page.getByRole('dialog', { name: '새 앱' })
  await expect(dialog.getByRole('radio', { name: /Rust/ })).toHaveAttribute('aria-checked', 'true')
  await dialog.getByLabel('이름', { exact: true }).fill('Beripick API')
  await expect(dialog.getByLabel('Slug')).toHaveValue('beripick-api')
  await dialog.getByLabel('Git 저장소').fill('ssh://git@100.100.199.123:2222/beri/backend.git')
  await dialog.getByLabel('바이너리 이름').fill('hello-api')
  await dialog.getByLabel('도메인').fill('api.beripick.com')
  await dialog.getByLabel('헬스체크 경로').fill('/api/health')
  await dialog.getByRole('button', { name: '앱 만들기' }).click()

  await expect(page.getByRole('heading', { name: /Beripick API/ })).toBeVisible()
  await expect(page.getByText('127.0.0.1:8100')).toBeVisible()
  await expect(page.getByText('아직 배포가 없습니다')).toBeVisible()

  await page.getByRole('button', { name: '배포', exact: true }).click()
  await expect(page.getByRole('heading', { name: /배포 #\d+/ })).toBeVisible()
  const log = page.getByTestId('log-viewer')
  await expect(log).toContainText('✔ 배포 완료')
  await expect(log).toContainText('Compiling hello-api')
  // seq 중복 제거: "▶ 소스 가져오기"는 한 번만
  await expect(log.getByText('▶ 소스 가져오기')).toHaveCount(1)
  await expect(page.getByRole('heading', { name: /배포 #\d+/ }).getByText('성공')).toBeVisible()

  await page.getByRole('link', { name: '앱으로' }).click()
  await expect(page.getByRole('cell', { name: /현재/ })).toBeVisible()
})

test('환경변수 .env 붙여넣기·저장 + Deploy key·Webhook', async ({ page }) => {
  const backend = createMockBackend()
  backend.db.projects.push({ id: 1, name: 'P', description: '', created_at: '', updated_at: '' })
  backend.db.apps.push({
    id: 2, project_id: 1, name: 'Front', slug: 'front', kind: 'static', git_url: 'https://github.com/a/b.git', git_branch: 'main',
    root_dir: '', build_cmd: null, start_cmd: null, binary_name: null, output_dir: 'dist', port: null, api_proxy_port: 8100,
    domains: 'beripick.com www.beripick.com', health_path: '/', status: 'idle', current_commit: null, current_release: null,
  })
  backend.db.env[2] = []
  backend.db.seq = 10
  await loginAs(page, backend)
  await page.goto('/apps/2')

  await page.getByRole('tab', { name: '환경변수' }).click()
  await page.getByRole('button', { name: '.env 붙여넣기' }).click()
  await page.getByLabel('.env 내용').fill('# comment\nexport VITE_API_URL="/api"\nVITE_TITLE=베리픽\n')
  await page.getByRole('button', { name: '가져오기' }).click()
  await expect(page.getByLabel('키 1')).toHaveValue('VITE_API_URL')
  await expect(page.getByLabel('값 1')).toHaveAttribute('type', 'password')
  await page.getByRole('button', { name: '값 보기' }).click()
  await expect(page.getByLabel('값 2')).toHaveValue('베리픽')
  await page.getByRole('button', { name: '저장', exact: true }).click()
  await expect(page.getByText('환경변수 저장됨')).toBeVisible()
  expect(backend.db.env[2].map((v) => v.key)).toEqual(['VITE_API_URL', 'VITE_TITLE'])

  await page.getByRole('tab', { name: 'Git · 웹훅' }).click()
  await expect(page.getByLabel('Webhook URL')).toHaveValue(/\/api\/hooks\/front$/)
  await page.getByRole('button', { name: 'Deploy key 만들기' }).click()
  await expect(page.getByText('ssh-ed25519 AAAAC3NzaTEST rpanel-front')).toBeVisible()
})
