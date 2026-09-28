import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { probeInstalledCli } from '../../scripts/native-cli/canary-probe.mjs'

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-canary-'))
  const packageDir = join(root, 'node_modules', '@example', 'codex')
  mkdirSync(packageDir, { recursive: true })
  writeFileSync(join(packageDir, 'package.json'), JSON.stringify({
    name: '@example/codex',
    version: '1.2.3',
    bin: { codex: 'bin.mjs' },
  }))
  writeFileSync(join(packageDir, 'bin.mjs'), `
    if (process.argv.includes('--version')) {
      process.stdout.write('codex 1.2.3\\n')
    } else if (process.argv.includes('--help')) {
      process.stdout.write('help\\n')
    } else {
      process.exitCode = 9
    }
  `)
  const workDir = join(root, 'work')
  mkdirSync(workDir)
  return { root, workDir }
}

test('D29_Canary_RecordsIdentityButNeverClaimsCertification_01', () => {
  const value = fixture()
  const report = probeInstalledCli({
    packageRoot: value.root,
    packageName: '@example/codex',
    cli: 'codex',
    lane: 'pinned',
    requestedSpec: '1.2.3',
    workDir: value.workDir,
    sourceEnv: { PATH: process.env.PATH ?? '' },
  })
  assert.equal(report.probeStatus, 'PASS')
  assert.equal(report.certificationStatus, 'NOT_RUN')
  assert.equal(report.resolvedVersion, '1.2.3')
  assert.match(report.binarySha256, /^[0-9a-f]{64}$/)
})

test('D29_Canary_DoesNotForwardProviderCredentials_02', () => {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-canary-secret-'))
  const packageDir = join(root, 'node_modules', '@example', 'claude')
  mkdirSync(packageDir, { recursive: true })
  writeFileSync(join(packageDir, 'package.json'), JSON.stringify({
    name: '@example/claude',
    version: '4.5.6',
    bin: { claude: 'bin.mjs' },
  }))
  writeFileSync(join(packageDir, 'bin.mjs'), `
    if (process.env.OPENAI_API_KEY || process.env.ANTHROPIC_API_KEY || process.env.CODEX_API_KEY) {
      process.exit(8)
    }
    if (process.argv.includes('--version')) process.stdout.write('claude 4.5.6\\n')
    else if (process.argv.includes('--help')) process.stdout.write('help\\n')
    else process.exitCode = 9
  `)
  const workDir = join(root, 'work')
  mkdirSync(workDir)
  const report = probeInstalledCli({
    packageRoot: root,
    packageName: '@example/claude',
    cli: 'claude',
    lane: 'latest-stable',
    requestedSpec: 'latest',
    workDir,
    sourceEnv: {
      PATH: process.env.PATH ?? '',
      OPENAI_API_KEY: 'fixture-secret',
      ANTHROPIC_API_KEY: 'fixture-secret',
      CODEX_API_KEY: 'fixture-secret',
    },
  })
  assert.equal(report.probeStatus, 'PASS')
  assert.equal(report.certificationStatus, 'NOT_RUN')
})

test('D29_Canary_RejectsBinaryPathEscape_03', () => {
  const value = fixture()
  const packageDir = join(value.root, 'node_modules', '@example', 'codex')
  writeFileSync(join(packageDir, 'package.json'), JSON.stringify({
    name: '@example/codex',
    version: '1.2.3',
    bin: { codex: '../escape.mjs' },
  }))
  assert.throws(() => probeInstalledCli({
    packageRoot: value.root,
    packageName: '@example/codex',
    cli: 'codex',
    lane: 'latest-stable',
    requestedSpec: 'latest',
    workDir: value.workDir,
  }), /CANARY_BINARY_INVALID/)
})
