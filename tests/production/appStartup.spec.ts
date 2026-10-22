import { test, expect } from '@playwright/test'
import { readFileSync } from 'node:fs'

for (const language of ['en', 'zh'] as const) {
  // 真实生产入口在严格 CSP 下必须显示导航和设置；缺失 CLI/bridge 只能成为局部提示。
  test(`AppStartup_StrictCsp_001_${language}`, async ({ page }) => {
    const errors: string[] = []
    page.on('pageerror', error => errors.push(error.message))
    page.on('console', message => {
      if (message.type() === 'error' && /EvalError|Content Security Policy/.test(message.text())) errors.push(message.text())
    })
    await page.addInitScript(({ language }) => {
      let callbackId = 1
      const host = window as unknown as {
        __TAURI_INTERNALS__: Record<string, unknown>
        __TAURI_EVENT_PLUGIN_INTERNALS__: Record<string, unknown>
        __STARTUP_CSP_VIOLATIONS__: string[]
      }
      host.__STARTUP_CSP_VIOLATIONS__ = []
      document.addEventListener('securitypolicyviolation', event => host.__STARTUP_CSP_VIOLATIONS__.push(event.violatedDirective))
      host.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => callbackId++,
        unregisterCallback() {},
        invoke: async (command: string) => {
          if (command === 'get_app_config') return { language }
          if (command === 'plugin:event|listen') return callbackId++
          if (command === 'plugin:event|unlisten') return undefined
          if (/^plugin:window\|(?:is_maximized|is_focused)$/.test(command)) return false
          // 仅模拟 IPC 边界；所有其他读取和写入固定失败，不安装 Native authority 或启动 CLI。
          throw { code: 'TEST_READ_UNAVAILABLE' }
        },
      }
      host.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
    }, { language })

    const response = await page.goto('/')
    expect(response?.headers()['content-security-policy']).toContain("default-src 'self'")
    expect(response?.headers()['content-security-policy']).not.toContain('unsafe-eval')
    await expect(page.locator('.app-shell'), 'Production app must mount its visible shell under the actual CSP').toBeVisible()
    const favicon = await page.locator('link[rel="icon"]').getAttribute('href')
    expect(favicon).toMatch(/^\/assets\/app-icon-.*\.ico$/)
    const faviconResponse = await page.request.get(favicon!)
    expect(faviconResponse.status()).toBe(200)
    expect(await faviconResponse.body()).toEqual(readFileSync('src-tauri/icons/source/windows/ccdesk.ico'))
    const titleIcon = page.locator('.win-app-icon')
    await expect(titleIcon).toBeVisible()
    expect(await titleIcon.evaluate(node => (node as HTMLImageElement).naturalWidth)).toBe(16)
    // 小 PNG 在生产构建中内联，暗色豁免不能依赖已消失的资源文件名。
    await page.evaluate(() => { document.documentElement.dataset.theme = 'dark' })
    await expect(titleIcon).toHaveCSS('filter', 'none')
    await expect(page.locator('[data-primary-section="workspace"]')).toHaveAttribute('aria-label', language === 'zh' ? '工作区' : 'Workspace')
    const sidebar = page.locator('[data-session-column]')
    const toggleKey = process.platform === 'darwin' ? 'Meta+b' : 'Control+b'
    await expect(sidebar).toBeVisible()
    await sidebar.locator('[data-active-session-filter]').focus()
    await page.keyboard.press(toggleKey)
    await expect(sidebar).toBeHidden()
    await page.keyboard.press(toggleKey)
    await expect(sidebar).toBeVisible()
    const search = sidebar.locator('input.search-input')
    await search.focus()
    await page.keyboard.press(toggleKey)
    await expect(sidebar).toBeVisible()
    await expect(search).toBeFocused()
    await page.locator('[data-primary-section="projects"]').click()
    await expect(page.locator('.projects-view')).toBeVisible()
    await page.locator('[data-primary-section="settings"]').click()
    await expect(page.locator('.settings-view')).toBeVisible()
    await expect(page.locator('.settings-nav h1')).toHaveText(language === 'zh' ? '设置' : 'Settings')
    await page.locator('[data-settings-section="about"]').click()
    const aboutIcon = page.locator('[data-settings-about] img[alt="CC Desk"]')
    await expect(aboutIcon).toBeVisible()
    expect(await aboutIcon.evaluate(node => (node as HTMLImageElement).naturalWidth)).toBe(256)
    await expect(aboutIcon).toHaveCSS('filter', 'none')
    await page.evaluate(() => { document.documentElement.dataset.theme = 'light' })
    await expect(titleIcon).toHaveCSS('filter', 'none')
    await expect(aboutIcon).toHaveCSS('filter', 'none')
    expect(await page.evaluate(() => (window as unknown as { __STARTUP_CSP_VIOLATIONS__: string[] }).__STARTUP_CSP_VIOLATIONS__)).toEqual([])
    expect(errors, 'Production startup and lazy settings must not require blocked JavaScript evaluation').toEqual([])
  })
}
