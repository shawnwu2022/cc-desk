import { existsSync, mkdtempSync, rmSync } from 'node:fs'
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
})
