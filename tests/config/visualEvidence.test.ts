import { describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'

const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg=='

describe('Visual review evidence retention', () => {
  // 用真实Playwright列表报告器执行成功用例，六个未批准PNG必须仍留在输出目录。
  it('VisualEvidence_RetainPassingPng_001', () => {
    const directory = mkdtempSync(join(tmpdir(), 'cc-desk-evidence-'))
    const output = join(directory, 'retained')
    const helper = resolve('tests/visual/fixtureEvidence.ts')
    const config = resolve('playwright.config.ts')
    const playwright = resolve('node_modules/@playwright/test/index.js')
    const cli = resolve('node_modules/@playwright/test/cli.js')
    try {
      writeFileSync(join(directory, 'playwright.config.ts'), `import base from ${JSON.stringify(config)}
export default { testDir: '.', testMatch: 'evidence.spec.ts', reporter: base.reporter,
  preserveOutput: base.preserveOutput, outputDir: ${JSON.stringify(output)}, workers: 1, retries: 0 }
`)
      writeFileSync(join(directory, 'evidence.spec.ts'), `import { test } from ${JSON.stringify(playwright)}
import { writeFileSync } from 'node:fs'
import { captureFixtureEvidence } from ${JSON.stringify(helper)}
const bytes = Buffer.from(${JSON.stringify(png)}, 'base64')
for (const locale of ['en', 'zh']) test('passing evidence ' + locale, async ({}, testInfo) => {
  const page = { async screenshot(options) { if (options?.path) writeFileSync(options.path, bytes); return bytes } }
  for (const state of ['catalog', 'selection', 'prepared-blocked']) {
    await captureFixtureEvidence(page, testInfo, 'history-' + state + '-' + locale + '-unapproved')
  }
})
`)
      const run = spawnSync(process.execPath, [cli, 'test', '--config', join(directory, 'playwright.config.ts')], {
        cwd: directory, encoding: 'utf8', timeout: 20000, env: { ...process.env, FORCE_COLOR: '0' },
      })
      expect(run.error).toBeUndefined()
      expect(run.status, `${run.stdout}\n${run.stderr}`).toBe(0)
      const files = readdirSync(output, { recursive: true }).filter((name): name is string => typeof name === 'string' && /history-.+-unapproved\.png$/.test(name))
      const expected = ['en', 'zh'].flatMap(locale => ['catalog', 'selection', 'prepared-blocked'].map(state => `history-${state}-${locale}-unapproved.png`)).sort()
      expect(files.map(file => basename(file)).sort(), 'passing list-reporter cases must retain all six named PNG files').toEqual(expected)
      for (const file of files) expect(readFileSync(join(output, file))).toEqual(Buffer.from(png, 'base64'))
    } finally { rmSync(directory, { recursive: true, force: true }) }
  }, 25000)
})
