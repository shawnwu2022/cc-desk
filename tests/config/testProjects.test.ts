import { expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { relative, resolve } from 'node:path'

// 执行真实入口的文件选择，防止快速环漏掉普通测试或完整入口漏掉慢测试。
it.each([
  { script: 'test:fast', included: ['tests/components/projectsView.test.ts', 'tests/config/csp.test.ts'], excluded: ['tests/native-cli/probe.test.ts', 'tests/stores/unifiedStress.test.ts'] },
  { script: 'test:process', included: ['tests/native-cli/probe.test.ts', 'tests/native-cli/observerReporter.test.ts', 'tests/config/visualWorkflow.test.ts'], excluded: ['tests/components/projectsView.test.ts', 'tests/stores/unifiedStress.test.ts'] },
  { script: 'test:stress', included: ['tests/stores/unifiedStress.test.ts'], excluded: ['tests/components/projectsView.test.ts', 'tests/native-cli/probe.test.ts'] },
  { script: 'test:all', included: ['tests/components/projectsView.test.ts', 'tests/native-cli/probe.test.ts', 'tests/stores/unifiedStress.test.ts'], excluded: ['tests/visual/fixture.test.ts'] },
  { script: 'test:ci', included: ['tests/components/projectsView.test.ts', 'tests/native-cli/probe.test.ts', 'tests/stores/unifiedStress.test.ts'], excluded: ['tests/visual/fixture.test.ts'] },
])('TestGroups_Selection_001: $script', ({ script, included, excluded }) => {
  const scripts = JSON.parse(readFileSync('package.json', 'utf8')).scripts
  expect(scripts[script], `缺少测试入口 ${script}`).toBeTypeOf('string')
  const command = scripts[script].split(' ')
  expect(command.slice(0, 2)).toEqual(['vitest', 'run'])
  const result = spawnSync(process.execPath, [resolve('node_modules/vitest/vitest.mjs'), 'list', ...command.slice(2), '--filesOnly', '--json'], {
    encoding: 'utf8', timeout: 15_000, windowsHide: true,
  })
  expect(result.error, `${script} 文件选择未完成`).toBeUndefined()
  expect(result.status, `${script} 文件选择失败: ${result.stderr}`).toBe(0)
  const files = JSON.parse(result.stdout).map((row: { file: string }) => relative(process.cwd(), row.file).replaceAll('\\', '/'))
  for (const path of included) expect(files, `${script} 漏掉 ${path}`).toContain(path)
  for (const path of excluded) expect(files, `${script} 错选 ${path}`).not.toContain(path)
}, 20_000)

// 独立扫描原始文件范围，检查分组后的完整入口恰好执行每个测试文件一次。
it('TestGroups_CompleteInventory_002', () => {
  const expected = readdirSync('tests', { recursive: true })
    .map(path => `tests/${String(path).replaceAll('\\', '/')}`)
    .filter(path => path.endsWith('.test.ts') && !path.startsWith('tests/visual/'))
    .sort()
  const result = spawnSync(process.execPath, [resolve('node_modules/vitest/vitest.mjs'), 'list', '--filesOnly', '--json'], {
    encoding: 'utf8', timeout: 15_000, windowsHide: true,
  })
  expect(result.error).toBeUndefined()
  expect(result.status, `完整测试库存读取失败: ${result.stderr}`).toBe(0)
  const actual = JSON.parse(result.stdout).map((row: { file: string }) => relative(process.cwd(), row.file).replaceAll('\\', '/'))
  expect(actual.sort()).toEqual(expected)
  expect(new Set(actual).size).toBe(actual.length)
}, 20_000)

// 进程契约不依赖浏览器全局，检查它不会继续承担 jsdom 初始化。
it('TestGroups_NodeEnvironment_003', () => {
  expect(typeof window).toBe('undefined')
  expect(typeof document).toBe('undefined')
})
