import { describe, expect, it, vi } from 'vitest'
import { createOpencodeArchiveClient, type OpencodeArchiveRequest } from '@/api/opencodeArchive'
import type { SourceRef } from '@/types/nativeProjection'
const operationId = '70000000-0000-4000-8000-000000000002'
const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'opencode', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } }
const request = (): OpencodeArchiveRequest => ({ operationId, source, sessionKey: 'key', archived: true, acknowledgeRecoverable: true })
const receipt = () => ({ operationId, source, sessionKey: 'key', archived: true, state: 'archived' })
describe('OpenCode native archive API', () => {
  it('OpenCodeArchiveApi_ExactExplicitContract_001', async () => {
    const invoke = vi.fn(async () => receipt()), client = createOpencodeArchiveClient({ instanceId: 'instance', invoke })
    for (const extra of [{ archived: null }, { acknowledgeRecoverable: false }, { path: 'private' }, { operationId: 'invalid' }, { source: { ...source, cli: 'claude' } }]) {
      await expect(client.set({ ...request(), ...extra } as any)).rejects.toThrow('INVALID_REQUEST')
    }
    expect(invoke).not.toHaveBeenCalled()
    expect(await client.set(request())).toEqual(receipt())
    expect(invoke.mock.calls).toEqual([['cli_set_opencode_archive', request()]])
  })
  it('OpenCodeArchiveApi_RejectsMismatchedOrPrivateReceipts_002', async () => {
    for (const extra of [{ archived: false }, { state: 'deleted' }, { sessionKey: 'other' }, { operationId: '70000000-0000-4000-8000-000000000003' }, { privatePath: 'secret' }, { source: { ...source, scopeId: 'other' } }]) {
      const client = createOpencodeArchiveClient({ instanceId: 'instance', invoke: async () => ({ ...receipt(), ...extra }) })
      await expect(client.set(request())).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
    }
  })
  it('OpenCodeArchiveApi_UnknownNeverReplaysOrToggles_003', async () => {
    const invoke = vi.fn(async () => { throw { privateError: 'secret' } }), client = createOpencodeArchiveClient({ instanceId: 'instance', invoke })
    await expect(client.set(request())).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
    await expect(client.set({ ...request(), archived: false })).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
    expect(invoke).toHaveBeenCalledTimes(1)
  })
  it('OpenCodeArchiveApi_LostDocumentRejectsLateReceipt_004', async () => {
    let current = true
    const invoke = vi.fn(async () => { current = false; return receipt() }), client = createOpencodeArchiveClient({ instanceId: 'instance', invoke }, () => current)
    await expect(client.set(request())).rejects.toThrow('BACKEND_INSTANCE_CHANGED')
    await expect(client.set(request())).rejects.toThrow('BACKEND_INSTANCE_CHANGED')
    expect(invoke).toHaveBeenCalledTimes(1)
  })
  it('OpenCodeArchiveApi_ExplicitRestoreAndSafeErrors_005', async () => {
    const invoke = vi.fn(async () => ({ ...receipt(), archived: false }))
    await expect(createOpencodeArchiveClient({ instanceId: 'instance', invoke }).set({ ...request(), archived: false })).resolves.toMatchObject({ archived: false })
    for (const code of ['OPENCODE_ARCHIVE_UNSUPPORTED', 'ARCHIVE_SCHEMA_UNSUPPORTED', 'ARCHIVE_SOURCE_UNAVAILABLE', 'ARCHIVE_OPERATION_CONFLICT', 'NODE_RUNTIME_UNAVAILABLE', 'SDK_HELPER_UNAVAILABLE', 'SOURCE_BUSY', 'SCOPE_REVOKED']) {
      const client = createOpencodeArchiveClient({ instanceId: 'instance', invoke: async () => { throw { code, privatePath: 'secret' } } })
      await expect(client.set(request())).rejects.toThrow(code)
    }
  })
})
