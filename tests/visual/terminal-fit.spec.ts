import { expect, test } from '@playwright/test'
import { writeFile } from 'node:fs/promises'
import type {} from './terminal-fit/entry'

for (const dpr of [1, 1.25, 1.5]) {
  test.describe(`DPR ${dpr}`, () => {
    test.use({ deviceScaleFactor: dpr })
    // 真实组件与锁定渲染器必须完整容纳最后一行，PTY 行列与可见栅格一致。
    test('TerminalFit_BottomRow_001', async ({ page }, testInfo) => {
      const measurements = []
      await page.goto('/tests/visual/terminal-fit/index.html')
      await page.waitForFunction(() => window.terminalFit?.measure().sent)
      for (const [fontSize, lineHeight, terminalFontFamily] of [[12, 1.2, 'system'], [17, 1.5, 'Consolas'], [24, 2, 'system']] as const) {
        await page.evaluate(({ fontSize, lineHeight, terminalFontFamily }) => {
          Object.assign(window.terminalFit.settings, { fontSize, terminalLineHeight: lineHeight, terminalFontFamily })
        }, { fontSize, lineHeight, terminalFontFamily })
        await page.waitForTimeout(200)
        // 连续高度覆盖跨行边界，不能靠固定减一行通过。
        for (const height of [480, 481, 489, 497, 505, 513, 521, 529, 537, 545, 553]) {
          const count = await page.evaluate(() => window.terminalFit.measure().sizes.length)
          await page.setViewportSize({ width: 1024, height })
          // 等待生产 ResizeObserver 的 80ms 尾沿防抖，而非主动调用 fit 掩盖生命周期。
          await expect.poll(() => page.evaluate(() => window.terminalFit.measure().sizes.length)).toBeGreaterThan(count)
          await page.evaluate(() => window.terminalFit.markLastLine())
          const m = await page.evaluate(() => window.terminalFit.measure())
          expect(m.last.bottom, JSON.stringify(m)).toBeLessThanOrEqual(m.host.bottom)
          expect(m.screen.right, JSON.stringify(m)).toBeLessThanOrEqual(m.host.right)
          expect(m.last.text).toContain('LAST_ROW_')
          // 下一行必须放不下，防止以后用固定少算一行掩盖裁切。
          expect(m.screen.bottom + m.last.height, JSON.stringify(m)).toBeGreaterThan(m.host.bottom - 6)
          expect(m.last.height).toBeGreaterThan(0)
          expect(m.sent).toEqual({ cols: m.cols, rows: m.rows })
          expect(m.launches).toBe(1)
          const { sizes: _sizes, ...geometry } = m
          measurements.push({ fontSize, lineHeight, terminalFontFamily, height, ...geometry })
        }
      }
      await page.screenshot({ path: testInfo.outputPath('last-row.png') })
      const path = testInfo.outputPath('geometry.json')
      await writeFile(path, JSON.stringify({ dpr, measurements }, null, 2))
      await testInfo.attach('geometry', { path, contentType: 'application/json' })
    })
    // 隐藏期间不发送尺寸；字体与窗口改变后，显示重新 fit 同一个终端。
    test('TerminalFit_HiddenResize_002', async ({ page }) => {
      await page.goto('/tests/visual/terminal-fit/index.html?cli=claude')
      await page.waitForFunction(() => window.terminalFit?.measure().sent)
      await page.waitForTimeout(120)
      await page.evaluate(() => window.terminalFit.show(false))
      const count = await page.evaluate(() => window.terminalFit.measure().sizes.length)
      await page.evaluate(() => Object.assign(window.terminalFit.settings, { fontSize: 18, terminalLineHeight: 1.6 }))
      await page.setViewportSize({ width: 820, height: 620 })
      await page.waitForTimeout(120)
      expect(await page.evaluate(() => window.terminalFit.measure().sizes.length)).toBe(count)
      await page.evaluate(() => window.terminalFit.show(true))
      await page.waitForTimeout(120)
      await page.evaluate(() => window.terminalFit.markLastLine())
      const m = await page.evaluate(() => window.terminalFit.measure())
      expect(m.last.bottom, JSON.stringify(m)).toBeLessThanOrEqual(m.host.bottom)
      expect(m.sent).toEqual({ cols: m.cols, rows: m.rows })
      expect(m.launches).toBe(1)
    })
  })
}
