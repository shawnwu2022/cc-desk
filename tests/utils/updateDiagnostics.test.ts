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
})
