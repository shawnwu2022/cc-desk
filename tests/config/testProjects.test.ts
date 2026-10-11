import { expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { relative, resolve } from 'node:path'

// 用一次真实文件发现核对库存与脚本选择，避免重复启动完整配置增加 CI 开销。
it('TestGroups_CompleteInventory_001', () => {
  const scripts = JSON.parse(readFileSync('package.json', 'utf8')).scripts
  for (const script of ['test', 'test:all', 'test:ci']) {
    expect(scripts[script], `${script} 必须保留完整测试入口`).toBe('vitest run')
  }
  const expected = readdirSync('tests', { recursive: true })
    .map(path => `tests/${String(path).replaceAll('\\', '/')}`)
    .filter(path => path.endsWith('.test.ts') && !path.startsWith('tests/visual/'))
    .sort()
  const result = spawnSync(process.execPath, [resolve('node_modules/vitest/vitest.mjs'), 'list', '--filesOnly', '--json'], {
    encoding: 'utf8', timeout: 15_000, windowsHide: true,
  })
  expect(result.error).toBeUndefined()
  expect(result.status, `完整测试库存读取失败: ${result.stderr}`).toBe(0)
  const inventory: { file: string; projectName: string }[] = JSON.parse(result.stdout)
    .map((row: { file: string; projectName: string }) => ({ ...row, file: relative(process.cwd(), row.file).replaceAll('\\', '/') }))
  const actual = inventory.map(row => row.file)
  expect(actual.sort()).toEqual(expected)
  expect(new Set(actual).size).toBe(actual.length)
  for (const { script, projects, included, excluded } of [
    { script: 'test:fast', projects: ['fast-node', 'fast-dom'], included: ['tests/components/projectsView.test.ts', 'tests/config/csp.test.ts', 'tests/native-cli/xtermProvenance.test.ts'], excluded: ['tests/native-cli/probe.test.ts', 'tests/stores/unifiedStress.test.ts'] },
    { script: 'test:process', projects: ['process'], included: ['tests/native-cli/probe.test.ts', 'tests/native-cli/observerReporter.test.ts', 'tests/config/visualWorkflow.test.ts', 'tests/config/testProjects.test.ts'], excluded: ['tests/components/projectsView.test.ts', 'tests/stores/unifiedStress.test.ts'] },
    { script: 'test:stress', projects: ['stress'], included: ['tests/stores/unifiedStress.test.ts'], excluded: ['tests/components/projectsView.test.ts', 'tests/native-cli/probe.test.ts'] },
  ]) {
    expect(scripts[script], `缺少测试入口 ${script}`).toBeTypeOf('string')
    expect(scripts[script].split(' '), `${script} 的项目选择参数`).toEqual(['vitest', 'run', ...projects.flatMap(name => ['--project', name])])
    const files = inventory.filter(row => projects.includes(row.projectName)).map(row => row.file)
    for (const path of included) expect(files, `${script} 漏掉 ${path}`).toContain(path)
    for (const path of excluded) expect(files, `${script} 错选 ${path}`).not.toContain(path)
  }
  expect(inventory.find(row => row.file === 'tests/native-cli/xtermProvenance.test.ts')?.projectName).toBe('fast-dom')
}, 20_000)

// 进程契约不依赖浏览器全局，检查它不会继续承担 jsdom 初始化。
it('TestGroups_NodeEnvironment_002', () => {
  expect(typeof window).toBe('undefined')
  expect(typeof document).toBe('undefined')
})
