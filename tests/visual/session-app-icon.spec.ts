import { test, expect } from '@playwright/test'
import { expandFixtureProjects, clearFixtureSetupFocus } from './fixtureActions'

// 生产组件的合成状态投影验收；不连接 CLI、用户数据或安装包 WebView。
const cliName = (cli: string) => cli === 'claude' ? 'Claude Code' : cli === 'codex' ? 'Codex CLI' : 'opencode'
for (const gui of ['light', 'dark']) for (const cli of ['claude', 'codex', 'opencode']) {
  test(`merged app status ${gui} ${cli}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1024, height: 1000 })
    const errors: string[] = []
    page.on('pageerror', error => errors.push(error.message))
    await page.goto(`/__visual__/?scenario=session-status&gui=${gui}&cli=${cli}&locale=en`)
    await expect(page.locator('[data-visual-ready]')).toHaveAttribute('data-visual-ready', 'true')
    await expandFixtureProjects(page)
    const rows = page.locator('[data-session-row]')
    await expect(rows).toHaveCount(17)
    const idle = page.locator('[data-session-row="visual-status-0"] .session-app-icon')
    const working = page.locator('[data-session-row="visual-status-1"] .session-app-icon')
    const permission = page.locator('[data-session-row="visual-status-8"] .session-app-icon')
    const waiting = page.locator('[data-session-row="visual-status-7"] .session-app-icon')
    const closed = page.locator('[data-session-row="visual-status-16"] .session-app-icon')
    for (const row of await rows.all()) {
      await expect(row.locator('.session-app-icon')).toHaveCount(1)
      await expect(row).toHaveCSS('grid-template-columns', /20px .* 38px 20px 20px$/)
      await expect(row.locator('.session-app-icon')).toHaveCSS('width', '20px')
      const image = row.locator('.cli-app-icon__image:visible')
      await expect(image).toHaveCount(1)
      await expect(image).toHaveAttribute('src', new RegExp(cli + (cli === 'opencode' && gui === 'dark' ? '-dark' : '') + '\\.svg'))
      expect(await image.evaluate(el => (el as HTMLImageElement).naturalWidth)).toBe(16)
    }
    await expect(idle.locator('[data-session-badge]')).toHaveCount(cli === 'claude' ? 0 : 1)
    await expect(closed.locator('[data-session-badge]')).toHaveCount(0)
    await expect(closed).toHaveAttribute('aria-label', `${cliName(cli)}; Closed`)
    if (cli !== 'claude') {
      await expect(idle).toHaveAttribute('aria-label', `${cliName(cli)}; Idle`)
      await expect(idle.locator('[data-session-badge]')).toHaveAttribute('data-session-badge', 'running')
      await expect(idle.locator('.session-status-icon')).toHaveCSS('color', gui === 'light' ? 'rgb(61, 140, 110)' : 'rgb(93, 173, 142)')
      await expect(idle.locator('.session-status-icon__shape')).toHaveCSS('animation-name', 'none')
      await expect(idle.locator('.session-app-icon__app')).toHaveCSS('opacity', '1')
    }
    if (cli === 'codex') {
      const closedApp = closed.locator('.session-app-icon__app')
      expect(Number(await closedApp.evaluate(el => getComputedStyle(el).opacity))).toBeLessThanOrEqual(.55)
      await expect(closed.locator('img')).toHaveCSS('filter', gui === 'dark' ? 'invert(1)' : 'none')
      expect(await closed.locator('img').evaluate(el => (el as HTMLImageElement).naturalWidth)).toBe(16)
      for (const interaction of ['hover', 'focus'] as const) {
        await closed[interaction]()
        expect(Number(await closedApp.evaluate(el => getComputedStyle(el).opacity))).toBeLessThanOrEqual(.55)
        await expect(closed.locator('[data-session-badge]')).toHaveCount(0)
      }
      await page.keyboard.press('Escape')
      await clearFixtureSetupFocus(page)
    }
    await expect(working.locator('[data-session-badge]')).toHaveAttribute('data-session-badge', 'working')
    await expect(working.locator('.session-app-icon__badge')).toHaveCSS('width', '10px')
    const badgeBox = await working.locator('.session-app-icon__badge').boundingBox()
    const shapeBox = await working.locator('svg').boundingBox()
    expect(shapeBox!.y).toBeCloseTo(badgeBox!.y, 1)
    await expect(closed.locator('.session-app-icon__app')).toHaveCSS('filter', 'grayscale(1)')
    await expect(permission.locator('.session-status-icon')).toHaveCSS('color', gui === 'light' ? 'rgb(122, 92, 58)' : 'rgb(240, 212, 168)')
    await expect(waiting.locator('.session-status-icon')).toHaveCSS('color', gui === 'light' ? 'rgb(42, 80, 130)' : 'rgb(106, 154, 205)')
    await permission.hover()
    await expect(page.getByRole('tooltip')).toHaveText(cliName(cli) + '; Waiting for permission')
    await page.mouse.move(1020, 996)
    await waiting.focus()
    await expect(page.getByRole('tooltip')).toHaveText(cliName(cli) + '; Waiting for input')
    await page.keyboard.press('Escape')
    await expect(page.getByRole('tooltip')).toHaveCount(0)
    await expect(waiting).toBeFocused()
    await page.emulateMedia({ reducedMotion: 'no-preference' })
    await expect(working.locator('.session-app-icon__app')).toHaveCSS('animation-duration', '2.6s')
    await expect(working.locator('.session-app-icon__app')).toHaveCSS('animation-iteration-count', 'infinite')
    await expect(idle.locator('.session-app-icon__app')).toHaveCSS('animation-name', 'none')
    await expect(working.locator('.session-status-icon__shape')).toHaveCSS('animation-name', 'none')
    await page.emulateMedia({ reducedMotion: 'reduce' })
    await expect(working.locator('.session-app-icon__app')).toHaveCSS('animation-name', 'none')
    await expect(working.locator('[data-session-badge]')).toHaveCount(1)
    await clearFixtureSetupFocus(page)
    const screenshot = testInfo.outputPath(`merged-status-${gui}-${cli}.png`)
    await page.screenshot({ path: screenshot })
    await testInfo.attach('merged status screenshot for review', { path: screenshot, contentType: 'image/png' })
    expect(errors).toEqual([])
    await expect(page.locator('[data-blocked-host-calls]')).toHaveAttribute('data-blocked-host-calls', '0')
  })
}

for (const gui of ['light', 'dark']) test(`OpenCode actual icon surfaces ${gui}`, async ({ page }, testInfo) => {
  const errors: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  await page.route('**/*', route => new URL(route.request().url()).origin === new URL(testInfo.project.use.baseURL!).origin ? route.continue() : route.abort())
  const expected = gui === 'dark' ? 'opencode-dark.svg' : 'opencode.svg'
  const check = async (selector: string) => {
    const image = page.locator(selector).locator('.cli-app-icon__image:visible').first()
    await expect(image).toHaveAttribute('src', new RegExp(expected.replace('.', '\\.')))
    await expect(image).toHaveCSS('filter', 'none')
    expect(await image.evaluate(el => (el as HTMLImageElement).naturalWidth)).toBe(16)
    expect(await image.evaluate(el => (el as HTMLImageElement).complete)).toBe(true)
  }
  const capture = async (surface: string) => {
    await expect(page.locator('[data-blocked-host-calls]')).toHaveAttribute('data-blocked-host-calls', '0')
    const path = testInfo.outputPath(`opencode-${surface}-${gui}.png`)
    await page.screenshot({ path })
    await testInfo.attach(`${surface} ${gui}`, { path, contentType: 'image/png' })
  }
  await page.goto(`/__visual__/?scenario=session-status&cli=opencode&gui=${gui}&locale=en`)
  await expect(page.locator('[data-visual-ready]')).toHaveAttribute('data-visual-ready', 'true')
  await expandFixtureProjects(page)
  await check('[data-session-row="visual-status-0"]')
  await page.locator('[data-project-quick-action]').first().click()
  await check('[data-item-id="opencode"]')
  await capture('list-and-menu')

  await page.goto(`/__visual__/?scenario=resume-history&cli=opencode&gui=${gui}&locale=en`)
  await page.locator('[data-resume-scope]').selectOption('all')
  await check('[data-resume-result]')
  await capture('history-picker')

  await page.goto(`/__visual__/?scenario=launch-configurations&gui=${gui}&locale=en`)
  await check('[data-launch-group="opencode"]')
  await check('[data-launch-row="visual-opencode-0"]')
  await capture('launch-settings')
  await page.locator('[data-settings-section="general"]').click()
  await check('[data-cli-support="opencode"]')
  await capture('general-settings')
  expect(errors).toEqual([])
})

test('merged unread receipt touch', async ({ browser }, testInfo) => {
  const context = await browser.newContext({ hasTouch: true, isMobile: true, viewport: { width: 1024, height: 640 }, reducedMotion: 'reduce' })
  const page = await context.newPage()
  await page.goto('http://127.0.0.1:4174/__visual__/?scenario=native-notice&locale=zh&gui=dark')
  await expect(page.locator('[data-visual-ready]')).toHaveAttribute('data-visual-ready', 'true')
  await expandFixtureProjects(page)
  const icon = page.locator('[data-session-row="visual-native-notice"] .session-app-icon')
  await expect(icon.locator('[data-session-badge]')).toHaveAttribute('data-session-badge', 'completed')
  await expect(icon).toHaveAttribute('aria-label', 'Claude Code; 活动未知; 收到回复结束通知（未读）；当前活动尚未验证')
  await icon.tap()
  await expect(page.getByRole('tooltip')).toHaveText('Claude Code; 活动未知; 收到回复结束通知（未读）；当前活动尚未验证')
  await expect(icon).toHaveAttribute('data-unread', 'true')
  await expect(icon.locator('.session-app-icon__app')).toHaveCSS('animation-name', 'none')
  const screenshot = testInfo.outputPath('merged-unread-touch.png')
  await page.screenshot({ path: screenshot })
  await testInfo.attach('unread touch screenshot for review', { path: screenshot, contentType: 'image/png' })
  await expect(page.locator('[data-blocked-host-calls]')).toHaveAttribute('data-blocked-host-calls', '0')
  await context.close()
})
