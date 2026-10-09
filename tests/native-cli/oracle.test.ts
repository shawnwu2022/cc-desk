import { spawnSync } from 'node:child_process'
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

const collectorPath = resolve(process.cwd(), 'scripts/native-cli/collect-prompt.mjs')
const inventoryPath = resolve(process.cwd(), 'docs/testing/native-cli-feature-inventory.json')
const protocolDecisionPath = resolve(process.cwd(), 'docs/testing/native-cli-protocol-decision.md')
const cleanup: string[] = []

function makeRoot(prefix: string): string {
  const root = mkdtempSync(join(tmpdir(), prefix))
  cleanup.push(root)
  return root
}

async function loadCollector() {
  expect(existsSync(collectorPath), 'collect-prompt.mjs must exist').toBe(true)
  return import(`${pathToFileURL(collectorPath).href}?case=${Date.now()}-${Math.random()}`)
}

function syntheticEnv(root: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    CC_DESK_SYNTHETIC_TEST_MODE: '1',
    CC_DESK_TEST_ROOT: root,
    CC_DESK_SECRET_SHOULD_NOT_APPEAR: 'synthetic-secret-never-persist',
  }
}

function codexFixture(overrides: Record<string, unknown> = {}) {
  return {
    nonce: 'synthetic-nonce-123',
    sessionId: 'thr_expected',
    turnId: 'turn_expected',
    expectedCwd: '/fixture/repo',
    expectedPrompt: 'synthetic-nonce-123\n你好\n<Pasted text 1>literal wrapper-like body</Pasted text 1>\n',
    transformId: 'codex-user-prompt-submit-v1-exact',
    ...overrides,
  }
}

function codexEvent(overrides: Record<string, unknown> = {}) {
  return {
    session_id: 'thr_expected',
    turn_id: 'turn_expected',
    cwd: '/fixture/repo',
    hook_event_name: 'UserPromptSubmit',
    prompt: codexFixture().expectedPrompt,
    model: 'fixture-model',
    ...overrides,
  }
}

afterEach(() => {
  while (cleanup.length > 0) {
    rmSync(cleanup.pop()!, { recursive: true, force: true })
  }
})

describe('native CLI prompt oracle contract', () => {
  it('D04_Oracle_CollectorExists_00', () => {
    expect(existsSync(collectorPath), 'collect-prompt.mjs must exist').toBe(true)
  })

  it('D04_Oracle_WrongSession_01', async () => {
    const { validatePromptEvent } = await loadCollector()
    expect(validatePromptEvent(
      codexEvent({ session_id: 'thr_other' }),
      codexFixture(),
      'codex',
    )).toEqual({ valid: false, reason: 'SESSION_MISMATCH' })
  })

  it('D04_Oracle_WrongTurn_02', async () => {
    const { validatePromptEvent } = await loadCollector()
    expect(validatePromptEvent(
      codexEvent({ turn_id: 'turn_other' }),
      codexFixture(),
      'codex',
    )).toEqual({ valid: false, reason: 'TURN_MISMATCH' })
  })

  it('D04_Oracle_WrongEventAndCwd_03', async () => {
    const { validatePromptEvent } = await loadCollector()
    expect(validatePromptEvent(
      codexEvent({ hook_event_name: 'Stop' }),
      codexFixture(),
      'codex',
    )).toEqual({ valid: false, reason: 'EVENT_MISMATCH' })
    expect(validatePromptEvent(
      codexEvent({ cwd: '/fixture/other' }),
      codexFixture(),
      'codex',
    )).toEqual({ valid: false, reason: 'CWD_MISMATCH' })
  })

  it('D04_Oracle_PreservesWrapperLikePromptExactly_04', async () => {
    const { validatePromptEvent } = await loadCollector()
    const fixture = codexFixture()
    expect(validatePromptEvent(codexEvent(), fixture, 'codex')).toEqual({ valid: true })
    expect(validatePromptEvent(
      codexEvent({ prompt: fixture.expectedPrompt.trim() }),
      fixture,
      'codex',
    )).toEqual({ valid: false, reason: 'CONTENT_MISMATCH' })
  })

  it('D04_Oracle_ClaudeDoesNotInventTurnId_05', async () => {
    const { validatePromptEvent } = await loadCollector()
    const fixture = {
      nonce: 'claude-synthetic-nonce',
      sessionId: 'claude-session',
      expectedCwd: '/fixture/claude',
      expectedPrompt: 'claude-synthetic-nonce\nbody',
      transformId: 'claude-user-prompt-submit-envelope-v1',
    }
    const event = {
      session_id: 'claude-session',
      cwd: '/fixture/claude',
      hook_event_name: 'UserPromptSubmit',
      prompt: fixture.expectedPrompt,
    }
    expect(validatePromptEvent(event, fixture, 'claude')).toEqual({ valid: true })
  })

  it('D04_Oracle_CollectorIsSilentAndPersistsRawEnvelope_06', () => {
    const root = makeRoot('cc-desk-oracle-')
    const fixturePath = join(root, 'fixture.json')
    const reportPath = join(root, 'reports', 'result.json')
    mkdirSync(dirname(reportPath), { recursive: true })
    writeFileSync(fixturePath, JSON.stringify(codexFixture()), 'utf8')
    const event = codexEvent()

    const result = spawnSync(
      process.execPath,
      [
        collectorPath,
        '--cli', 'codex',
        '--fixture', fixturePath,
        '--report', reportPath,
      ],
      {
        env: syntheticEnv(root),
        input: JSON.stringify(event),
        encoding: 'utf8',
      },
    )

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')
    const report = JSON.parse(readFileSync(reportPath, 'utf8'))
    expect(report.validation).toEqual({ valid: true })
    expect(report.rawEnvelope).toEqual(event)
    expect(report.transformId).toBe('codex-user-prompt-submit-v1-exact')
    expect(report.cli).toBe('codex')
    expect(JSON.stringify(report)).not.toContain('synthetic-secret-never-persist')
    expect(JSON.stringify(report)).not.toContain('additionalContext')
  })

  it('D04_Oracle_InvalidEventReportsWithoutInfluencingCli_07', () => {
    const root = makeRoot('cc-desk-oracle-invalid-')
    const fixturePath = join(root, 'fixture.json')
    const reportPath = join(root, 'result.json')
    writeFileSync(fixturePath, JSON.stringify(codexFixture()), 'utf8')

    const result = spawnSync(
      process.execPath,
      [
        collectorPath,
        '--cli', 'codex',
        '--fixture', fixturePath,
        '--report', reportPath,
      ],
      {
        env: syntheticEnv(root),
        input: JSON.stringify(codexEvent({ session_id: 'wrong' })),
        encoding: 'utf8',
      },
    )

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')
    expect(JSON.parse(readFileSync(reportPath, 'utf8')).validation).toEqual({
      valid: false,
      reason: 'SESSION_MISMATCH',
    })
  })

  it('D04_Oracle_RequiresSyntheticModeAndTestRoot_08', () => {
    const root = makeRoot('cc-desk-oracle-guard-')
    const fixturePath = join(root, 'fixture.json')
    const reportPath = join(root, 'result.json')
    writeFileSync(fixturePath, JSON.stringify(codexFixture()), 'utf8')

    const result = spawnSync(
      process.execPath,
      [
        collectorPath,
        '--cli', 'codex',
        '--fixture', fixturePath,
        '--report', reportPath,
      ],
      {
        env: { ...process.env, CC_DESK_TEST_ROOT: root },
        input: JSON.stringify(codexEvent()),
        encoding: 'utf8',
      },
    )

    expect(result.status).not.toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toContain('synthetic test mode required')
    expect(existsSync(reportPath)).toBe(false)
  })

  it('D04_Oracle_FeatureInventoryMapsRequiredHostCapabilities_09', () => {
    expect(existsSync(inventoryPath), 'feature inventory must exist').toBe(true)
    const inventory = JSON.parse(readFileSync(inventoryPath, 'utf8'))
    expect(inventory.schemaVersion).toBe(1)
    expect(inventory.status).toBe('NOT_RUN')
    expect(inventory.features.length).toBeGreaterThanOrEqual(12)

    const categories = new Set(inventory.features.map((entry: { category: string }) => entry.category))
    for (const category of [
      'interactive',
      'editor',
      'image',
      'mcp',
      'extensions',
      'permissions',
      'authentication',
      'terminal-protocol',
      'background-process',
    ]) {
      expect(categories.has(category), `missing category ${category}`).toBe(true)
    }

    for (const entry of inventory.features) {
      expect(['claude', 'codex', 'shared']).toContain(entry.cli)
      expect(entry.hostDependencies.length).toBeGreaterThan(0)
      expect(entry.caseIds.length).toBeGreaterThan(0)
      expect(['NOT_RUN', 'BLOCKED', 'PASS', 'FAIL']).toContain(entry.status)
    }
  })

  it('D04_Oracle_ProtocolDecisionForbidsByteRegexGuessing_10', () => {
    expect(existsSync(protocolDecisionPath), 'protocol decision must exist').toBe(true)
    const text = readFileSync(protocolDecisionPath, 'utf8')
    expect(text).toContain('pending clipboard')
    expect(text).toContain('protocol reply')
    expect(text).toContain('禁止按字节正则猜测来源')
    expect(text).toContain('BLOCKED')
    expect(text).toContain('@xterm/xterm 5.5')
  })
})
