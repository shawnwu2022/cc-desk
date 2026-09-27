import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

const commandPath = resolve(process.cwd(), 'scripts/native-cli/run-real-cli-certification.mjs')
const cleanup: string[] = []

function tempRoot() {
  const path = mkdtempSync(join(tmpdir(), 'cc-desk-d20-command-'))
  cleanup.push(path)
  return path
}

async function loadCommand() {
  expect(existsSync(commandPath), 'run-real-cli-certification.mjs must exist').toBe(true)
  return import(`${pathToFileURL(commandPath).href}?case=${Date.now()}-${Math.random()}`)
}

afterEach(() => {
  while (cleanup.length > 0) rmSync(cleanup.pop()!, { recursive: true, force: true })
})

describe('D20 target-machine certification command', () => {
  it('D20_Command_ModuleExists_00', async () => {
    await loadCommand()
  })

  it('D20_Command_ResolvesExplicitAccountEnvReferences_01', async () => {
    const { resolveProductCommandConfig } = await loadCommand()
    const root = tempRoot()
    const resolved = resolveProductCommandConfig('codex', {
      cli: 'codex',
      authorizedTestAccount: true,
      testRoot: root,
      binaryPath: join(root, 'bin', 'codex'),
      drivers: {
        ccDesk: join(root, 'drivers', 'desk.mjs'),
        systemTerminal: join(root, 'drivers', 'system.mjs'),
      },
      nonce: 'd20-command-nonce',
      originalText: 'd20-command-nonce',
      transformId: 'codex-user-prompt-submit-v1-exact',
      testAccountEnv: {
        OPENAI_API_KEY: '${D20_CODEX_TEST_TOKEN}',
      },
    }, {
      PATH: '/safe/path',
      D20_CODEX_TEST_TOKEN: 'explicit-test-token',
      UNRELATED_SECRET: 'must-not-inherit',
    })

    expect(resolved.status).toBe('READY')
    expect(resolved.config.testAccountEnv).toEqual({
      OPENAI_API_KEY: 'explicit-test-token',
    })
    expect(resolved.config.hostEnv.PATH).toBe('/safe/path')
    expect(resolved.config.hostEnv.UNRELATED_SECRET).toBe('must-not-inherit')
  })

  it('D20_Command_RejectsPlaintextAccountSecrets_02', async () => {
    const { resolveProductCommandConfig } = await loadCommand()
    const resolved = resolveProductCommandConfig('claude', {
      cli: 'claude',
      authorizedTestAccount: true,
      testAccountEnv: {
        CLAUDE_CODE_OAUTH_TOKEN: 'plaintext-secret',
      },
    }, {})

    expect(resolved).toEqual({
      status: 'FAIL',
      reason: 'PLAINTEXT_TEST_ACCOUNT_ENV_FORBIDDEN',
    })
  })

  it('D20_Command_MissingReferencedAccountEnvIsBlocked_03', async () => {
    const { resolveProductCommandConfig } = await loadCommand()
    const resolved = resolveProductCommandConfig('codex', {
      cli: 'codex',
      authorizedTestAccount: true,
      testAccountEnv: {
        OPENAI_API_KEY: '${D20_CODEX_TEST_TOKEN}',
      },
    }, {})

    expect(resolved).toEqual({
      status: 'BLOCKED',
      cli: 'codex',
      reason: 'TEST_ACCOUNT_ENV_UNAVAILABLE',
      recordPaths: [],
    })
  })

  it('D20_Command_ResultNeverEchoesResolvedSecret_04', async () => {
    const { runD20CommandConfig } = await loadCommand()
    const root = tempRoot()
    const result = runD20CommandConfig({
      schemaVersion: 1,
      codex: {
        cli: 'codex',
        authorizedTestAccount: true,
        testRoot: root,
        binaryPath: join(root, 'bin', 'missing-codex'),
        drivers: {
          ccDesk: join(root, 'drivers', 'desk.mjs'),
          systemTerminal: join(root, 'drivers', 'system.mjs'),
        },
        nonce: 'd20-command-nonce',
        originalText: 'd20-command-nonce',
        transformId: 'codex-user-prompt-submit-v1-exact',
        testAccountEnv: {
          OPENAI_API_KEY: '${D20_CODEX_TEST_TOKEN}',
        },
      },
    }, {
      PATH: '/safe/path',
      D20_CODEX_TEST_TOKEN: 'explicit-test-token-must-not-escape',
    })

    expect(result.status).toBe('BLOCKED')
    expect(JSON.stringify(result)).not.toContain('explicit-test-token-must-not-escape')
  })

  it('D20_Command_CliUsesExitCode2ForBlockedWithoutStderrNoise_05', () => {
    const root = tempRoot()
    const configPath = join(root, 'config.json')
    writeFileSync(configPath, JSON.stringify({
      schemaVersion: 1,
      claude: { cli: 'claude', authorizedTestAccount: false },
      codex: { cli: 'codex', authorizedTestAccount: false },
    }))

    const result = spawnSync(process.execPath, [commandPath, '--config', configPath], {
      encoding: 'utf8',
      env: { ...process.env },
    })

    expect(result.status).toBe(2)
    expect(result.stderr).toBe('')
    const report = JSON.parse(result.stdout)
    expect(report.status).toBe('BLOCKED')
    expect(report.blockedClis).toEqual(['claude', 'codex'])
  })
})
