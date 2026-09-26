import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { describe, expect, it } from 'vitest'

const orchestratorPath = resolve(process.cwd(), 'scripts/native-cli/real-cli-certify.mjs')

async function loadOrchestrator() {
  expect(existsSync(orchestratorPath), 'real-cli-certify.mjs must exist').toBe(true)
  return import(`${pathToFileURL(orchestratorPath).href}?case=${Date.now()}-${Math.random()}`)
}

describe('D20 real CLI certification orchestrator', () => {
  it('D20_Certify_ModuleExists_00', async () => {
    await loadOrchestrator()
  })

  it('D20_Certify_MissingProductConfigIsBlocked_01', async () => {
    const { runD20Certification } = await loadOrchestrator()
    const result = runD20Certification({})

    expect(result).toEqual({
      status: 'BLOCKED',
      reason: 'REAL_CLI_EVIDENCE_INCOMPLETE',
      blockedClis: ['claude', 'codex'],
      results: {
        claude: {
          status: 'BLOCKED',
          cli: 'claude',
          reason: 'REAL_CLI_CONFIG_REQUIRED',
          recordPaths: [],
        },
        codex: {
          status: 'BLOCKED',
          cli: 'codex',
          reason: 'REAL_CLI_CONFIG_REQUIRED',
          recordPaths: [],
        },
      },
    })
  })

  it('D20_Certify_UnauthorizedAccountsStayBlockedWithoutProbing_02', async () => {
    const { runD20Certification } = await loadOrchestrator()
    const result = runD20Certification({
      claude: { cli: 'claude', authorizedTestAccount: false },
      codex: { cli: 'codex', authorizedTestAccount: false },
    })

    expect(result.status).toBe('BLOCKED')
    expect(result.blockedClis).toEqual(['claude', 'codex'])
    expect(result.results.claude).toEqual({
      status: 'BLOCKED',
      cli: 'claude',
      reason: 'AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE',
      recordPaths: [],
    })
    expect(result.results.codex).toEqual({
      status: 'BLOCKED',
      cli: 'codex',
      reason: 'AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE',
      recordPaths: [],
    })
  })

  it('D20_Certify_RejectsCrossProductCliConfig_03', async () => {
    const { runD20Certification } = await loadOrchestrator()
    const result = runD20Certification({
      claude: { cli: 'codex', authorizedTestAccount: false },
      codex: { cli: 'claude', authorizedTestAccount: false },
    })

    expect(result).toEqual({
      status: 'FAIL',
      reason: 'REAL_CLI_CERTIFICATION_CONFIG_INVALID',
      failedClis: ['claude', 'codex'],
      results: {
        claude: {
          status: 'FAIL',
          reason: 'CLI_CONFIG_KIND_MISMATCH',
        },
        codex: {
          status: 'FAIL',
          reason: 'CLI_CONFIG_KIND_MISMATCH',
        },
      },
    })
  })

  it('D20_Certify_DoesNotExposeConfigSecretsInBlockedResult_04', async () => {
    const { runD20Certification } = await loadOrchestrator()
    const result = runD20Certification({
      claude: {
        cli: 'claude',
        authorizedTestAccount: false,
        testAccountEnv: { CLAUDE_CODE_OAUTH_TOKEN: 'd20-secret-claude' },
      },
      codex: {
        cli: 'codex',
        authorizedTestAccount: false,
        testAccountEnv: { OPENAI_API_KEY: 'd20-secret-codex' },
      },
    })

    const serialized = JSON.stringify(result)
    expect(serialized).not.toContain('d20-secret-claude')
    expect(serialized).not.toContain('d20-secret-codex')
  })
})
