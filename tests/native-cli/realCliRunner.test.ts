import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

const runnerPath = resolve(process.cwd(), 'scripts/native-cli/real-cli-runner.mjs')
const cleanup: string[] = []

async function loadRunner() {
  expect(existsSync(runnerPath), 'real-cli-runner.mjs must exist').toBe(true)
  return import(`${pathToFileURL(runnerPath).href}?case=${Date.now()}-${Math.random()}`)
}

function root() {
  const value = mkdtempSync(join(tmpdir(), 'cc-desk-d20-runner-'))
  cleanup.push(value)
  return value
}

function config(overrides: Record<string, unknown> = {}) {
  const testRoot = root()
  return {
    testRoot,
    cli: 'codex',
    nonce: 'd20-runner-nonce-123',
    authorizedTestAccount: true,
    binaryPath: join(testRoot, 'bin', 'codex'),
    drivers: {
      ccDesk: join(testRoot, 'drivers', 'cc-desk-driver'),
      systemTerminal: join(testRoot, 'drivers', 'system-terminal-driver'),
    },
    hostEnv: {
      PATH: '/explicit/test/path',
      LANG: 'C.UTF-8',
      OPENAI_API_KEY: 'production-openai-secret-must-not-leak',
      ANTHROPIC_API_KEY: 'production-anthropic-secret-must-not-leak',
      CLAUDE_CODE_OAUTH_TOKEN: 'production-claude-token-must-not-leak',
      RANDOM_UNRELATED_SECRET: 'must-not-leak',
    },
    testAccountEnv: {
      D20_TEST_ACCOUNT_TOKEN: 'explicit-authorized-test-token',
    },
    transformId: 'codex-user-prompt-submit-v1-exact',
    originalText: 'd20-runner-nonce-123\n你好\n<pasted_content id="literal">keep literal</pasted_content id="literal">\n',
    ...overrides,
  }
}


function materializeBinary(testRoot: string) {
  const path = join(testRoot, 'bin', 'codex')
  mkdirSync(join(testRoot, 'bin'), { recursive: true })
  writeFileSync(path, 'fixture binary identity only', 'utf8')
  return path
}

function materializeDriver(testRoot: string, body: string) {
  const dir = join(testRoot, 'drivers')
  mkdirSync(dir, { recursive: true })
  for (const name of ['cc-desk-driver.mjs', 'system-terminal-driver.mjs']) {
    writeFileSync(join(dir, name), body, 'utf8')
  }
  return {
    ccDesk: join(dir, 'cc-desk-driver.mjs'),
    systemTerminal: join(dir, 'system-terminal-driver.mjs'),
  }
}

function blockedDriver(reason = 'SYNTHETIC_TEST_DRIVER_BLOCKED') {
  return [
    "import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'",
    "import { dirname } from 'node:path'",
    "const args = Object.fromEntries(process.argv.slice(2).reduce((rows, item, index, all) => {",
    "  if (item.startsWith('--')) rows.push([item.slice(2), all[index + 1]])",
    "  return rows",
    "}, []))",
    "const fixture = JSON.parse(readFileSync(args.fixture, 'utf8'))",
    "mkdirSync(dirname(args.report), { recursive: true })",
    "writeFileSync(args.report, JSON.stringify({",
    "  schemaVersion: 1, caseId: 'NATIVE-63', evidenceLayer: 'C', status: 'BLOCKED',",
    "  runId: fixture.runId, cli: fixture.cli, reason: " + JSON.stringify(reason) + ",",
    "  environmentProbe: {",
    "    hasOpenAi: Object.hasOwn(process.env, 'OPENAI_API_KEY'),",
    "    hasAnthropic: Object.hasOwn(process.env, 'ANTHROPIC_API_KEY'),",
    "    hasClaudeOauth: Object.hasOwn(process.env, 'CLAUDE_CODE_OAUTH_TOKEN'),",
    "    hasExplicitTestToken: Object.hasOwn(process.env, 'D20_TEST_ACCOUNT_TOKEN'),",
    "    homeInsideTestRoot: process.env.HOME?.startsWith(process.env.CC_DESK_TEST_ROOT ?? '') ?? false,",
    "  },",
    "}), { flag: 'wx' })",
  ].join('\n')
}


function passDriver(binaryHashExpression: string) {
  return [
    "import { createHash } from 'node:crypto'",
    "import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'",
    "import { dirname } from 'node:path'",
    "const args = Object.fromEntries(process.argv.slice(2).reduce((rows, item, index, all) => {",
    "  if (item.startsWith('--')) rows.push([item.slice(2), all[index + 1]])",
    "  return rows",
    "}, []))",
    "const fixture = JSON.parse(readFileSync(args.fixture, 'utf8'))",
    "const payload = Buffer.from(fixture.hostPayloadBase64, 'base64')",
    "const payloadSha256 = createHash('sha256').update(payload).digest('hex')",
    "const binarySha256 = " + binaryHashExpression,
    "const target = {",
    "  targetId: 'd20-fixture-target', os: 'fixture-os', osBuild: 'fixture-build', arch: 'fixture-arch',",
    "  cli: { kind: fixture.cli, version: 'codex-fixture-1', binarySha256 },",
    "}",
    "const host = fixture.lane === 'cc-desk'",
    "  ? { kind: 'cc-desk', deskCommit: 'c'.repeat(40), xtermVersion: '5.5.0', webviewRuntime: 'fixture-webview', renderer: 'webgl' }",
    "  : { kind: 'system-terminal', terminalProgram: 'fixture-terminal' }",
    "const hostPayloadEvidence = fixture.lane === 'cc-desk'",
    "  ? { kind: 'native-input-frame', bytesBase64: fixture.hostPayloadBase64, sha256: payloadSha256, frame: { runId: fixture.runId, generation: 1, inputSeq: '1', modeEpoch: '1' } }",
    "  : { kind: 'terminal-driver-write', bytesBase64: fixture.hostPayloadBase64, sha256: payloadSha256, driver: 'd20-system-terminal-driver-v1', writeSeq: '1' }",
    "const prompt = payload.toString('utf8')",
    "const rawEnvelope = { session_id: 'd20-session', turn_id: 'd20-turn', cwd: fixture.expectedCwd, hook_event_name: 'UserPromptSubmit', prompt }",
    "const record = {",
    "  schemaVersion: 1, caseId: 'NATIVE-63', evidenceLayer: 'C', status: 'PASS',",
    "  runId: fixture.runId, lane: fixture.lane, observer: fixture.observer, target, host, hostPayloadEvidence,",
    "  fixture: { fixtureSha256: fixture.fixtureSha256, nonce: fixture.nonce, originalText: fixture.originalText, hostPayloadBase64: fixture.hostPayloadBase64, transformId: fixture.transformId },",
    "  oracle: { kind: 'real-user-prompt-submit', schemaVersion: 1, cli: fixture.cli, runId: fixture.runId, lane: fixture.lane, observer: fixture.observer, transformId: fixture.transformId, validation: { valid: true }, sessionId: 'd20-session', turnId: 'd20-turn', cwd: fixture.expectedCwd, rawEnvelope },",
    "}",
    "mkdirSync(dirname(args.report), { recursive: true })",
    "writeFileSync(args.report, JSON.stringify(record), { flag: 'wx' })",
  ].join('\n')
}

afterEach(() => {
  while (cleanup.length > 0) {
    rmSync(cleanup.pop()!, { recursive: true, force: true })
  }
})

describe('D20 real CLI matrix runner', () => {
  it('D20_Runner_ModuleExists_00', async () => {
    await loadRunner()
  })

  it('D20_Runner_NoAuthorizedTestAccountIsBlockedBeforeExecution_01', async () => {
    const { prepareD20Matrix } = await loadRunner()
    const value = prepareD20Matrix(config({
      authorizedTestAccount: false,
      binaryPath: null,
      drivers: null,
      testAccountEnv: null,
    }))

    expect(value).toEqual({
      status: 'BLOCKED',
      cli: 'codex',
      reason: 'AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE',
      runs: [],
    })
  })

  it('D20_Runner_BuildsExactlyFourIsolatedCells_02', async () => {
    const { prepareD20Matrix } = await loadRunner()
    const value = prepareD20Matrix(config())

    expect(value.status).toBe('READY')
    expect(value.runs.map((run: any) => `${run.lane}:${run.observer}`)).toEqual([
      'cc-desk:off',
      'cc-desk:on',
      'system-terminal:off',
      'system-terminal:on',
    ])
    expect(new Set(value.runs.map((run: any) => run.runId)).size).toBe(4)
    expect(new Set(value.runs.map((run: any) => run.configRoot)).size).toBe(4)
    expect(new Set(value.runs.map((run: any) => run.reportPath)).size).toBe(4)

    for (const run of value.runs) {
      expect(run.testRoot).toBe(value.testRoot)
      expect(run.projectRoot.startsWith(value.testRoot)).toBe(true)
      expect(run.configRoot.startsWith(value.testRoot)).toBe(true)
      expect(run.reportPath.startsWith(value.testRoot)).toBe(true)
      expect(run.fixture.runId).toBe(run.runId)
      expect(run.fixture.lane).toBe(run.lane)
      expect(run.fixture.observer).toBe(run.observer)
      expect(run.fixture.expectedCwd).toBe(run.projectRoot)
      expect(run.fixture.nonce).toBe('d20-runner-nonce-123')
    }
  })

  it('D20_Runner_DoesNotInheritProductionCredentialEnvironment_03', async () => {
    const { prepareD20Matrix } = await loadRunner()
    const value = prepareD20Matrix(config())

    expect(value.status).toBe('READY')
    for (const run of value.runs) {
      expect(run.env.PATH).toBe('/explicit/test/path')
      expect(run.env.LANG).toBe('C.UTF-8')
      expect(run.env.D20_TEST_ACCOUNT_TOKEN).toBe('explicit-authorized-test-token')
      expect(run.env.OPENAI_API_KEY).toBeUndefined()
      expect(run.env.ANTHROPIC_API_KEY).toBeUndefined()
      expect(run.env.CLAUDE_CODE_OAUTH_TOKEN).toBeUndefined()
      expect(run.env.RANDOM_UNRELATED_SECRET).toBeUndefined()
      expect(run.env.HOME.startsWith(value.testRoot)).toBe(true)
      expect(run.env.USERPROFILE.startsWith(value.testRoot)).toBe(true)
      expect(run.env.XDG_CONFIG_HOME.startsWith(value.testRoot)).toBe(true)
      expect(run.env.CODEX_HOME).toBe(run.configRoot)
      expect(run.env.CLAUDE_CONFIG_DIR).toBeUndefined()
      expect(run.env.CC_DESK_REAL_CLI_TEST_MODE).toBe('1')
      expect(run.env.CC_DESK_TEST_ROOT).toBe(value.testRoot)
    }
  })

  it('D20_Runner_ClaudeUsesOnlyClaudeIsolatedConfigRoot_04', async () => {
    const { prepareD20Matrix } = await loadRunner()
    const value = prepareD20Matrix(config({
      cli: 'claude',
      transformId: 'claude-user-prompt-submit-v1-exact',
    }))

    expect(value.status).toBe('READY')
    for (const run of value.runs) {
      expect(run.env.CLAUDE_CONFIG_DIR).toBe(run.configRoot)
      expect(run.env.CODEX_HOME).toBeUndefined()
    }
  })

  it('D20_Runner_RejectsPathsOutsideTheExplicitTestRoot_05', async () => {
    const { prepareD20Matrix } = await loadRunner()
    const value = prepareD20Matrix(config({
      binaryPath: resolve(tmpdir(), 'outside-real-cli'),
    }))

    expect(value).toEqual({
      status: 'BLOCKED',
      cli: 'codex',
      reason: 'REAL_CLI_BINARY_NOT_ISOLATED',
      runs: [],
    })
  })

  it('D20_Runner_ExecutionMissingBinaryIsBlocked_06', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const value = config()
    value.drivers = materializeDriver(value.testRoot, blockedDriver())
    const plan = prepareD20Matrix(value)

    expect(executeD20Matrix(plan)).toEqual({
      status: 'BLOCKED',
      cli: 'codex',
      reason: 'REAL_CLI_BINARY_UNAVAILABLE',
      recordPaths: [],
    })
  })

  it('D20_Runner_ExecutionMissingDriverIsBlocked_07', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const value = config()
    materializeBinary(value.testRoot)
    const plan = prepareD20Matrix(value)

    expect(executeD20Matrix(plan)).toEqual({
      status: 'BLOCKED',
      cli: 'codex',
      reason: 'REAL_CLI_DRIVER_UNAVAILABLE',
      recordPaths: [],
    })
  })

  it('D20_Runner_ExecutesFourCellsWithoutProductionCredentialLeak_08', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const value = config()
    materializeBinary(value.testRoot)
    value.drivers = materializeDriver(value.testRoot, blockedDriver())
    const plan = prepareD20Matrix(value)
    const result = executeD20Matrix(plan)

    expect(result.status).toBe('BLOCKED')
    expect(result.reason).toBe('INCOMPLETE_REAL_CLI_EVIDENCE')
    expect(result.recordPaths).toHaveLength(4)
    expect(result.recordPaths).toEqual(plan.runs.map((run: any) => run.reportPath))

    for (const recordPath of result.recordPaths) {
      const record = JSON.parse(readFileSync(recordPath, 'utf8'))
      expect(record.status).toBe('BLOCKED')
      expect(record.environmentProbe).toEqual({
        hasOpenAi: false,
        hasAnthropic: false,
        hasClaudeOauth: false,
        hasExplicitTestToken: true,
        homeInsideTestRoot: true,
      })
    }
  })

  it('D20_Runner_RejectsMalformedPassBeforeComparison_09', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const value = config()
    materializeBinary(value.testRoot)
    value.drivers = materializeDriver(value.testRoot, [
      "import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'",
      "import { dirname } from 'node:path'",
      "const args = Object.fromEntries(process.argv.slice(2).reduce((rows, item, index, all) => {",
      "  if (item.startsWith('--')) rows.push([item.slice(2), all[index + 1]])",
      "  return rows",
      "}, []))",
      "const fixture = JSON.parse(readFileSync(args.fixture, 'utf8'))",
      "mkdirSync(dirname(args.report), { recursive: true })",
      "writeFileSync(args.report, JSON.stringify({",
      "  schemaVersion: 1, caseId: 'NATIVE-63', evidenceLayer: 'C', status: 'PASS',",
      "  runId: fixture.runId, lane: fixture.lane, observer: fixture.observer, target: null,",
      "}), { flag: 'wx' })",
    ].join('\n'))
    const result = executeD20Matrix(prepareD20Matrix(value))

    expect(result.status).toBe('FAIL')
    expect(result.reason).toBe('INVALID_REAL_CLI_EVIDENCE:TARGET_IDENTITY_REQUIRED')
    expect(result.failedRunId).toBe('codex-cc-desk-off-d20-runner-nonce-123')
    expect(JSON.stringify(result)).not.toContain('explicit-authorized-test-token')
    expect(JSON.stringify(result)).not.toContain('keep literal')
  })

  it('D20_Runner_DriverStdoutIsFailureAndNeverEchoed_10', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const value = config()
    materializeBinary(value.testRoot)
    value.drivers = materializeDriver(value.testRoot, [
      "process.stdout.write('driver-secret-output-must-not-escape')",
      blockedDriver(),
    ].join('\n'))
    const result = executeD20Matrix(prepareD20Matrix(value))

    expect(result.status).toBe('FAIL')
    expect(result.reason).toBe('REAL_CLI_DRIVER_STDOUT_FORBIDDEN')
    expect(JSON.stringify(result)).not.toContain('driver-secret-output-must-not-escape')
  })

  it.skipIf(process.platform === 'win32')(
    'D20_Runner_ExecutionRejectsSymlinkEscape_11',
    async () => {
      const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
      const value = config()
      const outside = root()
      const outsideBinary = join(outside, 'codex')
      writeFileSync(outsideBinary, 'outside binary', 'utf8')
      mkdirSync(join(value.testRoot, 'bin'), { recursive: true })
      symlinkSync(outsideBinary, value.binaryPath)
      value.drivers = materializeDriver(value.testRoot, blockedDriver())
      const result = executeD20Matrix(prepareD20Matrix(value))

      expect(result).toEqual({
        status: 'BLOCKED',
        cli: 'codex',
        reason: 'REAL_CLI_BINARY_NOT_ISOLATED',
        recordPaths: [],
      })
    },
  )


  it('D20_Runner_AcceptsPassWhenEvidenceMatchesActualBinaryHash_12', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const value = config()
    materializeBinary(value.testRoot)
    value.drivers = materializeDriver(
      value.testRoot,
      passDriver("createHash('sha256').update(readFileSync(args.binary)).digest('hex')"),
    )
    const result = executeD20Matrix(prepareD20Matrix(value))

    expect(result).toEqual({
      status: 'PASS',
      cli: 'codex',
      caseId: 'NATIVE-63',
      recordPaths: expect.arrayContaining([
        join(value.testRoot, 'runs', 'codex', 'cc-desk-off', 'evidence.json'),
        join(value.testRoot, 'runs', 'codex', 'cc-desk-on', 'evidence.json'),
        join(value.testRoot, 'runs', 'codex', 'system-terminal-off', 'evidence.json'),
        join(value.testRoot, 'runs', 'codex', 'system-terminal-on', 'evidence.json'),
      ]),
    })
  })

  it('D20_Runner_RejectsForgedBinaryIdentityEvenWhenFourRecordsAgree_13', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const value = config()
    const binaryPath = materializeBinary(value.testRoot)
    const actualHash = createHash('sha256').update(readFileSync(binaryPath)).digest('hex')
    expect(actualHash).not.toBe('a'.repeat(64))
    value.drivers = materializeDriver(value.testRoot, passDriver("'a'.repeat(64)"))
    const result = executeD20Matrix(prepareD20Matrix(value))

    expect(result).toEqual({
      status: 'FAIL',
      reason: 'REAL_CLI_BINARY_HASH_MISMATCH',
      failedRunId: 'codex-cc-desk-off-d20-runner-nonce-123',
    })
  })

})
