import { test, expect, type Page } from '@playwright/test'
import { expandFixtureProjects, focusFixtureMain, openFixtureSessionMenu } from './fixtureActions'

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
  await page.clock.setFixedTime(new Date('2026-09-28T12:00:00Z'))
  await page.goto(`/__visual__/?${new URLSearchParams(Object.entries(options).map(([key, value]) => [key, String(value)]))}`)
  await expect(page.locator('[data-visual-ready]')).toHaveAttribute('data-visual-ready', 'true')
  await page.evaluate(() => document.fonts.ready)
  if (['mixed', 'hover', 'menu'].includes(String(options.scenario))) {
    await expandFixtureProjects(page)
  }
}

for (const sample of snapshots) {
  test.describe(sample.name, () => {
    test.use({ viewport: { width: sample.width, height: sample.height }, deviceScaleFactor: 'dpr' in sample ? sample.dpr : 1 })
    test('snapshot', async ({ page }) => {
      await openFixture(page, { scenario: sample.scenario, locale: sample.locale, gui: 'gui' in sample ? sample.gui : 'light', density: 'density' in sample ? sample.density : 'standard', terminal: 'terminal' in sample ? sample.terminal : 'cc-box-dark' })
      if (['empty', 'mixed', 'hover', 'projects', 'terminal-settings', 'launch-configurations'].includes(sample.scenario)) await focusFixtureMain(page)
      if (sample.scenario === 'menu') await openFixtureSessionMenu(page)
      if (sample.scenario === 'tooltip') await page.locator('[data-tooltip-trigger]').focus()
      // Use the main area's empty lower-right gutter, never a window control.
      await page.mouse.move(sample.width - 4, sample.height - 4)
      if (sample.scenario !== 'tooltip') await expect(page.getByRole('tooltip')).toHaveCount(0)
      if (sample.scenario === 'hover') await page.locator('[data-session-row]').first().hover()
      await expect(page).toHaveScreenshot(`${sample.name}.png`)
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
    await focusFixtureMain(page)
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
