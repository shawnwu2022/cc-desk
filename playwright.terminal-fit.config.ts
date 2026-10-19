import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './tests/visual', testMatch: 'terminal-fit.spec.ts',
  workers: 1, retries: 0, timeout: 60_000,
  outputDir: 'test-results/terminal-fit', reporter: [['list']],
  globalSetup: './tests/visual/terminal-fit/server.ts',
  use: {
    baseURL: 'http://127.0.0.1:4175', browserName: 'chromium',
    launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE },
    viewport: { width: 1024, height: 640 }, trace: 'retain-on-failure',
  },
})
