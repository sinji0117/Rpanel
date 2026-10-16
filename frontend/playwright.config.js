import { defineConfig, devices } from '@playwright/test'

// E2E는 백엔드 없이 동작합니다: e2e/mockApi.js 가 /api 와 WebSocket 을 가로채 응답합니다.
// 실제 백엔드(dry-run)로 돌리려면: E2E_BASE_URL=http://127.0.0.1:8090 E2E_REAL=1 npx playwright test
const executablePath = process.env.CHROME_PATH || undefined

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  fullyParallel: true,
  // CI: 실패 내용을 GitHub 실행 요약(annotations)에도 표시
  reporter: process.env.CI ? [['line'], ['github']] : 'list',
  use: {
    baseURL: process.env.E2E_BASE_URL || 'http://127.0.0.1:4173',
    trace: 'retain-on-failure',
    locale: 'ko-KR',
    launchOptions: executablePath ? { executablePath } : {},
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: process.env.E2E_BASE_URL
    ? undefined
    : {
        command: 'npm run build && npm run preview',
        url: 'http://127.0.0.1:4173',
        reuseExistingServer: !process.env.CI,
        timeout: 120_000,
      },
})
