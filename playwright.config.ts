import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './tests/visual',
  testMatch: '**/*.spec.ts',
  fullyParallel: true,
  workers: 4,
  retries: 0,
  timeout: 30_000,
  updateSnapshots: 'none',
  snapshotPathTemplate: '{testDir}/__screenshots__/{arg}{ext}',
  outputDir: 'test-results/visual',
  reporter: [['list']],
  expect: { timeout: 5_000, toHaveScreenshot: { animations: 'disabled', caret: 'hide', scale: 'device', maxDiffPixels: 0 } },
  use: {
    baseURL: 'http://127.0.0.1:4174',
    browserName: 'chromium',
    launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE },
    locale: 'en-US', timezoneId: 'UTC', reducedMotion: 'reduce',
    userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'npm run dev -- --mode visual --host 127.0.0.1 --port 4174',
    env: { CC_DESK_VISUAL_FIXTURE: '1' },
    url: 'http://127.0.0.1:4174/__visual__/',
    reuseExistingServer: false,
  },
})
