import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './tests/production',
  testMatch: '**/*.spec.ts',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 15000,
  outputDir: 'test-results/production-startup',
  preserveOutput: 'always',
  reporter: [['list']],
  use: {
    baseURL: 'http://127.0.0.1:4177',
    browserName: 'chromium',
    launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE },
    trace: 'retain-on-failure',
    screenshot: 'on',
  },
  webServer: {
    command: 'node tests/production/serve.mjs',
    url: 'http://127.0.0.1:4177',
    reuseExistingServer: false,
    timeout: 60000,
  },
})
