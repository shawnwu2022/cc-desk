import { describe, expect, it, vi } from 'vitest'
import { createSessionDeleteClient } from '@/api/nativeSessionDelete'
import type { SourceRef } from '@/types/nativeProjection'

const operationId = '20000000-0000-4000-8000-000000000001'
const previewId = '20000000-0000-4000-8000-000000000002'
const nativeId = '20000000-0000-4000-8000-000000000003'
const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'codex', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '7', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '7', projectIds: ['project'] } }
const request = () => ({ operationId, source, sessionKey: 'synthetic-key' })
const preview = () => ({ operationId, previewId, nativeSessionId: nativeId, cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null })
const receipt = () => ({ operationId, cli: 'codex', state: 'acknowledged', rootPresent: false, descendants: 'unverified', reason: null })
const confirmation = () => ({ operationId, previewId, acknowledgePermanentDelete: true as const, acknowledgeNativeSubtree: true as const })

describe('permanent deletion authenticated protocol', () => {
  it('PermanentDeleteApi_PreviewNeverConfirms_001', async () => {
    const invoke = vi.fn(async () => preview())
    const client = createSessionDeleteClient({ instanceId: 'instance', invoke })
    expect(await client.preview(request())).toEqual(preview())
    expect(invoke.mock.calls).toEqual([['cli_preview_session_delete', request()]])
  })
  it('PermanentDeleteApi_RequiresBothAcknowledgementsAndIssuedPreview_002', async () => {
    const invoke = vi.fn(async () => preview())
    const client = createSessionDeleteClient({ instanceId: 'instance', invoke })
    await expect(client.confirm(confirmation())).rejects.toThrow()
    await client.preview(request())
    for (const partial of [{ acknowledgePermanentDelete: false }, { acknowledgeNativeSubtree: false }, { previewId: operationId }]) {
      await expect(client.confirm({ ...confirmation(), ...partial } as any)).rejects.toThrow()
    }
    expect(invoke).toHaveBeenCalledTimes(1)
  })
  it('PermanentDeleteApi_ReplacedDocumentCannotDelete_003', async () => {
    let current = true
    const invoke = vi.fn(async () => preview())
    const client = createSessionDeleteClient({ instanceId: 'instance', invoke }, () => current)
    await client.preview(request()); current = false
    await expect(client.confirm(confirmation())).rejects.toThrow('BACKEND_INSTANCE_CHANGED')
    expect(invoke).toHaveBeenCalledTimes(1)
  })
  it('PermanentDeleteApi_UnknownIsInspectedWithoutReplay_004', async () => {
    const invoke = vi.fn(async (command: string) => command === 'cli_preview_session_delete' ? preview() : command === 'cli_confirm_session_delete' ? { ...receipt(), state: 'unknown', rootPresent: null, reason: 'DELETE_OUTCOME_UNKNOWN' } : { ...receipt(), state: 'unknown', reason: 'DELETE_OUTCOME_UNKNOWN' })
    const client = createSessionDeleteClient({ instanceId: 'instance', invoke })
    await client.preview(request())
    expect((await client.confirm(confirmation())).state).toBe('unknown')
    await expect(client.confirm(confirmation())).rejects.toThrow('DELETE_OUTCOME_UNKNOWN')
    expect((await client.inspect({ operationId })).state).toBe('unknown')
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(['cli_preview_session_delete', 'cli_confirm_session_delete', 'cli_inspect_session_delete'])
  })
  it('PermanentDeleteApi_MalformedOrCrossOwnerResultIsUnknown_005', async () => {
    for (const invalid of [{ ...receipt(), operationId: previewId }, { ...receipt(), cli: 'claude' }, { ...receipt(), state: 'deleted' }, { ...receipt(), descendants: 'deleted' }, { ...receipt(), secret: 'private' }]) {
      const invoke = vi.fn(async (command: string) => command === 'cli_preview_session_delete' ? preview() : invalid)
      const client = createSessionDeleteClient({ instanceId: 'instance', invoke })
      await client.preview(request())
      const failure = await client.confirm(confirmation()).catch(error => error)
      expect(failure).toMatchObject({ code: 'DELETE_OUTCOME_UNKNOWN', retryable: false })
      expect(String(failure)).not.toContain('private')
      await expect(client.confirm(confirmation())).rejects.toThrow('DELETE_OUTCOME_UNKNOWN')
      expect(invoke).toHaveBeenCalledTimes(2)
    }
  })
  it('PermanentDeleteApi_UnsafeReadyAndUnknownReasonFailClosed_006', async () => {
    for (const invalid of [{ ...preview(), rootArchived: false }, { ...preview(), nativeSessionId: null }, { ...preview(), knownDescendantCount: 0 }, { ...preview(), reason: 'private arbitrary reason' }, { ...preview(), cli: 'opencode' }]) {
      const client = createSessionDeleteClient({ instanceId: 'instance', invoke: async () => invalid })
      await expect(client.preview(request())).rejects.toThrow()
    }
  })
  it('PermanentDeleteApi_DocumentLostBeforeDispatchCannotSend_007', async () => {
    let current = true
    const invoke = vi.fn(async (command: string) => command === 'cli_preview_session_delete' ? preview() : receipt())
    const client = createSessionDeleteClient({ instanceId: 'instance', invoke }, () => current)
    await client.preview(request())
    const task = client.confirm(confirmation()); current = false
    await task.catch(() => undefined)
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(['cli_preview_session_delete'])
  })
})
