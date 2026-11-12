import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { defaultBaselineCommands } from '../../scripts/native-cli/record-baseline.mjs'

const root = new URL('../../', import.meta.url)
const read = file => fs.readFileSync(new URL(file, root), 'utf8').replace(/\r\n/g, '\n')

// 每个独立编译 job 必须在同一 checkout 先生成被 include_bytes 嵌入的 helper。
test('NativeHelpers_WorkflowInputs_001', () => {
  const routes = []
  for (const file of fs.readdirSync(new URL('.github/workflows/', root)).filter(name => name.endsWith('.yml'))) {
    const source = read(`.github/workflows/${file}`).split(/^jobs:\n/m)[1] ?? ''
    for (const [, id, body] of source.matchAll(/^  ([\w-]+):\n([\s\S]*?)(?=^  [\w-]+:|$(?![\s\S]))/gm)) {
      const lines = body.split('\n')
      const compile = lines.findIndex(line => (/\bcargo (test|build|check|clippy)\b/.test(line)
        && !/--manifest-path tests\/native-cli\/(observer|transport|input)-core\//.test(line))
        || /windows-rust-tests\.ps1 -Action Compile/.test(line))
      const session = lines.findIndex(line => /^\s+(?:run: )?npm run prepare:session-sdk\s*$/.test(line))
      if (compile < 0 && session < 0) continue
      routes.push(`${file}/${id}`)
      const archive = lines.findIndex(line => /^\s+(?:run: )?npm run prepare:opencode-archive\s*$/.test(line))
      assert.ok(session >= 0 && archive > session, `${file}/${id}: prepare both native helpers after npm ci`)
      assert.ok(compile < 0 || archive < compile, `${file}/${id}: archive resources must exist before direct Cargo compilation`)
      assert.ok(lines.slice(0, session).some(line => /npm ci\b/.test(line)), `${file}/${id}: install locked build dependencies first`)
      assert.ok(lines[archive + 1]?.includes('if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }')
        || body.includes('set -euo pipefail'), `${file}/${id}: abort on archive preparation failure`)
    }
  }
  for (const route of ['ci.yml/rust-compile', 'ci.yml/rust-static', 'ci.yml/roundtrip-release', 'ci.yml/roundtrip-debug',
    'conpty-integration.yml/validation', 'conpty-integration.yml/package', 'd12-scope-core.yml/core']) {
    assert.ok(routes.includes(route), `required independent build route disappeared: ${route}`)
  }
})

// 本地 baseline 与公开 Rust 测试命令也不能依赖旧工作区残留的生成资源。
test('NativeHelpers_LocalInputs_002', () => {
  for (const platform of ['win32', 'linux', 'darwin']) {
    const commands = defaultBaselineCommands(platform)
    const session = commands.findIndex(command => command.argv.includes('scripts/prepare-session-sdk.mjs'))
    const archive = commands.findIndex(command => command.argv.includes('scripts/prepare-opencode-archive-sdk.mjs'))
    const compile = commands.findIndex(command => command.argv[0] === 'cargo' && command.argv[1] !== 'fmt')
    assert.ok(session >= 0 && archive > session && archive < compile, `${platform}: prepare helpers before baseline Cargo`)
  }
  const command = JSON.parse(read('package.json')).scripts['test:rust:lib']
  assert.match(command, /^npm run prepare:session-sdk && npm run prepare:opencode-archive && cd src-tauri && cargo test --locked --lib$/)
})

// 分片运行真实 Rust/helper 测试，不能只升级编译机而让执行机继续使用旧 Node。
test('NativeHelpers_NodeRuntime_003', () => {
  const routes = []
  for (const file of fs.readdirSync(new URL('.github/workflows/', root)).filter(name => name.endsWith('.yml'))) {
    const source = read(`.github/workflows/${file}`).split(/^jobs:\n/m)[1] ?? ''
    for (const [, id, body] of source.matchAll(/^  ([\w-]+):\n([\s\S]*?)(?=^  [\w-]+:|$(?![\s\S]))/gm)) {
      const mainCrate = body.split('\n').some(line => (/\bcargo (test|build|check|clippy)\b/.test(line)
        && !/--manifest-path tests\/native-cli\//.test(line))
        || /windows-rust-tests\.ps1 -Action Compile/.test(line)
        || /windows-rust-shard-runner\.mjs run\b/.test(line))
      const preparedPackage = /npm run prepare:opencode-archive/.test(body) && /npm run tauri\b/.test(body)
      if (!mainCrate && !preparedPackage) continue
      routes.push(`${file}/${id}`)
      const nodeStep = body.split(/(?=^      - )/m).find(step => /uses: actions\/setup-node@/.test(step))
      const version = nodeStep?.match(/node-version: ['"]?(\d+)\.(\d+)\.(\d+)['"]?\s*$/m)
      assert.ok(version && (Number(version[1]) > 24 || Number(version[1]) === 24 && Number(version[2]) >= 18),
        `${file}/${id}: main crate helper execution requires an exact Node runtime >=24.18.0`)
    }
  }
  assert.ok(routes.includes('ci.yml/rust-compile'), 'check the producer runtime')
  assert.ok(routes.includes('ci.yml/rust-tests'), 'check the independent compiled-test execution runtime')
  assert.ok(routes.includes('conpty-integration.yml/package'), 'check the Windows package runtime')
})
