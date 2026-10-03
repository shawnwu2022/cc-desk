import { test, expect, type Page } from '@playwright/test'
import wire from '../fixtures/version-manager-wire.json'
import { captureFixtureEvidence } from './fixtureEvidence'

// These are synthetic bridge states, not Windows installation evidence. Each
// passing case retains unapproved PNGs for explicit human pixel inspection.
const surfaces = [
  { name: 'native-en-light', width: 760, height: 600, locale: 'en', gui: 'light', dpr: 1 },
  { name: 'native-zh-dark', width: 760, height: 600, locale: 'zh', gui: 'dark', dpr: 1.5 },
  { name: 'minimum-en-light', width: 640, height: 480, locale: 'en', gui: 'light', dpr: 1.25 },
  { name: 'minimum-zh-dark', width: 640, height: 480, locale: 'zh', gui: 'dark', dpr: 1.5 },
] as const
const errors = new WeakMap<Page, string[]>()
test.beforeEach(async ({ page }) => {
  const messages: string[] = []; errors.set(page, messages)
  page.on('pageerror', error => messages.push(error.message))
  await page.route('**/*', route => new URL(route.request().url()).origin === 'http://127.0.0.1:4174' ? route.continue() : route.abort())
})
test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([])
  await expect(page.locator('[data-blocked-host-calls]')).toHaveAttribute('data-blocked-host-calls', '0')
  expect(await page.locator('.xterm').count()).toBe(0)
})

async function openManager(page: Page, query: Record<string, string>) {
  await page.clock.install({ time: new Date('2026-10-02T12:00:00Z') })
  await page.goto(`/__visual__/version-manager/?${new URLSearchParams(query)}`)
  await expect(page.locator('[data-visual-ready]')).toHaveAttribute('data-visual-ready', 'true')
  await expect(page.locator('[data-phase]')).toHaveAttribute('data-phase', query.phase)
  await page.evaluate(() => document.fonts.ready)
  // Freeze polling only after the real component has loaded. Tests advance it
  // explicitly; elapsed time can never serve as an installation-success proof.
  await page.clock.pauseAt(new Date('2026-10-02T13:00:00Z'))
}

async function assertGeometry(page: Page) {
  const actual = await page.evaluate(() => {
    const footer = document.querySelector('.manager-actions')!.getBoundingClientRect()
    const content = document.querySelector('.manager-content')!.getBoundingClientRect()
    const boxes = [...document.querySelectorAll('.manager-actions button')].map(button => button.getBoundingClientRect())
    return {
      horizontalOverflow: document.documentElement.scrollWidth > innerWidth,
      footerVisible: footer.top >= 0 && footer.bottom <= innerHeight,
      contentClearOfFooter: content.bottom <= footer.top + 1 && content.height > 0,
      controlsFit: boxes.every(box => box.left >= 0 && box.right <= innerWidth && box.height >= 32 && box.top >= footer.top && box.bottom <= footer.bottom),
      controlsOverlap: boxes.some((box, index) => boxes.slice(index + 1).some(other =>
        Math.min(box.right, other.right) > Math.max(box.left, other.left)
        && Math.min(box.bottom, other.bottom) > Math.max(box.top, other.top))),
    }
  })
  expect(actual).toEqual({ horizontalOverflow: false, footerVisible: true, contentClearOfFooter: true, controlsFit: true, controlsOverlap: false })
}

for (const surface of surfaces) {
  test.describe(`version manager ${surface.name}`, () => {
    test.use({ viewport: { width: surface.width, height: surface.height }, deviceScaleFactor: surface.dpr,
      locale: surface.locale === 'zh' ? 'zh-CN' : 'en-US', colorScheme: surface.gui })

    for (const status of Object.values(wire)) {
      test(`phase ${status.phase}`, async ({ page }, testInfo) => {
        await openManager(page, { phase: status.phase, locale: surface.locale })
        await expect(page.locator('html')).toHaveAttribute('lang', surface.locale === 'zh' ? 'zh-CN' : 'en')
        await expect(page.locator('html')).toHaveAttribute('data-theme', surface.gui)
        await expect(page.locator('[data-manager-confirm]')).toHaveCount(status.allowedActions.includes('confirm-historical-version') ? 1 : 0)
        await expect(page.locator('[data-manager-return]')).toHaveCount(status.allowedActions.includes('return-to-previous') ? 1 : 0)
        await expect(page.locator('[data-manager-cancel]')).toHaveCount(0)
        await expect(page.locator('[data-manager-confirms]')).toHaveAttribute('data-manager-confirms', '0')
        await expect(page.locator('[data-manager-returns]')).toHaveAttribute('data-manager-returns', '0')
        await assertGeometry(page)
        await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-${status.phase}-unapproved`)
        if (status.phase === 'installed-unconfirmed' || status.phase === 'recovery-required') {
          await page.locator('[data-manager-boundary]').scrollIntoViewIfNeeded()
          await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-${status.phase}-boundary-unapproved`)
        }
      })
    }

    test('explicit first-launch confirmation and keyboard review', async ({ page }, testInfo) => {
      await openManager(page, { phase: 'installed-unconfirmed', locale: surface.locale })
      const opener = page.locator('[data-manager-confirm]')
      await opener.click()
      await expect(page.locator('[role="dialog"]')).toBeVisible()
      await expect(page.locator('[data-manager-back]')).toBeFocused()
      await page.keyboard.press('Tab')
      await expect(page.locator('[data-manager-submit]')).toBeFocused()
      await page.keyboard.press('Tab')
      await expect(page.locator('[data-manager-back]')).toBeFocused()
      await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-confirm-review-unapproved`)
      await page.keyboard.press('Escape')
      await expect(page.locator('[role="dialog"]')).toHaveCount(0)
      await expect(opener).toBeFocused()
      await expect(page.locator('[data-manager-confirms]')).toHaveAttribute('data-manager-confirms', '0')
      await opener.click()
      await page.locator('[data-manager-submit]').click()
      await expect(page.locator('[data-phase]')).toHaveAttribute('data-phase', 'historical-active')
      await expect(page.locator('[data-manager-confirms]')).toHaveAttribute('data-manager-confirms', '1')
      await expect(page.locator('[data-manager-confirm]')).toHaveCount(0)
      await expect(page.locator('[data-manager-return]')).toBeEnabled()
      await assertGeometry(page)
    })

    test('return review preserves safe focus and reports confirmed restoration', async ({ page }, testInfo) => {
      await openManager(page, { phase: 'historical-active', locale: surface.locale })
      await page.locator('[data-manager-return]').click()
      await expect(page.locator('[data-manager-back]')).toBeFocused()
      await expect(page.locator('[data-manager-submit]')).toHaveAttribute('data-danger', 'true')
      await expect(page.locator('[role="dialog"]')).toContainText('0.18.0')
      await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-return-review-unapproved`)
      await page.locator('[data-manager-submit]').click()
      await expect(page.locator('[data-phase]')).toHaveAttribute('data-phase', 'returning')
      await expect(page.locator('[data-manager-phase]')).toBeFocused()
      await expect(page.locator('[data-manager-returns]')).toHaveAttribute('data-manager-returns', '1')
      await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-returning-request-unapproved`)
      await page.clock.runFor(1500)
      await expect(page.locator('[data-phase]')).toHaveAttribute('data-phase', 'restored')
      await expect(page.locator('[data-manager-return]')).toHaveCount(0)
      await assertGeometry(page)
      await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-restored-receipt-unapproved`)
    })

    test('unknown return outcome permits inspection without replay', async ({ page }, testInfo) => {
      await openManager(page, { phase: 'installed-unconfirmed', locale: surface.locale, outcome: 'unknown' })
      await page.locator('[data-manager-return]').click()
      await page.locator('[data-manager-submit]').click()
      await expect(page.locator('[data-manager-error]')).toBeVisible()
      await expect(page.locator('[data-phase]')).toHaveAttribute('data-phase', 'unknown')
      await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-outcome-unknown-unapproved`)
      await page.locator('[data-manager-refresh]').click()
      await expect(page.locator('[data-manager-uncertain]')).toBeVisible()
      await expect(page.locator('[data-manager-return]')).toBeDisabled()
      await expect(page.locator('[data-manager-confirm]')).toBeDisabled()
      await page.clock.runFor(6000)
      await expect(page.locator('[data-manager-returns]')).toHaveAttribute('data-manager-returns', '1')
      await assertGeometry(page)
      await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-unknown-no-replay-unapproved`)
    })

    test('backend can withhold every mutation at an installed state', async ({ page }, testInfo) => {
      await openManager(page, { phase: 'installed-unconfirmed', locale: surface.locale, actions: 'refresh-only' })
      await expect(page.locator('[data-manager-confirm]')).toHaveCount(0)
      await expect(page.locator('[data-manager-return]')).toHaveCount(0)
      await expect(page.locator('[data-manager-refresh]')).toBeEnabled()
      await assertGeometry(page)
      await captureFixtureEvidence(page, testInfo, `manager-${surface.name}-actions-withheld-unapproved`)
    })
  })
}
