import { describe, expect, it } from 'vitest'
import { safeUpdateDiagnostics } from '@/utils/updateDiagnostics'

describe('local update diagnostics', () => {
  it('UpdateDiagnostics_OnlySafeFields_001', () => {
    const summary = safeUpdateDiagnostics({ channel: 'test', state: 'verifying', candidate: '0.27.0-rc.1',
      failure: { code: 'UPDATER_SIGNATURE_INVALID', stage: 'verify', raw: 'https://TOKEN:SECRET@host/PROMPT_BODY' } })
    expect(summary).toMatchObject({ selectedChannel: 'test', phase: 'verifying', candidateVersion: '0.27.0-rc.1', failure: { code: 'UPDATER_SIGNATURE_INVALID', stage: 'verify' } })
    expect(JSON.stringify(summary)).not.toMatch(/TOKEN|SECRET|host|PROMPT_BODY|raw/)
    expect(summary.cliVersions.codex).toBe('not-collected')
  })
  it('UpdateDiagnostics_RejectsUnexpectedValues_002', () => {
    const summary = safeUpdateDiagnostics({ channel: 'TOKEN', state: '/PRIVATE_PATH', candidate: '0.27.0-SECRET', failure: new Error('PROMPT_BODY') })
    expect(summary).toMatchObject({ selectedChannel: 'unknown', phase: 'unknown', candidateVersion: 'unknown', failure: { code: 'UPDATER_FAILED', stage: 'unknown' } })
    expect(JSON.stringify(summary)).not.toMatch(/TOKEN|PRIVATE_PATH|SECRET|PROMPT_BODY/)
  })
  it('UpdateDiagnostics_CliVersionsKeepOnlySafeSelectedSource_003', () => {
    const summary = safeUpdateDiagnostics({ channel: 'stable', state: 'idle', candidate: null, failure: null, cliVersions: {
      codex: { source: 'selected-profile-2', state: 'known', version: '0.166.0', reason: null, profileId: 'SECRET', path: '/private' },
      claude: { source: '/private', state: 'unknown', version: 'SECRET', reason: 'timeout' },
      opencode: { source: 'selected-profile-3', state: 'unknown', version: null, reason: 'TOKEN' },
    } })
    expect(summary.cliVersions.codex).toEqual({ source: 'selected-profile-2', state: 'known', version: '0.166.0', reason: null })
    expect(summary.cliVersions.claude).toEqual({ source: null, state: 'unknown', version: null, reason: 'timeout' })
    expect(summary.cliVersions.opencode).toEqual({ source: 'selected-profile-3', state: 'unknown', version: null, reason: 'unavailable' })
    expect(JSON.stringify(summary)).not.toMatch(/SECRET|TOKEN|private|profileId/)
  })
})
