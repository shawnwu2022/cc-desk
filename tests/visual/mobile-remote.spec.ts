import { expect, test } from '@playwright/test'

for (const width of [320, 390]) {
  for (const entry of ['/browser-registration.html', '/browser-remote.html']) {
    test(`mobile remote controls render without overflow at ${width}px: ${entry}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 700 })
      await page.goto(entry)
      const main = page.getByRole('main')
      await expect(main.getByRole('status')).toBeVisible()
      const controls = main.getByRole('button')
      const registration = entry.includes('registration')
      await expect(controls).toHaveCount(registration ? 3 : 2)
      for (const button of await controls.all()) {
        expect((await button.boundingBox())?.height).toBeGreaterThanOrEqual(48)
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width)
      if (registration) {
        await expect(main.getByRole('button', { name: 'Start registration' })).toBeDisabled()
        await expect(main.getByRole('link', { name: 'Open terminal' })).toHaveAttribute('href', './browser-remote.html')
      }
    })
  }
}
