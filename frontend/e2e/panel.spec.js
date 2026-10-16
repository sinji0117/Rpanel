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
    domains: 'beripick.com www.beripick.com', health_path: '/', status: 'idle', current_commit: null, current_release: null, source: 'git', ssl_mode: 'none',
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

function seededBackend() {
  const backend = createMockBackend()
  backend.db.projects.push({ id: 1, name: 'Beripick', description: '', created_at: '', updated_at: '' })
  backend.db.apps.push({
    id: 2, project_id: 1, name: 'Beripick API', slug: 'beripick-api', kind: 'rust', git_url: 'ssh://git@100.100.199.123:2222/b/api.git',
    git_branch: 'main', root_dir: '', build_cmd: null, start_cmd: null, binary_name: 'beripick-api', output_dir: 'dist', port: 8100,
    api_proxy_port: null, domains: 'api.beripick.com www.api.beripick.com', health_path: '/api/health', status: 'running',
    current_commit: 'abc12345', current_release: 9, redis_db: null, backup_schedule: 'off', backup_keep: 7, source: 'git', ssl_mode: 'none',
  })
  backend.db.env[2] = [{ key: 'RUST_LOG', value: 'info' }]
  backend.db.seq = 20
  backend.db.loggedIn = true
  return backend
}

test('데이터베이스·Redis 생성 → 환경변수에 자동 주입 표시 → 전역 목록', async ({ page }) => {
  const backend = seededBackend()
  await backend.install(page)
  await page.goto('/apps/2')
  await page.getByRole('tab', { name: '데이터베이스' }).click()
  await page.getByRole('button', { name: '+ 데이터베이스' }).click()
  await expect(page.getByLabel('DB 이름', { exact: true })).toHaveValue('beripick_api')
  await page.getByRole('button', { name: '만들기' }).click()
  await expect(page.getByText('데이터베이스 beripick_api 생성됨')).toBeVisible()
  // 비밀번호는 기본 마스킹
  await expect(page.getByText('mysql://beripick_api:••••••@127.0.0.1:3306/beripick_api')).toBeVisible()
  await page.getByRole('button', { name: 'Redis 배정' }).click()
  await expect(page.getByText('DB 4')).toBeVisible()

  await page.getByRole('tab', { name: '환경변수' }).click()
  await expect(page.getByText('패널이 자동 주입')).toBeVisible()
  await expect(page.getByText('MariaDB · beripick_api')).toBeVisible()
  await expect(page.getByText('Redis · DB 4')).toBeVisible()

  await page.getByRole('link', { name: '데이터베이스' }).click()
  await expect(page.getByRole('cell', { name: 'beripick_api' }).first()).toBeVisible()
})

test('백업 → 목록, 크론 등록(잘못된 일정 거부), 사이트·Nginx 로그 통계', async ({ page }) => {
  const backend = seededBackend()
  backend.db.dbs.push({ id: 30, app_id: 2, name: 'beripick_api', username: 'beripick_api', env_key: 'DATABASE_URL', created_at: '' })
  await backend.install(page)
  await page.goto('/apps/2')

  await page.getByRole('tab', { name: '백업' }).click()
  await page.getByRole('button', { name: '지금 백업' }).click()
  await expect(page.getByText('20.0 KB')).toBeVisible()
  await page.getByLabel('예약').selectOption('daily')
  await page.getByLabel('대상별 보관 개수').fill('5')
  await page.getByRole('button', { name: '저장' }).click()
  await expect(page.getByText('예약 백업: 매일 3시, 5개 보관')).toBeVisible()

  await page.getByRole('tab', { name: '크론' }).click()
  await page.getByRole('button', { name: '+ 작업' }).click()
  await page.getByLabel('이름').fill('세션 정리')
  await page.getByRole('button', { name: '5분마다' }).click()
  await expect(page.getByText('다음 실행: Wed 2026-10-14 03:00:00 KST')).toBeVisible()
  await page.getByLabel('일정 (systemd OnCalendar)').fill('daily; rm -rf /')
  await page.getByLabel('명령').fill('./beripick-api --task cleanup')
  await page.getByRole('button', { name: '저장' }).click()
  await expect(page.getByRole('alert')).toContainText('일정 형식')
  await page.getByLabel('일정 (systemd OnCalendar)').fill('*:0/5')
  await page.getByRole('button', { name: '저장' }).click()
  await expect(page.getByText('크론 작업을 등록했습니다')).toBeVisible()
  await expect(page.getByText('$ ./beripick-api --task cleanup')).toBeVisible()

  await page.getByRole('tab', { name: 'Nginx 로그' }).click()
  await expect(page.getByTestId('stat-2xx')).toHaveText('3')
  await expect(page.getByTestId('stat-5xx')).toHaveText('1')

  await page.getByRole('link', { name: '사이트' }).click()
  await expect(page.getByText('응답 이상 1개')).toBeVisible()
  await expect(page.getByRole('cell', { name: /^200/ })).toBeVisible()

  await page.getByRole('link', { name: '백업' }).click()
  await expect(page.getByRole('heading', { name: '백업' })).toBeVisible()
  await expect(page.getByRole('cell', { name: 'Beripick API' }).first()).toBeVisible()
})


test('Rust 사이트 만들기 → 접속 정보 → 실행 파일 업로드·편집 → SSL 인증서', async ({ page }) => {
  const backend = createMockBackend()
  backend.db.loggedIn = true
  await backend.install(page)
  await page.goto('/sites')
  await page.getByRole('button', { name: '첫 사이트 만들기' }).click()
  const dialog = page.getByRole('dialog', { name: '사이트 추가' })
  await expect(dialog.getByRole('radio', { name: /Rust/ })).toHaveAttribute('aria-checked', 'true')
  await expect(dialog.getByRole('radio', { name: /PHP/ })).toHaveCount(0)
  await dialog.getByLabel('도메인').fill('Shop.example.com www.shop.example.com')
  await expect(dialog.getByLabel('사이트 유저')).toHaveValue('shop')
  await expect(dialog.getByLabel('시작 명령')).toHaveValue('./app')
  await expect(dialog.getByRole('radio', { name: /Node/ })).toHaveCount(0)
  await dialog.getByRole('button', { name: '사이트 만들기' }).click()

  const done = page.getByRole('dialog', { name: '사이트를 만들었습니다' })
  await expect(done.getByTestId('generated-password')).toHaveText('Gen3ratedPassw0rd')
  await expect(done.getByText('/home/shop/htdocs/shop.example.com')).toBeVisible()
  await expect(done.getByTestId('rust-next')).toContainText('cargo build --release')
  await done.getByRole('button', { name: '사이트로 이동' }).click()

  // 파일 관리자가 기본 탭
  await expect(page.getByRole('tab', { name: '파일' })).toHaveAttribute('aria-selected', 'true')
  await expect(page.getByRole('button', { name: '적용', exact: true })).toBeVisible()
  await expect(page.getByRole('tab', { name: '서비스 로그' })).toBeVisible()
  await expect(page.getByRole('tab', { name: 'PHP', exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '· README.txt', exact: true }).click()
  const editor = page.getByLabel('파일 내용')
  await expect(editor).toHaveValue('cargo build --release')
  await editor.fill('PORT 로 실행')
  await editor.press('Control+s')
  await expect(page.getByText('저장했습니다')).toBeVisible()
  expect(backend.db.files[Object.keys(backend.db.files)[0]]['htdocs/shop.example.com']['README.txt']).toBe('PORT 로 실행')
  await page.getByRole('button', { name: '닫기' }).click()

  // 빌드한 실행 파일 업로드
  await page.getByLabel('파일 업로드').setInputFiles({ name: 'app', mimeType: 'application/octet-stream', buffer: Buffer.from([0x7f, 0x45, 0x4c, 0x46]) })
  await expect(page.getByRole('button', { name: '· app', exact: true })).toBeVisible()

  // SSL: 잘못된 PEM → 오류, 올바른 PEM → 상태 표시
  await page.getByRole('tab', { name: 'SSL' }).click()
  await expect(page.getByText('HTTPS 없음')).toBeVisible()
  await page.getByRole('button', { name: '인증서 붙여넣기' }).click()
  await page.getByLabel('인증서 (체인 포함)').fill('-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----')
  await page.getByLabel('개인키').fill('-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----')
  await page.getByRole('button', { name: '설치' }).click()
  await expect(page.getByText('직접 올린 인증서')).toBeVisible()
  await expect(page.getByText('Jan 14 00:00:00 2027 GMT')).toBeVisible()

  // SSH · SFTP
  await page.getByRole('tab', { name: 'SSH · SFTP' }).click()
  await expect(page.getByText('sftp shop@')).toBeVisible()
  await page.getByLabel('새 비밀번호 (10자 이상)').fill('new-site-password')
  await page.getByRole('button', { name: '비밀번호 변경' }).click()
  await expect(page.getByText('비밀번호를 바꿨습니다')).toBeVisible()

  // 설정: 시작 명령
  await page.getByRole('tab', { name: '설정' }).click()
  await expect(page.getByLabel('시작 명령')).toHaveValue('./app')
})
