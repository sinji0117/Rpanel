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
  // 모니터링 그래프 4개 + 가동 시간 (부팅 시각으로 계산)
  for (const name of ['CPU 사용률', '메모리 사용률', '디스크 사용률', 'Load Average']) await expect(page.getByRole('img', { name })).toBeVisible()
  await expect(page.getByText('/ (371 GB)')).toBeVisible()
  await expect(page.getByText('6 CPU').first()).toBeVisible()
  await expect(page.getByTestId('uptime-value')).toHaveText('3일 4시간 5분')
  await page.getByLabel('기간').selectOption('24h')
  await expect(page.getByRole('img', { name: 'CPU 사용률' }).locator('circle')).toHaveCount(0) // 점이 많으면 선만
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
  // 같은 저장소의 React 프런트엔드 → npm 빌드
  await expect(dialog.getByLabel('빌드 결과 폴더')).toHaveCount(0)
  await dialog.getByLabel('프런트엔드 폴더').fill('frontend')
  await expect(dialog.getByText('프로그램에서 frontend/dist 경로로 읽으면 됩니다')).toBeVisible()
  await dialog.getByLabel('도메인').fill('api.beripick.com')
  await dialog.getByLabel('헬스체크 경로').fill('/api/health')
  await dialog.getByRole('button', { name: '앱 만들기' }).click()

  await expect(page.getByRole('heading', { name: /Beripick API/ })).toBeVisible()
  await expect(page.getByText('127.0.0.1:8100')).toBeVisible()
  expect(backend.db.apps.find((a) => a.name === 'Beripick API')).toMatchObject({ frontend_dir: 'frontend', output_dir: 'dist' })
  await expect(page.getByText('아직 배포가 없습니다')).toBeVisible()

  // 배포를 눌러도 앱 화면에 머물고, 배포 탭 아래에서 실시간 로그를 봄
  await page.getByRole('button', { name: '배포', exact: true }).click()
  const panel = page.getByRole('region', { name: /배포 #\d+ 로그/ })
  await expect(panel).toBeVisible()
  const log = panel.getByTestId('log-viewer')
  await expect(log).toContainText('✔ 배포 완료')
  await expect(log).toContainText('Compiling hello-api')
  // seq 중복 제거: "▶ 소스 가져오기"는 한 번만
  await expect(log.getByText('▶ 소스 가져오기')).toHaveCount(1)
  await expect(panel.getByText('성공')).toBeVisible()
  await expect(page.getByRole('cell', { name: /현재/ })).toBeVisible()

  // 다른 탭에 있다가도 "배포 로그" 버튼으로 바로 돌아옴
  await page.getByRole('tab', { name: '환경변수' }).click()
  await page.getByRole('button', { name: '배포 로그' }).click()
  await expect(page.getByRole('region', { name: /배포 #\d+ 로그/ }).getByTestId('log-viewer')).toContainText('✔ 배포 완료')
  // 전체 화면으로 보기
  await page.getByRole('link', { name: '전체 화면 ↗' }).click()
  await expect(page.getByRole('heading', { name: /배포 #\d+/ })).toBeVisible()
  await page.getByRole('link', { name: '앱으로' }).click()

  // 서비스 중지 → systemd 상태가 바로 "멈춤" 으로, 시작 버튼으로 바뀜
  await expect(page.getByTestId('systemd-state')).toContainText('실행 중')
  await page.getByRole('button', { name: '■' }).click()
  await expect(page.getByTestId('systemd-state')).toContainText('멈춤')
  await page.getByRole('button', { name: '▶' }).click()
  await expect(page.getByTestId('systemd-state')).toContainText('실행 중')
})

test('환경변수 .env 붙여넣기·저장 + Deploy key·Webhook', async ({ page }) => {
  const backend = createMockBackend()
  backend.db.projects.push({ id: 1, name: 'P', description: '', created_at: '', updated_at: '' })
  backend.db.apps.push({
    id: 2,
    project_id: 1,
    name: 'Front',
    slug: 'front',
    kind: 'static',
    git_url: 'https://github.com/a/b.git',
    git_branch: 'main',
    root_dir: '',
    build_cmd: null,
    start_cmd: null,
    binary_name: null,
    output_dir: 'dist',
    port: null,
    api_proxy_port: 8100,
    domains: 'beripick.com www.beripick.com',
    health_path: '/',
    status: 'idle',
    current_commit: null,
    current_release: null,
    source: 'git',
    ssl_mode: 'none',
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
    id: 2,
    project_id: 1,
    name: 'Beripick API',
    slug: 'beripick-api',
    kind: 'rust',
    git_url: 'ssh://git@100.100.199.123:2222/b/api.git',
    git_branch: 'main',
    root_dir: '',
    build_cmd: null,
    start_cmd: null,
    binary_name: 'beripick-api',
    output_dir: 'dist',
    port: 8100,
    api_proxy_port: null,
    domains: 'api.beripick.com www.api.beripick.com',
    health_path: '/api/health',
    status: 'running',
    current_commit: 'abc12345',
    current_release: 9,
    redis_db: null,
    backup_schedule: 'off',
    backup_keep: 7,
    source: 'git',
    ssl_mode: 'none',
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

  await page.getByRole('link', { name: 'MariaDB' }).click()
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
  // 이 앱의 백업 예약 추가 (대상은 이 앱으로 고정)
  await page.getByRole('button', { name: '+ 예약 추가' }).click()
  const jobDlg = page.getByRole('dialog', { name: '백업 예약 추가' })
  await expect(jobDlg.getByLabel('앱', { exact: true })).toHaveValue('2')
  await jobDlg.getByLabel('보관 개수').fill('5')
  await jobDlg.getByRole('button', { name: '추가' }).click()
  await expect(page.getByText('예약을 추가했습니다')).toBeVisible()
  await expect(page.getByText(/매일 03:00 · 5개 보관/)).toBeVisible()

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
  await expect(page.getByRole('heading', { name: '백업', exact: true })).toBeVisible()
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
  await expect(dialog.getByTestId('site-summary')).toContainText('shop')
  await dialog.getByText('고급 설정 (선택)').click()
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

  // 사이트 계정: 기본 계정 + 추가 계정 만들기 → 잠금 → 비밀번호 → 삭제
  page.on('dialog', (d) => d.accept(d.type() === 'prompt' ? '' : undefined))
  await page.getByRole('tab', { name: '사이트 계정' }).click()
  const accounts = page.getByRole('table', { name: '사이트 계정 목록' })
  await expect(accounts.getByText('shop', { exact: true })).toBeVisible()
  await expect(accounts.getByRole('button', { name: 'shop 계정 삭제' })).toHaveCount(0)
  await page.getByRole('button', { name: '+ 계정 추가' }).click()
  const acc = page.getByRole('dialog', { name: '사이트 계정 추가' })
  await acc.getByLabel('계정 이름').fill('shop-dev')
  await acc.getByLabel('메모 (선택)').fill('외주 디자이너')
  await acc.getByRole('button', { name: '계정 만들기' }).click()
  await expect(page.getByTestId('account-password')).toHaveText('AutoGen3rated-pw-01')
  await page.getByRole('button', { name: '확인' }).click()
  const row = accounts.getByRole('row', { name: /shop-dev/ })
  await expect(row.getByText('SFTP 전용')).toBeVisible()
  await row.getByRole('button', { name: '잠금', exact: true }).click()
  await expect(row.getByRole('button', { name: '잠금 해제' })).toBeVisible()
  await row.getByRole('button', { name: '수정' }).click()
  const ed = page.getByRole('dialog', { name: '계정 shop-dev 수정' })
  await ed.getByRole('radio', { name: /SSH \+ SFTP/ }).click()
  await ed.getByRole('button', { name: '저장' }).click()
  await expect(row.getByText('SSH + SFTP')).toBeVisible()
  await row.getByRole('button', { name: '비밀번호' }).click()
  await expect(page.getByTestId('account-password')).toHaveText('AutoGen3rated-pw-02')
  await page.getByRole('button', { name: '확인' }).click()
  await accounts.getByRole('button', { name: 'shop-dev 계정 삭제' }).click()
  await expect(accounts.getByText('shop-dev')).toHaveCount(0)

  // 설정: 시작 명령
  await page.getByRole('tab', { name: '설정' }).click()
  await expect(page.getByLabel('시작 명령')).toHaveValue('./app')
})

test('DB·Redis 계정 추가·삭제, 백업 예약 추가·수정·실행·삭제', async ({ page }) => {
  const backend = seededBackend()
  await backend.install(page)
  page.on('dialog', (d) => d.accept(d.type() === 'prompt' ? 'shop' : undefined))
  await page.goto('/mariadb')

  // 앱에 연결하지 않은 DB 만들기 → 접속 정보 창
  await page.getByRole('button', { name: '+ 데이터베이스' }).first().click()
  const create = page.getByRole('dialog', { name: '데이터베이스 만들기' })
  await create.getByLabel('DB 이름').fill('shop')
  await create.getByRole('button', { name: '만들기' }).click()
  const cred = page.getByRole('dialog', { name: 'shop 접속 정보' })
  await expect(cred.getByText('mysql://shop:••••••@127.0.0.1:3306/shop')).toBeVisible()
  await cred.getByRole('button', { name: '확인' }).click()
  const card = page.getByTestId('db-shop')
  await expect(card.getByText('앱에 연결 안 됨')).toBeVisible()

  // 읽기 전용 계정 추가 → 새 비밀번호 표시 → 목록에 2개
  await card.getByRole('button', { name: '+ 계정' }).click()
  const form = page.getByRole('form', { name: 'shop 계정 추가' })
  await form.getByLabel('계정 이름').fill('shop_ro')
  await form.getByLabel('권한').selectOption('read')
  await form.getByLabel('비밀번호').fill('Report_pass_2026')
  await form.getByRole('button', { name: '계정 만들기' }).click()
  await expect(page.getByTestId('new-password')).toHaveText('Report_pass_2026')
  await page.getByRole('button', { name: '확인' }).click()
  await expect(card.getByRole('cell', { name: '읽기 전용' })).toBeVisible()
  // 같은 이름은 거부
  await card.getByRole('button', { name: '+ 계정' }).click()
  await form.getByLabel('계정 이름').fill('shop_ro')
  await form.getByRole('button', { name: '계정 만들기' }).click()
  await expect(form.getByRole('alert')).toContainText('이미 있습니다')
  await form.getByRole('button', { name: '취소' }).click()

  // 앱에 연결
  await card.getByRole('button', { name: '앱 연결' }).click()
  const link = page.getByRole('dialog', { name: 'shop 앱 연결' })
  await link.getByLabel('앱').selectOption('2')
  await link.getByLabel('환경변수 이름').fill('SHOP_DB_URL')
  await link.getByRole('button', { name: '저장' }).click()
  await expect(card.getByRole('link', { name: 'Beripick API' })).toBeVisible()
  await expect(card.getByText('SHOP_DB_URL')).toBeVisible()

  // 계정 삭제
  await card.getByRole('button', { name: 'shop_ro 계정 삭제' }).click()
  await expect(card.getByText('shop_ro')).toHaveCount(0)

  // Redis 계정: 만들기 → 수정 → 삭제 (Redis 메뉴)
  await page.getByRole('link', { name: 'Redis' }).click()
  await page.getByRole('button', { name: '+ Redis 계정' }).click()
  const rdlg = page.getByRole('dialog', { name: 'Redis 계정 만들기' })
  await rdlg.getByLabel('계정 이름').fill('sessions')
  await rdlg.getByLabel('읽기 전용').check()
  await rdlg.getByRole('button', { name: '만들기' }).click()
  await expect(page.getByRole('dialog', { name: 'sessions 접속 정보' }).getByText(/redis:\/\/sessions:••••••@127.0.0.1:6379\/0/)).toBeVisible()
  await page.getByRole('button', { name: '확인' }).click()
  const rlist = page.getByRole('table', { name: 'Redis 계정 목록' })
  await expect(rlist.getByText('sessions:*')).toBeVisible()
  await expect(rlist.getByText('읽기 전용')).toBeVisible()
  await rlist.getByRole('button', { name: '수정' }).click()
  const edit = page.getByRole('dialog', { name: 'Redis 계정 sessions' })
  await edit.getByLabel('쓸 수 있는 키').fill('sess:*')
  await edit.getByLabel('읽기 전용').uncheck()
  await edit.getByRole('button', { name: '저장' }).click()
  await expect(rlist.getByText('sess:*')).toBeVisible()
  await expect(rlist.getByText('읽기·쓰기')).toBeVisible()
  await rlist.getByRole('button', { name: 'Redis 계정 sessions 삭제' }).click()
  await expect(page.getByText('만든 Redis 계정이 없습니다.')).toBeVisible()

  // 백업 예약: 전체 · 매주 일요일 04:30 → 수정 → 지금 실행 → 끄기 → 삭제(파일 포함)
  await page.getByRole('link', { name: '백업' }).click()
  await page.getByRole('button', { name: '+ 예약 추가' }).click()
  const jd = page.getByRole('dialog', { name: '백업 예약 추가' })
  await expect(jd.getByLabel('백업할 대상')).toHaveValue('all')
  await jd.getByLabel('주기').selectOption('weekly')
  await jd.getByLabel('요일').selectOption('0')
  await jd.getByLabel('시', { exact: true }).selectOption('4')
  await jd.getByLabel('분', { exact: true }).selectOption('30')
  await expect(jd.getByText(/매주 일요일 04:30/)).toBeVisible()
  await jd.getByRole('button', { name: '추가' }).click()
  const jobs = page.getByRole('list', { name: '백업 예약 목록' })
  await expect(jobs.getByText('전체 백업')).toBeVisible()
  await expect(jobs.getByText(/매주 일요일 04:30 · 7개 보관/)).toBeVisible()

  await jobs.getByRole('button', { name: '수정' }).click()
  const je = page.getByRole('dialog', { name: '백업 예약 수정' })
  await je.getByLabel('이름 (선택)').fill('주간 전체 백업')
  await je.getByLabel('보관 개수').fill('4')
  await je.getByRole('button', { name: '저장' }).click()
  await expect(jobs.getByText('주간 전체 백업')).toBeVisible()
  await expect(jobs.getByText(/4개 보관/)).toBeVisible()

  await jobs.getByRole('button', { name: '지금 실행' }).click()
  await expect(page.getByText('백업 1개를 시작했습니다')).toBeVisible()
  await expect(page.getByText('예약 · 주간 전체 백업')).toBeVisible()

  await jobs.getByRole('button', { name: '끄기' }).click()
  await expect(jobs.getByText('꺼짐', { exact: true })).toBeVisible()
  await expect(jobs.getByText('예약 꺼짐')).toBeVisible()

  await jobs.getByRole('button', { name: '삭제' }).click()
  await expect(page.getByText('백업 예약이 없습니다.', { exact: false })).toBeVisible()
  await page.reload()
  await expect(page.getByText('예약 · 주간 전체 백업')).toHaveCount(0)
})

test('관리자 추가·수정·삭제', async ({ page }) => {
  const backend = createMockBackend()
  backend.db.loggedIn = true
  await backend.install(page)
  page.on('dialog', (d) => d.accept())
  await page.goto('/')
  await page.getByRole('link', { name: '관리자' }).click()
  const list = page.getByRole('table', { name: '관리자 목록' })
  await expect(list.getByText('admin@example.com')).toBeVisible()
  await expect(list.getByRole('button', { name: 'admin@example.com 삭제' })).toHaveCount(0)

  await page.getByRole('button', { name: '+ 관리자 추가' }).click()
  const dlg = page.getByRole('dialog', { name: '관리자 추가' })
  await dlg.getByLabel('이메일 (로그인 아이디)').fill('ops@example.com')
  await dlg.getByRole('button', { name: '생성' }).click()
  await expect(dlg.getByLabel('비밀번호')).toHaveValue(/.{16}/)
  await dlg.getByRole('button', { name: '추가' }).click()
  await expect(page.getByText('관리자 ops@example.com 을(를) 추가했습니다')).toBeVisible()

  await list
    .getByRole('row', { name: /ops@example.com/ })
    .getByRole('button', { name: '수정' })
    .click()
  const ed = page.getByRole('dialog', { name: '관리자 수정' })
  await ed.getByLabel('이메일 (로그인 아이디)').fill('devops@example.com')
  await ed.getByRole('button', { name: '저장' }).click()
  await expect(list.getByText('devops@example.com')).toBeVisible()

  await list.getByRole('button', { name: 'devops@example.com 삭제' }).click()
  await expect(list.getByText('devops@example.com')).toHaveCount(0)

  // 설정에는 시스템 정보가 없고 대시보드에 있음
  await page.getByRole('link', { name: '설정' }).click()
  await expect(page.getByRole('heading', { name: '내 비밀번호 변경' })).toBeVisible()
  await page.getByRole('link', { name: '대시보드' }).click()
  await expect(page.getByText('Intel Xeon E5-2690', { exact: false })).toBeVisible()
})

test('테마 낮·밤 전환과 영어 전환', async ({ page }) => {
  const backend = createMockBackend()
  backend.db.loggedIn = true
  await backend.install(page)
  await page.goto('/settings')
  const html = page.locator('html')
  await page.getByRole('radiogroup', { name: '테마' }).first().getByRole('radio', { name: '낮' }).click()
  await expect(html).toHaveAttribute('data-theme', 'light')
  await page.getByRole('radiogroup', { name: '테마' }).first().getByRole('radio', { name: '밤' }).click()
  await expect(html).toHaveAttribute('data-theme', 'dark')

  await page.getByRole('combobox', { name: '언어' }).selectOption('en')
  await expect(page.getByRole('link', { name: 'Dashboard' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible()
  await expect(html).toHaveAttribute('lang', 'en')
  // 새로 고쳐도 유지
  await page.reload()
  await expect(page.getByRole('link', { name: 'Sites' })).toBeVisible()
  await expect(html).toHaveAttribute('data-theme', 'dark')
  await page.getByRole('combobox', { name: 'Language' }).selectOption('ko')
  await expect(page.getByRole('link', { name: '사이트' })).toBeVisible()
})

test('버전 표시 (지금 · GitHub) 와 런타임 버전 설치·기본 변경·앱별 선택', async ({ page }) => {
  const backend = seededBackend()
  await backend.install(page)
  page.on('dialog', (d) => d.accept())
  await page.goto('/')
  await expect(page.getByTestId('update-badge')).toHaveText('새 버전 v0.5.0')
  await expect(page.getByTestId('version-current')).toHaveText('v0.4.0')
  await expect(page.getByTestId('version-latest')).toHaveText('v0.5.0')
  await expect(page.getByText('sudo rpanel-update')).toBeVisible()

  await page.getByRole('link', { name: '런타임' }).click()
  await expect(page.getByRole('heading', { name: '런타임' })).toBeVisible()
  // Python: 3.13 고르기 → 설치하고 기본으로 변경 → 설치 끝나면 기본
  const py = page.getByRole('group', { name: 'Python 버전 고르기' })
  await py.getByLabel('Python 버전').selectOption('3.13')
  await py.getByRole('button', { name: '설치하고 기본으로 변경' }).click()
  await expect(page.getByText('3.13 설치를 시작했습니다. 끝나면 기본 버전이 됩니다')).toBeVisible()
  const pyRows = page.getByRole('table', { name: 'Python 설치된 버전' })
  const row313 = pyRows.getByRole('row', { name: /3\.13/ })
  await expect(row313.getByText('사용 가능')).toBeVisible({ timeout: 10000 })
  await expect(row313.getByText('기본', { exact: true })).toBeVisible()
  await expect(row313.getByText('3.13.9')).toBeVisible()
  // 다시 서버 기본으로
  await pyRows
    .getByRole('row', { name: /서버 기본/ })
    .getByRole('button', { name: '기본으로' })
    .click()
  await expect(pyRows.getByRole('row', { name: /서버 기본/ }).getByText('기본', { exact: true })).toBeVisible()

  // Rust: 직접 입력한 버전을 설치만 → 삭제
  const rs = page.getByRole('group', { name: 'Rust 버전 고르기' })
  await rs.getByLabel('Rust 버전', { exact: true }).selectOption('__custom')
  await rs.getByLabel('Rust 버전 직접 입력').fill('1.85')
  await rs.getByRole('button', { name: '설치만' }).click()
  const rsRows = page.getByRole('table', { name: 'Rust 설치된 버전' })
  await expect(rsRows.getByRole('row', { name: /1\.85/ }).getByText('사용 가능')).toBeVisible({ timeout: 10000 })
  await rsRows.getByRole('button', { name: 'Rust 1.85 삭제' }).click()
  await expect(rsRows.getByText('1.85')).toHaveCount(0)

  // 앱별 버전: Rust Git 앱 → 설정 탭에서 stable 대신 1.85 는 없음, 기본 따르기 / stable 고르기
  await page.goto('/apps/2')
  await page.getByRole('tab', { name: '설정' }).click()
  const pick = page.getByLabel('앱 런타임 버전')
  await expect(pick.locator('option').first()).toHaveText('기본 버전 따르기 (최신 안정판 (stable))')
  await pick.selectOption('stable')
  await page.getByRole('button', { name: '저장' }).first().click()
  await expect(page.getByText('버전을 저장했습니다. 다음 배포·적용부터 쓰입니다')).toBeVisible()
  expect(backend.db.apps.find((a) => a.id === 2).runtime_version).toBe('stable')
})

test('? 도움말 · 로그 메뉴 (종류별)', async ({ page }) => {
  const backend = seededBackend()
  backend.db.deployments.push({
    id: 77,
    app_id: 2,
    status: 'success',
    commit_sha: 'abc12345',
    triggered_by: 'manual',
    created_at: new Date().toISOString(),
    finished_at: new Date().toISOString(),
  })
  await backend.install(page)
  await page.goto('/projects')
  // 설명글은 ? 를 눌러야 보임
  const helpText = '서비스 단위로 앱(백엔드·프런트)을 묶어 관리합니다'
  await expect(page.getByText(helpText)).toHaveCount(0)
  await page.getByRole('button', { name: '프로젝트 도움말' }).click()
  await expect(page.getByRole('tooltip')).toContainText(helpText)
  await page.keyboard.press('Escape')
  await expect(page.getByRole('tooltip')).toHaveCount(0)

  await page.getByRole('link', { name: '로그' }).click()
  await expect(page.getByRole('heading', { name: '로그' })).toBeVisible()
  await expect(page.getByRole('tab', { name: '활동 기록' })).toHaveAttribute('aria-selected', 'true')
  await page.getByRole('tab', { name: '배포 기록' }).click()
  await expect(page.getByRole('list', { name: '배포 기록' }).getByText('Beripick API')).toBeVisible()
  await page.getByRole('tab', { name: '서버 로그' }).click()
  await expect(page.getByText('starting rpanel 0.4.0')).toBeVisible()
  await page.getByRole('button', { name: 'SSH 접속' }).click()
  await expect(page.getByText('Failed password for root')).toBeVisible()
  await page.getByLabel('로그 검색').fill('accepted')
  await expect(page.getByText('Failed password for root')).toHaveCount(0)
  await expect(page.getByText('Accepted publickey for shop')).toBeVisible()
  await expect(page).toHaveURL(/tab=system/)
  await page.getByRole('tab', { name: '사이트 접속 로그' }).click()
  await expect(page.getByLabel('사이트 선택')).toHaveValue('2')
  await expect(page.getByTestId('stat-2xx')).toHaveText('3')
})

test('알림: 종 아이콘 · 목록 · 설정 · 시험 알림, 메뉴 순서, 도움말 위치', async ({ page }) => {
  const backend = seededBackend()
  backend.db.notifications.push(
    {
      id: 501,
      kind: 'deploy_failed',
      level: 'error',
      title: '배포 실패: {app} #{id}',
      params: { app: 'Beripick API', id: 9 },
      body: 'cargo build 실패',
      link: '/apps/2',
      read_at: null,
      created_at: new Date().toISOString(),
    },
    {
      id: 502,
      kind: 'disk_high',
      level: 'warning',
      title: '디스크 사용량 {pct}% ({mount})',
      params: { pct: 93, mount: '/' },
      body: null,
      link: '/',
      read_at: null,
      created_at: new Date().toISOString(),
    },
  )
  await backend.install(page)
  page.on('dialog', (d) => d.accept())
  await page.goto('/')

  // 메뉴 순서: … 로그 · 런타임 · 설정 · 관리자
  await expect(page.locator('aside nav a')).toHaveText(['대시보드', '사이트', '프로젝트', 'MariaDB', 'Redis', '백업', '로그', '런타임', '설정', '관리자'])

  // 종 아이콘: 안 읽은 2개 → 펼치면 번역된 제목 → 누르면 링크로 이동하고 읽음
  await expect(page.getByTestId('notification-count').first()).toHaveText('2')
  await page.getByRole('button', { name: '알림 2개 안 읽음' }).first().click()
  const pop = page.getByRole('dialog', { name: '알림' })
  await expect(pop.getByText('디스크 사용량 93% (/)')).toBeVisible()
  await pop.getByText('배포 실패: Beripick API #9').click()
  await expect(page).toHaveURL(/\/apps\/2$/)
  await expect(page.getByTestId('notification-count').first()).toHaveText('1')

  // 설정: 잘못된 주소 거부 → 고친 뒤 저장 → 시험 알림
  await page.goto('/notifications?tab=settings')
  await page.getByLabel('배포·적용 성공').check()
  await page.getByLabel('Discord 웹훅 주소').fill('https://evil.example/hook')
  await page.getByRole('button', { name: '저장', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('discord.com/api/webhooks')
  await page.getByLabel('Discord 웹훅 주소').fill('https://discord.com/api/webhooks/1/abc')
  await page.getByRole('button', { name: '저장', exact: true }).click()
  await expect(page.getByText('알림 설정을 저장했습니다')).toBeVisible()
  expect(backend.db.notifySettings.events.deploy_success).toBe(true)
  await page.getByRole('button', { name: '시험 알림 보내기' }).click()
  await expect(page.getByRole('list', { name: '시험 결과' })).toContainText('discord: 성공')

  // 목록: 모두 읽음 → 모두 지우기
  await page.getByRole('tab', { name: '알림 목록' }).click()
  const list = page.getByRole('list', { name: '알림 목록' })
  await expect(list.getByText('시험 알림입니다')).toBeVisible()
  await page.getByRole('button', { name: '모두 읽음' }).click()
  await expect(page.getByTestId('notification-count')).toHaveCount(0)
  await page.getByRole('button', { name: '모두 지우기' }).click()
  await expect(list.getByText('알림이 없습니다')).toBeVisible()

  // 빈 화면의 도움말은 위쪽 버튼 옆 ? 에
  await page.goto('/projects/1')
  backend.db.apps = []
  await page.reload()
  await expect(page.getByText('앱이 없습니다')).toBeVisible()
  await page.getByRole('button', { name: 'Beripick 도움말' }).click()
  await expect(page.getByRole('tooltip')).toContainText('Git 저장소에서 빌드하는 앱입니다')
})

test('https Git 주소 → SSH 주소로 바꾸기 안내', async ({ page }) => {
  const backend = seededBackend()
  backend.db.apps.find((a) => a.id === 2).git_url = 'https://github.com/sinji0117/bandiuri'
  await backend.install(page)
  await page.goto('/apps/2')
  await page.getByRole('tab', { name: 'Git · 웹훅' }).click()
  await expect(page.getByTestId('https-warning')).toContainText('https://github.com/sinji0117/bandiuri')
  await page.getByRole('button', { name: 'git@github.com:sinji0117/bandiuri.git 로 바꾸기' }).click()
  await expect(page.getByText('Git 주소를 git@github.com:sinji0117/bandiuri.git 로 바꿨습니다')).toBeVisible()
  await expect(page.getByTestId('https-warning')).toHaveCount(0)
  expect(backend.db.apps.find((a) => a.id === 2).git_url).toBe('git@github.com:sinji0117/bandiuri.git')
})
