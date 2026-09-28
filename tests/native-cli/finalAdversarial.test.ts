import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

const runnerPath = resolve(process.cwd(), 'scripts/native-cli/real-cli-runner.mjs')
const cleanup: string[] = []

async function loadRunner() {
  expect(existsSync(runnerPath)).toBe(true)
  return import(`${pathToFileURL(runnerPath).href}?case=${Date.now()}-${Math.random()}`)
}

function freshRoot(): string {
  const value = mkdtempSync(join(tmpdir(), 'cc-desk-final-adversarial-'))
  cleanup.push(value)
  return value
}

function materializeBinary(testRoot: string): string {
  const path = join(testRoot, 'bin', 'codex')
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, 'adversarial fixture binary', 'utf8')
  return path
}

function driverSource(mutation: 'none' | 'fixture' | 'cwd'): string {
  const mutate = mutation === 'fixture'
    ? [
        "fixture.nonce = 'attacker-selected-nonce'",
        "fixture.originalText = 'attacker-selected-nonce\\nsubstituted payload'",
        "fixture.hostPayloadBase64 = Buffer.from(fixture.originalText, 'utf8').toString('base64')",
        "fixture.fixtureSha256 = createHash('sha256').update(JSON.stringify({ nonce: fixture.nonce, originalText: fixture.originalText, hostPayloadBase64: fixture.hostPayloadBase64, transformId: fixture.transformId }), 'utf8').digest('hex')",
      ]
    : mutation === 'cwd'
      ? ["fixture.expectedCwd = dirname(args.fixture)"]
      : []

  return [
    "import { createHash } from 'node:crypto'",
    "import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'",
    "import { dirname } from 'node:path'",
    "const rows = []",
    "for (let index = 0; index < process.argv.length; index += 1) {",
    "  const item = process.argv[index]",
    "  if (item.startsWith('--')) rows.push([item.slice(2), process.argv[index + 1]])",
    "}",
    "const args = Object.fromEntries(rows)",
    "const fixture = JSON.parse(readFileSync(args.fixture, 'utf8'))",
    ...mutate,
    "const payload = Buffer.from(fixture.hostPayloadBase64, 'base64')",
    "const payloadSha256 = createHash('sha256').update(payload).digest('hex')",
    "const binarySha256 = createHash('sha256').update(readFileSync(args.binary)).digest('hex')",
    "const target = { targetId: 'adversarial-target', os: 'fixture-os', osBuild: 'fixture-build', arch: 'fixture-arch', cli: { kind: fixture.cli, version: 'fixture-1', binarySha256 } }",
    "const host = fixture.lane === 'cc-desk'",
    "  ? { kind: 'cc-desk', deskCommit: 'c'.repeat(40), xtermVersion: '5.5.0', webviewRuntime: 'fixture-webview', renderer: 'dom' }",
    "  : { kind: 'system-terminal', terminalProgram: 'fixture-terminal' }",
    "const hostPayloadEvidence = fixture.lane === 'cc-desk'",
    "  ? { kind: 'native-input-frame', bytesBase64: fixture.hostPayloadBase64, sha256: payloadSha256, frame: { runId: fixture.runId, generation: 1, inputSeq: '1', modeEpoch: '1' } }",
    "  : { kind: 'terminal-driver-write', bytesBase64: fixture.hostPayloadBase64, sha256: payloadSha256, driver: 'fixture-terminal-driver', writeSeq: '1' }",
    "const rawEnvelope = { session_id: 'fixture-session', turn_id: 'fixture-turn', cwd: fixture.expectedCwd, hook_event_name: 'UserPromptSubmit', prompt: payload.toString('utf8') }",
    "const record = {",
    "  schemaVersion: 1, caseId: 'NATIVE-63', evidenceLayer: 'C', status: 'PASS',",
    "  runId: fixture.runId, lane: fixture.lane, observer: fixture.observer, target, host, hostPayloadEvidence,",
    "  fixture: { fixtureSha256: fixture.fixtureSha256, nonce: fixture.nonce, originalText: fixture.originalText, hostPayloadBase64: fixture.hostPayloadBase64, transformId: fixture.transformId },",
    "  oracle: { kind: 'real-user-prompt-submit', schemaVersion: 1, cli: fixture.cli, runId: fixture.runId, lane: fixture.lane, observer: fixture.observer, transformId: fixture.transformId, validation: { valid: true }, sessionId: 'fixture-session', turnId: 'fixture-turn', cwd: fixture.expectedCwd, rawEnvelope },",
    "}",
    "mkdirSync(dirname(args.report), { recursive: true })",
    "writeFileSync(args.report, JSON.stringify(record), { flag: 'wx' })",
  ].join('\n')
}

function materializeDrivers(testRoot: string, mutation: 'none' | 'fixture' | 'cwd') {
  const directory = join(testRoot, 'drivers')
  mkdirSync(directory, { recursive: true })
  const source = driverSource(mutation)
  const ccDesk = join(directory, 'cc-desk.mjs')
  const systemTerminal = join(directory, 'system-terminal.mjs')
  writeFileSync(ccDesk, source, 'utf8')
  writeFileSync(systemTerminal, source, 'utf8')
  return { ccDesk, systemTerminal }
}

function config(mutation: 'none' | 'fixture' | 'cwd') {
  const testRoot = freshRoot()
  return {
    cli: 'codex',
    authorizedTestAccount: true,
    testRoot,
    binaryPath: materializeBinary(testRoot),
    drivers: materializeDrivers(testRoot, mutation),
    nonce: 'orchestrator-owned-nonce',
    originalText: 'orchestrator-owned-nonce\nexpected payload',
    transformId: 'codex-user-prompt-submit-v1-exact',
    hostEnv: { PATH: process.env.PATH ?? '' },
    testAccountEnv: {},
  }
}

afterEach(() => {
  while (cleanup.length > 0) {
    rmSync(cleanup.pop()!, { recursive: true, force: true })
  }
})

describe('D28 adversarial final review', () => {
  it('D28_D20_BaselinePlanStillPasses_01', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const result = executeD20Matrix(prepareD20Matrix(config('none')))
    expect(result.status).toBe('PASS')
  })

  it('D28_D20_SelfConsistentSubstitutedFixtureCannotPass_02', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const result = executeD20Matrix(prepareD20Matrix(config('fixture')))
    expect(result).toMatchObject({
      status: 'FAIL',
      reason: 'REAL_CLI_RECORD_FIXTURE_MISMATCH',
    })
  })

  it('D28_D20_OracleCwdMustMatchPlannedCellProject_03', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const result = executeD20Matrix(prepareD20Matrix(config('cwd')))
    expect(result).toMatchObject({
      status: 'FAIL',
      reason: 'REAL_CLI_RECORD_CWD_MISMATCH',
    })
  })

  it.skipIf(process.platform === 'win32')(
    'D28_D20_RunRootSymlinkEscapeIsRejected_04',
    async () => {
      const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
      const plan = prepareD20Matrix(config('none'))
      expect(plan.status).toBe('READY')
      const outside = freshRoot()
      const run = plan.runs[0]
      mkdirSync(dirname(run.runRoot), { recursive: true })
      symlinkSync(outside, run.runRoot, 'dir')

      expect(executeD20Matrix(plan)).toMatchObject({
        status: 'FAIL',
        reason: 'REAL_CLI_RUN_ROOT_NOT_ISOLATED',
        failedRunId: run.runId,
      })
    },
  )
})
