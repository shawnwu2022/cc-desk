import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { describe, expect, it } from 'vitest'

const scriptPath = resolve(process.cwd(), 'scripts/native-cli/record-baseline.mjs')

async function loadModule() {
  expect(existsSync(scriptPath), 'record-baseline.mjs must exist').toBe(true)
  return import(`${pathToFileURL(scriptPath).href}?case=${Date.now()}-${Math.random()}`)
}

describe('native CLI baseline report contract', () => {
  it('D01_Baseline_ScriptExists_00', () => {
    expect(existsSync(scriptPath), 'record-baseline.mjs must exist').toBe(true)
  })

  it('D01_Baseline_MissingExitCode_01', async () => {
    const { validateBaseline } = await loadModule()
    expect(() => validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: false,
      lockHashes: {},
      commands: [{
        argv: ['npm', 'run', 'test:ci'],
        status: 'PASS',
        exitCode: null,
        logPath: 'logs/npm-test.log',
      }],
      platform: 'test',
    })).toThrow('MISSING_EXIT_CODE')
  })

  it('D01_Baseline_PassRequiresZeroExit_02', async () => {
    const { validateBaseline } = await loadModule()
    expect(() => validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: false,
      lockHashes: { 'package-lock.json': 'a'.repeat(64) },
      commands: [{
        argv: ['npm', 'run', 'typecheck'],
        status: 'PASS',
        exitCode: 1,
        logPath: 'logs/typecheck.log',
      }],
      platform: 'test',
    })).toThrow('PASS_REQUIRES_ZERO_EXIT')
  })

  it('D01_Baseline_BlockedAllowsNullExit_03', async () => {
    const { validateBaseline } = await loadModule()
    expect(validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: true,
      lockHashes: {},
      commands: [{
        argv: ['codex', '--version'],
        status: 'BLOCKED',
        exitCode: null,
        logPath: 'logs/codex-version.log',
      }],
      platform: 'linux-x64',
    })).toBe(true)
  })

  it('D01_Baseline_RejectsAbsoluteLogPath_04', async () => {
    const { validateBaseline } = await loadModule()
    expect(() => validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: false,
      lockHashes: {},
      commands: [{
        argv: ['npm', 'run', 'build'],
        status: 'FAIL',
        exitCode: 1,
        logPath: '/tmp/build.log',
      }],
      platform: 'test',
    })).toThrow('LOG_PATH_MUST_BE_RELATIVE')
  })
})
