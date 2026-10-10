import { test, expect, type Page } from '@playwright/test'
import { expandFixtureProjects, clearFixtureSetupFocus, openFixtureSessionMenu } from './fixtureActions'
import { captureFixtureEvidence } from './fixtureEvidence'

const browserErrors = new WeakMap<Page, string[]>()
test.beforeEach(async ({ page }) => {
  const errors: string[] = []; browserErrors.set(page, errors)
  page.on('pageerror', error => errors.push(error.message))
  await page.route('**/*', route => new URL(route.request().url()).origin === 'http://127.0.0.1:4174' ? route.continue() : route.abort())
})
test.afterEach(async ({ page }) => {
  expect(browserErrors.get(page)).toEqual([])
  await expect(page.locator('[data-blocked-host-calls]')).toHaveAttribute('data-blocked-host-calls', '0')
})

const snapshots = [
  { name: 'workspace-empty-1024-zh', scenario: 'empty', width: 1024, height: 640, locale: 'zh' },
  { name: 'workspace-mixed-1366-zh', scenario: 'mixed', width: 1366, height: 768, locale: 'zh' },
  { name: 'workspace-hover-action-1366-en', scenario: 'hover', width: 1366, height: 768, locale: 'en' },
  { name: 'workspace-resources-overlay-1024', scenario: 'resources', width: 1024, height: 640, locale: 'en' },
  { name: 'projects-150-percent', scenario: 'projects', width: 1280, height: 720, locale: 'zh', dpr: 1.5, density: 'compact' },
  { name: 'new-session-dialog', scenario: 'new-session', width: 1024, height: 640, locale: 'en' },
  { name: 'archived-sessions', scenario: 'archived', width: 1366, height: 768, locale: 'zh', gui: 'dark' },
  { name: 'settings-terminal-light-gui-dark-terminal', scenario: 'terminal-settings', width: 1440, height: 900, locale: 'en' },
  { name: 'settings-launch-configurations', scenario: 'launch-configurations', width: 1280, height: 720, locale: 'zh', gui: 'dark' },
  { name: 'confirm-stop-and-archive', scenario: 'confirmation', width: 1024, height: 640, locale: 'en' },
  { name: 'workspace-menu-1024-en', scenario: 'menu', width: 1024, height: 640, locale: 'en' },
  { name: 'workspace-dark-gui-light-terminal', scenario: 'mixed', width: 1280, height: 720, locale: 'en', gui: 'dark', terminal: 'cc-box-light' },
  { name: 'tooltip-transformed-1024', scenario: 'tooltip', width: 1024, height: 640, locale: 'en' },
] as const

async function openFixture(page: Page, options: Record<string, string | number>) {
  // Keep the fixture date reproducible while allowing Vue's event timestamps to advance.
  // A frozen Date.now drops native key events after the tree's capture listener.
  await page.clock.setSystemTime(new Date('2026-09-28T12:00:00Z'))
  await page.goto(`/__visual__/?${new URLSearchParams(Object.entries(options).map(([key, value]) => [key, String(value)]))}`)
  await expect(page.locator('[data-visual-ready]')).toHaveAttribute('data-visual-ready', 'true')
  await page.evaluate(() => document.fonts.ready)
  if (['mixed', 'hover', 'menu', 'close-state', 'native-notice', 'session-status'].includes(String(options.scenario))) {
    await expandFixtureProjects(page)
  }
}

// Unapproved status/explicit-launch pixels stay separate from the immutable
// historical baselines. Synthetic signals do not certify live CLI semantics.
for (const gui of ['light', 'dark']) for (const locale of ['en', 'zh']) {
  test(`session glyph and explicit launch evidence ${gui} ${locale}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1024, height: 1000 })
    await openFixture(page, { scenario: 'session-status', gui, locale })
    const history = page.locator('[data-session-row="visual-status-16"]')
    const launch = history.locator('[data-session-launch]')
    await page.mouse.move(1020, 996)
    await expect(launch).toHaveCSS('opacity', '1')
    await expect(launch).toHaveCSS('pointer-events', 'auto')
    await expect(history.locator('.session-time')).toHaveCSS('opacity', '1')
    await expect(launch.getByRole('button')).toBeInViewport({ ratio: 1 })
    const time = await history.locator('.session-time').boundingBox(), action = await launch.boundingBox()
    expect(time && action && time.x + time.width <= action.x).toBeTruthy()
    await history.hover()
    await expect(history.locator('.session-time')).toHaveCSS('opacity', '1')
    // End the title hover before testing Escape on the independent launch trigger.
    await page.mouse.move(1020, 996)
    await expect(page.getByRole('tooltip')).toHaveCount(0)
    await launch.getByRole('button').focus()
    await expect(launch.getByRole('button')).toBeFocused()
    await expect(page.getByRole('tooltip')).toHaveText(locale === 'en' ? 'Resume session' : '恢复会话')
    await page.keyboard.press('Escape')
    await expect(page.getByRole('tooltip')).toHaveCount(0)
    await clearFixtureSetupFocus(page)
    await page.mouse.move(1020, 996)
    await captureFixtureEvidence(page, testInfo, `session-glyphs-${gui}-${locale}-unapproved`)
    const working = page.locator('[data-session-row="visual-status-2"] .session-status-icon')
    await working.hover()
    await expect(page.getByRole('tooltip')).toHaveText(locale === 'en' ? 'Thinking' : '思考中')
    await page.mouse.move(1020, 996)
    await expect(page.getByRole('tooltip')).toHaveCount(0)
  })
}

// These synthetic receipt captures are unapproved evidence, separate from the
// thirteen historical pixel baselines and actual authenticated CLI acceptance.
for (const locale of ['en', 'zh']) for (const notice of ['unread', 'read']) {
  test(`native receipt marker ${locale} ${notice}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1024, height: 640 })
    await openFixture(page, { scenario: 'native-notice', locale, notice })
    const row = page.locator('[data-session-row="visual-native-notice"]')
    const marker = row.locator('[data-native-observation-notice]')
    const label = notice === 'unread'
      ? locale === 'en' ? 'Reply-end notice received (unread); current activity unverified' : '收到回复结束通知（未读）；当前活动尚未验证'
      : locale === 'en' ? 'Recent notice: reply-end event; current activity unverified' : '最近收到：回复结束事件；当前活动尚未验证'
    await expect(marker).toHaveAttribute('aria-label', label)
    await expect(marker).toHaveAttribute('data-unread', String(notice === 'unread'))
    await expect(row.locator('.session-status-icon')).toHaveAttribute('aria-label', locale === 'en' ? 'Activity unknown' : '活动未知')
    await expect(row.locator('.session-primary-action button')).toHaveAttribute('aria-label', locale === 'en' ? 'Close' : '关闭')
    await marker.focus()
    await expect(marker).toBeFocused()
    await expect(marker).toBeInViewport({ ratio: 1 })
    await expect(page.getByRole('tooltip')).toHaveText(label)
    await expect(page.getByRole('tooltip')).toBeInViewport({ ratio: 1 })
    await captureFixtureEvidence(page, testInfo, `native-receipt-${locale}-${notice}-unapproved`)
    await page.keyboard.press('Escape')
    await expect(page.getByRole('tooltip')).toHaveCount(0)
    await expect(marker).toBeFocused()
    await page.keyboard.press('Enter')
    await expect(row.locator('input')).toHaveCount(0)
    await expect(page.locator('[role="dialog"]')).toHaveCount(0)
    await expect(row.locator('.session-status-icon')).toHaveAttribute('aria-label', locale === 'en' ? 'Activity unknown' : '活动未知')
  })
}

for (const sample of snapshots) {
  test.describe(sample.name, () => {
    test.use({ viewport: { width: sample.width, height: sample.height }, deviceScaleFactor: 'dpr' in sample ? sample.dpr : 1 })
    test('snapshot', async ({ page }) => {
      await openFixture(page, { scenario: sample.scenario, locale: sample.locale, gui: 'gui' in sample ? sample.gui : 'light', density: 'density' in sample ? sample.density : 'standard', terminal: 'terminal' in sample ? sample.terminal : 'cc-box-dark' })
      if (['empty', 'mixed', 'hover', 'projects', 'terminal-settings', 'launch-configurations'].includes(sample.scenario)) {
        await clearFixtureSetupFocus(page)
        await expect(page.locator('.shell-main')).not.toBeFocused()
      }
      if (sample.scenario === 'menu') await openFixtureSessionMenu(page)
      if (sample.scenario === 'tooltip') await page.locator('[data-tooltip-trigger]').focus()
      // Use the main area's empty lower-right gutter, never a window control.
      await page.mouse.move(sample.width - 4, sample.height - 4)
      if (sample.scenario !== 'tooltip') await expect(page.getByRole('tooltip')).toHaveCount(0)
      if (sample.scenario === 'hover') await page.locator('[data-session-row="visual-session-0"]').hover()
      await expect(page).toHaveScreenshot(`${sample.name}.png`)
    })
  })
}


// Close-only feedback has separate unapproved evidence; historical baselines stay untouched.
for (const locale of ['en', 'zh']) {
  test(`close-only confirmation evidence ${locale}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1024, height: 640 })
    await openFixture(page, { scenario: 'close-confirmation', locale })
    await expect(page.locator('[data-session-confirm]')).toHaveText(locale === 'en' ? 'Close' : '关闭')
    await captureFixtureEvidence(page, testInfo, `close-only-confirmation-${locale}-unapproved`)
  })
}

// Opened-state evidence is separate from approved snapshots and never invokes Close.
// Each fabricated row uses real hover/focus CSS and all three production menu entry points.
for (const [variantIndex, variant] of ['native-claude', 'native-codex', 'legacy-claude'].entries()) {
  for (const [stateIndex, state] of ['starting', 'running', 'needs-user', 'unknown', 'stopped', 'failed'].entries()) {
    const locale = (variantIndex + stateIndex) % 2 ? 'zh' : 'en'
    test.describe(`close-state evidence ${variant} ${state} ${locale}`, () => {
      test.use({ viewport: { width: 1024, height: 640 }, deviceScaleFactor: 1 })
      test('captures row Close and deduplicated menus', async ({ page }, testInfo) => {
        await openFixture(page, { scenario: 'close-state', runtime: variant, state, locale, gui: locale === 'zh' ? 'dark' : 'light' })
        await expect(page.locator('[data-session-row]')).toHaveCount(1)
        const row = page.locator(`[data-session-row="visual-close-${variant}-${state}"]`)
        const primary = row.locator('.session-primary-action')
        const close = primary.getByRole('button', { name: locale === 'en' ? 'Close' : '关闭', exact: true })
        const overflow = row.locator('.session-overflow-trigger button')
        const evidenceName = `close-state-${variant}-${state}-${locale}`
        await expect(primary.getByRole('button')).toHaveCount(1)
        await expect(close).toBeEnabled()
        await expect(close.locator('svg path')).toHaveAttribute('d', 'm6 6 12 12M18 6 6 18')
        await expect(row.locator('.session-status-icon')).toHaveClass(`session-status-icon session-status-icon--${state === 'unknown' ? 'confirming' : state === 'needs-user' ? 'running' : state}`)
        await expect(row.locator('.cli-app-icon')).toHaveAttribute('aria-label', variant === 'native-codex' ? 'Codex CLI' : 'Claude Code')

        // Opacity is explicit: Playwright visibility alone would also accept an invisible button.
        await page.mouse.move(1020, 636)
        await expect(primary).toHaveCSS('opacity', '0')
        await row.hover({ position: { x: 4, y: 4 } })
        await expect(primary).toHaveCSS('opacity', '1')
        await expect(primary).toHaveCSS('pointer-events', 'auto')
        await expect(close).toBeInViewport({ ratio: 1 })
        await expect(page.getByRole('tooltip')).toHaveCount(0)
        await captureFixtureEvidence(page, testInfo, `${evidenceName}-hover-unapproved`)

        await page.mouse.move(1020, 636)
        await expect(primary).toHaveCSS('opacity', '0')
        await overflow.focus()
        await page.keyboard.press('Shift+Tab')
        if (state === 'stopped' || state === 'failed') {
          const archive = row.locator('[data-session-archive] button')
          await expect(archive).toBeFocused()
          await expect(archive).toBeInViewport({ ratio: 1 })
          await expect(page.getByRole('tooltip')).toHaveText(locale === 'en' ? 'Archive' : '归档')
          await page.keyboard.press('Escape')
          await page.keyboard.press('Shift+Tab')
        }
        await expect(close).toBeFocused()
        await expect(page.getByRole('tooltip')).toHaveText(locale === 'en' ? 'Close' : '关闭')
        await page.keyboard.press('Escape')
        await expect(page.getByRole('tooltip')).toHaveCount(0)
        await expect(close).toBeFocused()
        await expect(close).toHaveCSS('outline-style', 'solid')
        await expect(primary).toHaveCSS('opacity', '1')
        await expect(primary).toHaveCSS('pointer-events', 'auto')
        await captureFixtureEvidence(page, testInfo, `${evidenceName}-focus-unapproved`)

        for (const entry of ['overflow', 'context', 'keyboard']) {
          await row.focus()
          if (entry === 'overflow') await overflow.click()
          else if (entry === 'context') await row.click({ button: 'right', position: { x: 4, y: 4 } })
          else await page.keyboard.press('Shift+F10')
          const menu = page.getByRole('menu')
          await expect(menu).toHaveCount(1)
          await expect(menu).toBeVisible()
          await expect(menu).toBeInViewport({ ratio: 1 })
          await expect(menu.locator('[data-item-id="close"], [data-item-id="stop"]')).toHaveCount(0)
          await expect(menu.getByRole('menuitem', { name: /^(Close|Stop|Stop and archive|关闭|停止|停止并归档)$/ })).toHaveCount(0)
          await expect(menu.locator('[data-item-id="archive"]')).toHaveCount(0)
          await expect(row.locator('[data-session-archive] button')).toHaveCount(state === 'stopped' || state === 'failed' ? 1 : 0)
          await page.mouse.move(1020, 636)
          await expect(page.getByRole('tooltip')).toHaveCount(0)
          await captureFixtureEvidence(page, testInfo, `${evidenceName}-${entry}-menu-unapproved`)
          await page.keyboard.press('Escape')
          await expect(menu).toHaveCount(0)
          await expect(entry === 'overflow' ? overflow : row).toBeFocused()
        }
        await expect(row).toHaveCount(1)
        await expect(page.getByRole('dialog')).toHaveCount(0)
        await expect(page.locator('.xterm, [data-native-tab], [data-terminal-view]')).toHaveCount(0)
      })
    })
  }
}

// Targeted resume and global history use the real dialog with an inert history adapter.
for (const locale of ['en', 'zh']) {
  for (const scenario of ['resume-session', 'resume-history']) {
    test(`resume entry evidence ${scenario} ${locale}`, async ({ page }, testInfo) => {
      await page.setViewportSize({ width: 1024, height: 640 })
      await openFixture(page, { scenario, locale, gui: locale === 'zh' ? 'dark' : 'light' })
      const dialog = page.getByRole('dialog')
      await expect(dialog).toBeVisible()
      if (scenario === 'resume-session') {
        await expect(dialog.locator('[data-resume-target]')).toContainText('Review terminal rendering')
        await expect(dialog.locator('[data-resume-query], [data-resume-result]')).toHaveCount(0)
        await expect(dialog.locator('[data-confirm-resume]')).toBeEnabled()
      } else {
        await expect(dialog.locator('[data-resume-target]')).toHaveCount(0)
        await expect(dialog.locator('[data-resume-query]')).toBeVisible()
        await expect(dialog.locator('[data-resume-result]')).toHaveCount(2)
        await expect(dialog.locator('[data-confirm-resume]')).toHaveCount(0)
      }
      const cancel = dialog.getByRole('button', { name: locale === 'en' ? 'Cancel' : '取消', exact: true })
      await expect(cancel).toBeInViewport({ ratio: 1 })
      await page.mouse.move(1020, 636)
      await captureFixtureEvidence(page, testInfo, `${scenario}-${locale}-unapproved`)
      await cancel.click()
      await expect(dialog).toHaveCount(0)
      await expect(page.locator('.xterm, [data-native-tab], [data-terminal-view]')).toHaveCount(0)
      await expect(page.locator('[data-blocked-host-calls]')).toHaveAttribute('data-blocked-host-calls', '0')
    })
  }
}

// New history pixels are evidence for review, not automatically approved baselines.
// Keep the existing committed snapshot inventory and all of its assertions intact.
for (const locale of ['en', 'zh']) {
  test.describe(`historical preparation evidence ${locale}`, () => {
    test.use({ viewport: { width: 1024, height: 640 }, deviceScaleFactor: 1.25 })
    for (const outcome of ['admitted', 'unknown', 'aborted']) {
      test(`review and ${outcome} ownership receipt`, async ({ page }, testInfo) => {
        await openFixture(page, { scenario: 'historical-versions', locale, gui: locale === 'zh' ? 'dark' : 'light', historySwitch: outcome })
        await page.locator('[data-history-refresh]').click()
        await page.locator('[data-history-select]').first().click()
        await page.locator('[data-history-prepare]').click()
        await page.locator('[data-history-install]').click()
        await expect(page.locator('[data-history-back]')).toBeFocused()
        await page.keyboard.press('Tab')
        await expect(page.locator('[data-history-begin]')).toBeFocused()
        await page.keyboard.press('Tab')
        await expect(page.locator('[data-history-back]')).toBeFocused()
        await captureFixtureEvidence(page, testInfo, `history-${outcome}-review-${locale}-unapproved`)
        await page.locator('[data-history-begin]').click()
        await expect(page.locator('[role="dialog"]')).toHaveCount(0)
        await expect(page.locator('[data-history-install]')).toBeDisabled()
        await expect(page.locator('[data-history-cancel]')).toHaveCount(0)
        await page.locator('[data-history-inspect]').scrollIntoViewIfNeeded()
        await captureFixtureEvidence(page, testInfo, `history-${outcome}-receipt-${locale}-unapproved`)
        await page.locator('[data-history-inspect]').click()
        await expect(page.locator('[data-history-prepare-again]')).toHaveCount(outcome === 'aborted' ? 1 : 0)
        await expect(page.locator('[data-history-install]')).toBeDisabled()
        await expect(page.locator('[data-history-cancel]')).toHaveCount(0)
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
        await captureFixtureEvidence(page, testInfo, `history-${outcome}-inspected-${locale}-unapproved`)
      })
    }
    test('catalogue, selection and blocked preparation remain usable', async ({ page }, testInfo) => {
      await openFixture(page, { scenario: 'historical-versions', locale, gui: locale === 'zh' ? 'dark' : 'light' })
      await page.locator('[data-history-refresh]').click()
      await expect(page.locator('[data-history-row]')).toHaveCount(2)
      await expect(page.locator('[data-history-select]').nth(1)).toBeDisabled()
      await page.locator('[data-history-panel]').scrollIntoViewIfNeeded()
      await captureFixtureEvidence(page, testInfo, `history-catalog-${locale}-unapproved`)
      await page.locator('[data-history-select]').first().click()
      await expect(page.locator('[data-history-selected]')).toContainText('0.17.7')
      await page.locator('[data-history-prepare]').scrollIntoViewIfNeeded()
      await captureFixtureEvidence(page, testInfo, `history-selection-${locale}-unapproved`)
      await page.locator('[data-history-prepare]').click()
      await expect(page.locator('[data-history-status]')).toContainText('SHA256')
      await expect(page.locator('[data-history-install]')).toBeEnabled()
      await expect(page.locator('[data-update-install]')).toBeDisabled()
      await page.locator('[data-history-install]').scrollIntoViewIfNeeded()
      await expect(page.locator('[data-history-cancel]')).toBeInViewport({ ratio: 1 })
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
      await captureFixtureEvidence(page, testInfo, `history-prepared-blocked-${locale}-unapproved`)
      await page.locator('[data-history-install]').click()
      await expect(page.locator('[data-history-back]')).toBeFocused()
      await expect(page.locator('[data-history-begin]')).toBeDisabled()
      await captureFixtureEvidence(page, testInfo, `history-review-blocked-${locale}-unapproved`)
      await page.keyboard.press('Escape')
      await expect(page.locator('[role="dialog"]')).toHaveCount(0)
      await expect(page.locator('[data-history-install]')).toBeFocused()
      await page.locator('[data-history-cancel]').click()
      await expect(page.locator('[data-history-cancel]')).toHaveCount(0)
    })
  })
}


const viewports = [{ width: 1024, height: 640 }, { width: 1280, height: 720 }, { width: 1366, height: 768 }, { width: 1440, height: 900 }, { width: 1920, height: 1080 }]
for (const viewport of viewports) for (const dpr of [1, 1.25, 1.5]) for (const locale of ['zh', 'en']) for (const gui of ['light', 'dark']) for (const density of ['compact', 'standard']) {
  test.describe(`geometry ${viewport.width}x${viewport.height} DPR${dpr} ${locale} ${gui} ${density}`, () => {
    test.use({ viewport, deviceScaleFactor: dpr })
    test('preserves fixed row columns, hit areas and viewport bounds', async ({ page }) => {
      // Both terminal polarities occur under either GUI theme; DPR is never applied to CSS widths again.
      const terminal = dpr === 1.25 ? 'cc-box-light' : 'cc-box-dark'
      await openFixture(page, { scenario: 'mixed', locale, gui, density, terminal })
      await expect(page.locator('[data-session-row]')).toHaveCount(6)
      const geometry = await page.locator('[data-session-row]').evaluateAll(rows => rows.map(row => {
        const box = row.getBoundingClientRect()
        const name = row.querySelector<HTMLElement>('.session-name')!
        const cells = [...row.children].slice(0, 5).map(cell => { const rect = cell.getBoundingClientRect(); return { left: rect.left, right: rect.right, width: rect.width } })
        return { height: box.height, right: box.right, cells, titleLength: row.getAttribute('aria-label')!.length,
          truncated: name.scrollWidth > name.clientWidth, textOverflow: getComputedStyle(name).textOverflow,
          actionCount: row.querySelectorAll('.session-primary-action button').length,
          controls: [...row.querySelectorAll('.session-row-control')].map(control => { const rect = control.getBoundingClientRect(); return { width: rect.width, height: rect.height } }) }
      }))
      for (const row of geometry) {
        expect(row.height).toBe(density === 'compact' ? 34 : 38)
        expect(row.cells).toHaveLength(5)
        expect(row.cells[0].width).toBeGreaterThanOrEqual(16)
        expect(row.cells[1].width).toBeGreaterThanOrEqual(18)
        expect(row.cells[3].width).toBeGreaterThanOrEqual(38)
        expect(row.cells[4].width).toBeGreaterThanOrEqual(20)
        expect(row.actionCount).toBeLessThanOrEqual(1)
        for (let index = 1; index < row.cells.length; index++) expect(row.cells[index].left).toBeGreaterThanOrEqual(row.cells[index - 1].right)
        for (const control of row.controls) { expect(control.width).toBeGreaterThanOrEqual(20); expect(control.height).toBeGreaterThanOrEqual(28) }
      }
      expect(geometry.find(row => row.titleLength === 200)).toMatchObject({ truncated: true, textOverflow: 'ellipsis' })
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
      expect(await page.locator('.app-shell').evaluate(element => element.getBoundingClientRect().width)).toBe(viewport.width)
      expect(await page.evaluate(() => devicePixelRatio)).toBe(dpr)
      await expect(page.locator('[data-terminal-preview]')).toHaveCSS('background-color', terminal === 'cc-box-light' ? 'rgb(248, 249, 250)' : 'rgb(30, 30, 30)')
    })
  })
}

// 空宿主必须填满工作区，内容组围绕可用主区域居中；会话栏变化不能留下旧中心点。
for (const viewport of [{ width: 1024, height: 640 }, { width: 1468, height: 744 }]) for (const locale of ['zh', 'en']) for (const scenario of ['empty', 'empty-project']) {
  test.describe(`empty workspace ${viewport.width}x${viewport.height} ${locale} ${scenario}`, () => {
    test.use({ viewport, deviceScaleFactor: viewport.width === 1024 ? 1 : 1.5 })
    test('centers guidance in the available production host after sidebar changes', async ({ page }) => {
      await openFixture(page, { scenario, locale, gui: scenario === 'empty' ? 'light' : 'dark' })
      await expect(page.locator('[data-unified-terminal-host]')).toHaveCount(1)
      // Guidance uses GUI text, so its surface must follow GUI appearance, not the independent terminal theme.
      expect(await page.locator('[data-unified-terminal-empty]').evaluate(element => {
        const expected = document.createElement('span')
        expected.style.backgroundColor = 'var(--bg-primary)'
        element.append(expected)
        const matches = getComputedStyle(element).backgroundColor === getComputedStyle(expected).backgroundColor
        expected.remove()
        return matches
      })).toBe(true)
      await expect(page.locator('.xterm, [data-terminal-view], [data-native-tab]')).toHaveCount(0)
      const toggle = page.locator('.workspace-header button[aria-expanded]').first()
      for (const sidebarVisible of [true, false, true]) {
        if ((await toggle.getAttribute('aria-expanded')) !== String(sidebarVisible)) await toggle.click()
        await expect.poll(async () => page.locator('[data-unified-terminal-host]').evaluate(host => {
          const hostBox = host.getBoundingClientRect()
          const guidance = host.querySelector<HTMLElement>('.ui-empty-state')!
          const guidanceBox = guidance.getBoundingClientRect()
          const content = [...guidance.children].map(child => child.getBoundingClientRect())
          const left = Math.min(...content.map(box => box.left)), right = Math.max(...content.map(box => box.right))
          const top = Math.min(...content.map(box => box.top)), bottom = Math.max(...content.map(box => box.bottom))
          return {
            fillsHost: Math.abs(guidanceBox.width - hostBox.width) <= 1 && Math.abs(guidanceBox.height - hostBox.height) <= 1,
            centeredX: Math.abs((left + right) / 2 - (hostBox.left + hostBox.right) / 2) <= 1,
            centeredY: Math.abs((top + bottom) / 2 - (hostBox.top + hostBox.bottom) / 2) <= 1,
            insideHost: left >= hostBox.left && right <= hostBox.right && top >= hostBox.top && bottom <= hostBox.bottom,
          }
        })).toEqual({ fillsHost: true, centeredX: true, centeredY: true, insideHost: true })
      }
      await expect(page.locator('.ui-empty-state button')).toBeInViewport({ ratio: 1 })
    })
  })
}

test.describe('rendered interaction boundaries', () => {
  test.use({ viewport: { width: 1024, height: 640 } })
  test('tooltip escapes transformed clipping and remains viewport bounded', async ({ page }) => {
    await openFixture(page, { scenario: 'tooltip' })
    await page.locator('[data-tooltip-trigger]').focus()
    const tooltip = page.getByRole('tooltip')
    await expect(tooltip).toBeVisible()
    await expect(tooltip).toHaveCount(1)
    expect(await tooltip.evaluate(element => document.querySelector('[data-tooltip-clipping]')!.contains(element))).toBe(false)
    await expect(page.locator('[data-tooltip-trigger]')).toHaveAttribute('aria-describedby', (await tooltip.getAttribute('id'))!)
    const box = await tooltip.boundingBox()
    expect(box!.x).toBeGreaterThanOrEqual(12); expect(box!.y).toBeGreaterThanOrEqual(12)
    expect(box!.x + box!.width).toBeLessThanOrEqual(1012)
    expect(box!.y + box!.height).toBeLessThanOrEqual(628)
    await page.keyboard.press('Escape'); await expect(tooltip).toHaveCount(0)
    await expect(page.locator('[data-tooltip-trigger]')).toBeFocused()
    await page.locator('[data-tooltip-trigger]').hover()
    await expect(tooltip).toBeVisible()
    await page.setViewportSize({ width: 800, height: 600 })
    await expect.poll(async () => {
      const resized = await tooltip.boundingBox()
      return !!resized && resized.x >= 12 && resized.y >= 12 && resized.x + resized.width <= 788 && resized.y + resized.height <= 588
    }).toBe(true)
    await page.mouse.move(796, 596)
    await clearFixtureSetupFocus(page)
    await expect(tooltip).toHaveCount(0)
  })
  test('new-session action stays visible while options scroll and Enter submits', async ({ page }) => {
    await openFixture(page, { scenario: 'new-session', locale: 'en' })
    const dialog = page.getByRole('dialog')
    const create = dialog.locator('[data-create-session]')
    await expect(create).toBeInViewport({ ratio: 1 })
    expect(await create.evaluate(element => (element as HTMLButtonElement).form?.classList.contains('new-session-fields'))).toBe(true)
    await dialog.getByText('More options…', { exact: true }).click()
    await dialog.getByText('Developer options', { exact: true }).click()
    await dialog.locator('.ui-dialog-body').evaluate(element => { element.scrollTop = element.scrollHeight })
    await expect(create).toBeInViewport({ ratio: 1 })
    await dialog.getByRole('textbox', { name: 'Session name (optional)', exact: true }).fill('Review keyboard submission')
    await page.keyboard.press('Enter')
    await expect(dialog).toHaveCount(0)
  })
  test('menu keyboard traversal retains focus and fixed trailing control', async ({ page }) => {
    await openFixture(page, { scenario: 'mixed', locale: 'en' })
    const trigger = page.locator('[data-session-row]').first().locator('.session-overflow-trigger button')
    await trigger.focus(); await page.keyboard.press('Enter')
    const menu = page.getByRole('menu'); await expect(menu).toBeVisible()
    await page.keyboard.press('End'); await expect(menu.getByRole('menuitem').last()).toBeFocused()
    await page.keyboard.press('Escape'); await expect(menu).toHaveCount(0); await expect(trigger).toBeFocused()
    await expect(trigger).toHaveCSS('outline-style', 'solid')
  })
  test('confirmation traps focus, keeps actions visible and Escape cancels', async ({ page }) => {
    await openFixture(page, { scenario: 'confirmation' })
    const dialog = page.getByRole('dialog'); await expect(dialog).toBeVisible()
    for (let index = 0; index < 12; index++) {
      await page.keyboard.press(index % 2 ? 'Tab' : 'Shift+Tab')
      expect(await dialog.evaluate(element => element.contains(document.activeElement))).toBe(true)
    }
    const button = dialog.locator('[data-session-confirm]'); const box = await button.boundingBox()
    expect(box!.y + box!.height).toBeLessThanOrEqual(640); expect(box!.height).toBeGreaterThanOrEqual(32)
    await page.keyboard.press('Escape'); await expect(dialog).toHaveCount(0)
  })
  test('resources use overlay below threshold and shared inline context above it', async ({ page }) => {
    await openFixture(page, { scenario: 'resources' })
    await expect(page.locator('.ui-drawer')).toBeVisible(); await expect(page.locator('[data-inline-context]')).toHaveCount(0)
    await page.setViewportSize({ width: 1366, height: 768 })
    await expect(page.locator('[data-inline-context]')).toBeVisible(); await expect(page.locator('.ui-drawer')).toHaveCount(0)
    await expect(page.locator('[data-project-resources] .resource-card')).toHaveCount(3)
  })
})
