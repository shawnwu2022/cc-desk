import type { Page, TestInfo } from '@playwright/test'

/** Capture unapproved review evidence independently of snapshot baselines. */
export async function captureFixtureEvidence(
  page: Pick<Page, 'screenshot'>,
  testInfo: Pick<TestInfo, 'outputPath' | 'attach'>,
  name: string,
) {
  const path = testInfo.outputPath(`${name}.png`)
  await page.screenshot({ path })
  await testInfo.attach(name, { path, contentType: 'image/png' })
}
