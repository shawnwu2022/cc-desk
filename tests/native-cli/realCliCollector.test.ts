import { spawnSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

const collectorPath = resolve(process.cwd(), 'scripts/native-cli/collect-real-prompt.mjs')
const cleanup: string[] = []

function makeRoot(prefix: string): string {
  const root = mkdtempSync(join(tmpdir(), prefix))
  cleanup.push(root)
  return root
}

function env(root: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    CC_DESK_REAL_CLI_TEST_MODE: '1',
    CC_DESK_TEST_ROOT: root,
    CC_DESK_SECRET_SHOULD_NOT_APPEAR: 'real-collector-secret-never-persist',
  }
}

function fixture(
  root: string,
  cli: 'claude' | 'codex',
  overrides: Record<string, unknown> = {},
) {
  const cwd = join(root, 'repo')
  mkdirSync(cwd, { recursive: true })
  const payload = 'd20-real-nonce-123\n你好\n<pasted_content id="literal">keep literal</pasted_content id="literal">\n'
  return {
    schemaVersion: 1,
    cli,
    runId: `d20-${cli}-desk-off`,
    lane: 'cc-desk',
    observer: 'off',
    expectedCwd: cwd,
    nonce: 'd20-real-nonce-123',
    hostPayloadBase64: Buffer.from(payload, 'utf8').toString('base64'),
    transformId: cli === 'codex'
      ? 'codex-user-prompt-submit-v1-exact'
      : 'claude-user-prompt-submit-v1-exact',
    ...overrides,
  }
}

function eventFor(
  value: ReturnType<typeof fixture>,
  overrides: Record<string, unknown> = {},
) {
  const prompt = Buffer.from(value.hostPayloadBase64, 'base64').toString('utf8')
  return {
    session_id: 'session-real-d20',
    cwd: value.expectedCwd,
    hook_event_name: 'UserPromptSubmit',
    prompt,
    ...(value.cli === 'codex' ? { turn_id: 'turn-real-d20' } : {}),
    ...overrides,
  }
}

function writeFixture(root: string, value: ReturnType<typeof fixture>) {
  const path = join(root, 'fixture.json')
  writeFileSync(path, JSON.stringify(value), 'utf8')
  return path
}

function runCollector(
  root: string,
  cli: 'claude' | 'codex',
  fixturePath: string,
  reportPath: string,
  event: Record<string, unknown>,
  environment = env(root),
) {
  mkdirSync(dirname(reportPath), { recursive: true })
  return spawnSync(
    process.execPath,
    [
      collectorPath,
      '--cli', cli,
      '--fixture', fixturePath,
      '--report', reportPath,
    ],
    {
      env: environment,
      input: JSON.stringify(event),
      encoding: 'utf8',
    },
  )
}

afterEach(() => {
  while (cleanup.length > 0) {
    rmSync(cleanup.pop()!, { recursive: true, force: true })
  }
})

describe('D20 real UserPromptSubmit collector', () => {
  it('D20_RealCollector_ModuleExists_00', () => {
    expect(existsSync(collectorPath), 'collect-real-prompt.mjs must exist').toBe(true)
  })

  it('D20_RealCollector_RequiresExplicitRealTestMode_01', () => {
    const root = makeRoot('cc-desk-d20-real-mode-')
    const value = fixture(root, 'codex')
    const fixturePath = writeFixture(root, value)
    const reportPath = join(root, 'report.json')
    const result = runCollector(
      root,
      'codex',
      fixturePath,
      reportPath,
      eventFor(value),
      {
        ...process.env,
        CC_DESK_TEST_ROOT: root,
      },
    )

    expect(result.status).not.toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toContain('real CLI test mode required')
    expect(existsSync(reportPath)).toBe(false)
  })

  it('D20_RealCollector_RestrictsFixtureAndReportToTestRoot_02', () => {
    const root = makeRoot('cc-desk-d20-real-root-')
    const outside = makeRoot('cc-desk-d20-real-outside-')
    const value = fixture(root, 'codex')
    const fixturePath = writeFixture(root, value)
    const reportPath = join(outside, 'report.json')
    const result = runCollector(root, 'codex', fixturePath, reportPath, eventFor(value))

    expect(result.status).not.toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toContain('report path outside test root')
    expect(existsSync(reportPath)).toBe(false)
  })

  it('D20_RealCollector_CapturesCodexRawEnvelopeWithoutChangingHookOutput_03', () => {
    const root = makeRoot('cc-desk-d20-real-codex-')
    const value = fixture(root, 'codex')
    const fixturePath = writeFixture(root, value)
    const reportPath = join(root, 'reports', 'codex.json')
    const event = eventFor(value, { model: 'fixture-model' })
    const result = runCollector(root, 'codex', fixturePath, reportPath, event)

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')

    const report = JSON.parse(readFileSync(reportPath, 'utf8'))
    expect(report).toEqual({
      schemaVersion: 1,
      kind: 'real-user-prompt-submit',
      cli: 'codex',
      runId: value.runId,
      lane: 'cc-desk',
      observer: 'off',
      transformId: 'codex-user-prompt-submit-v1-exact',
      validation: { valid: true },
      sessionId: 'session-real-d20',
      turnId: 'turn-real-d20',
      cwd: value.expectedCwd,
      rawEnvelope: event,
    })
    expect(JSON.stringify(report)).not.toContain('real-collector-secret-never-persist')
  })

  it('D20_RealCollector_CodexRequiresActualTurnId_04', () => {
    const root = makeRoot('cc-desk-d20-real-turn-')
    const value = fixture(root, 'codex')
    const fixturePath = writeFixture(root, value)
    const reportPath = join(root, 'report.json')
    const event = eventFor(value)
    delete event.turn_id
    const result = runCollector(root, 'codex', fixturePath, reportPath, event)

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')
    expect(JSON.parse(readFileSync(reportPath, 'utf8')).validation).toEqual({
      valid: false,
      reason: 'CODEX_TURN_ID_REQUIRED',
    })
  })

  it('D20_RealCollector_ClaudePreservesStructuralPasteWrapper_05', () => {
    const root = makeRoot('cc-desk-d20-real-claude-')
    const basePayload = [
      'd20-real-nonce-123',
      'literal before',
      '<pasted_content id="literal">do not strip me</pasted_content id="literal">',
      'literal after',
      '',
    ].join('\n')
    const value = fixture(root, 'claude', {
      hostPayloadBase64: Buffer.from(basePayload, 'utf8').toString('base64'),
      transformId: 'claude-user-prompt-submit-pasted-content-v1',
    })
    const fixturePath = writeFixture(root, value)
    const reportPath = join(root, 'report.json')
    const wrapped = [
      '<pasted_content id="outer-d20">',
      basePayload,
      '</pasted_content id="outer-d20">',
    ].join('\n')
    const event = eventFor(value, { prompt: wrapped })
    const result = runCollector(root, 'claude', fixturePath, reportPath, event)

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')
    const report = JSON.parse(readFileSync(reportPath, 'utf8'))
    expect(report.validation).toEqual({ valid: true })
    expect(report.turnId).toBeNull()
    expect(report.rawEnvelope.prompt).toBe(wrapped)
  })

  it('D20_RealCollector_ContentMismatchIsEvidenceNotHookInterference_06', () => {
    const root = makeRoot('cc-desk-d20-real-mismatch-')
    const value = fixture(root, 'codex')
    const fixturePath = writeFixture(root, value)
    const reportPath = join(root, 'report.json')
    const result = runCollector(
      root,
      'codex',
      fixturePath,
      reportPath,
      eventFor(value, { prompt: 'changed' }),
    )

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')
    expect(JSON.parse(readFileSync(reportPath, 'utf8')).validation).toEqual({
      valid: false,
      reason: 'CONTENT_MISMATCH',
    })
  })

  it('D20_RealCollector_RejectsCliFixtureMismatchBeforeWriting_07', () => {
    const root = makeRoot('cc-desk-d20-real-cli-')
    const value = fixture(root, 'claude')
    const fixturePath = writeFixture(root, value)
    const reportPath = join(root, 'report.json')
    const result = runCollector(
      root,
      'codex',
      fixturePath,
      reportPath,
      eventFor(value),
    )

    expect(result.status).not.toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toContain('fixture CLI mismatch')
    expect(existsSync(reportPath)).toBe(false)
  })

  it('D20_RealCollector_ReportIsWriteOnceToPreventReplayOverwrite_08', () => {
    const root = makeRoot('cc-desk-d20-real-replay-')
    const value = fixture(root, 'codex')
    const fixturePath = writeFixture(root, value)
    const reportPath = join(root, 'report.json')
    const event = eventFor(value)

    const first = runCollector(root, 'codex', fixturePath, reportPath, event)
    const second = runCollector(root, 'codex', fixturePath, reportPath, event)

    expect(first.status).toBe(0)
    expect(second.status).not.toBe(0)
    expect(second.stdout).toBe('')
    expect(second.stderr).toContain('report already exists')
    expect(JSON.parse(readFileSync(reportPath, 'utf8')).runId).toBe(value.runId)
  })
})
