import { describe, expect, it } from 'vitest'
import { createVersionDiagnosticsClient } from '@/api/cliVersionDiagnostics'

const request = { requestId: '00000000-0000-4000-8000-000000000001', profileId: 'codex-one', expectedProfileRevision: '7', cli: 'codex' as const }
const receipt = () => ({ requestId: request.requestId, profileId: request.profileId, profileRevision: '7', cli: 'codex', state: 'known', version: '0.166.0', reason: null })

describe('profile-bound version diagnostics API', () => {
  it('VersionProbe_ExactScopeNoArgumentsOrPaths_001', async () => {
    const calls: unknown[] = []
    const client = createVersionDiagnosticsClient({ instanceId: 'test-instance', invoke: async (command, payload) => { calls.push([command, payload]); return receipt() } })
    expect(await client.probe(request)).toEqual(receipt())
    expect(calls).toEqual([['native_probe_cli_version', { requestId: request.requestId, profileId: 'codex-one', expectedProfileRevision: '7' }]])
  })
  it('VersionProbe_RejectsForeignOrSensitiveReceipt_002', async () => {
    for (const changed of [{ profileId: 'other' }, { profileRevision: '8' }, { cli: 'claude' }, { requestId: 'other' },
      { version: 'TOKEN secret' }, { raw: 'TOKEN secret' }, { state: 'unknown', version: null, reason: 'TOKEN secret' }]) {
      const client = createVersionDiagnosticsClient({ instanceId: 'test-instance', invoke: async () => ({ ...receipt(), ...changed }) })
      await expect(client.probe(request)).rejects.toThrow('VERSION_PROBE_RESPONSE_INVALID')
    }
  })
  it('VersionProbe_DocumentReplacementFencesLateResult_003', async () => {
    let finish!: (value: unknown) => void, current = true
    const client = createVersionDiagnosticsClient({ instanceId: 'test-instance', invoke: () => new Promise(resolve => { finish = resolve }) }, () => current)
    const pending = client.probe(request)
    current = false; finish(receipt())
    await expect(pending).rejects.toThrow('DOCUMENT_BRIDGE_UNAVAILABLE')
  })
  it('VersionProbe_CancelUsesOriginalRequestOnly_004', async () => {
    const calls: unknown[] = []
    const client = createVersionDiagnosticsClient({ instanceId: 'test-instance', invoke: async (command, payload) => { calls.push([command, payload]); return { status: 'requested' } } })
    expect(await client.cancel(request.requestId)).toEqual({ status: 'requested' })
    expect(calls).toEqual([['native_cancel_cli_version_probe', { requestId: request.requestId }]])
  })
})
