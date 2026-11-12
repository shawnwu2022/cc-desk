import { describe, expect, it, vi } from 'vitest'
import { createSessionRecycleClient, type RecycleRequest } from '@/api/nativeSessionRecycle'
import type { SourceRef } from '@/types/nativeProjection'
const operationId = '70000000-0000-4000-8000-000000000002'
const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'claude', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } }
const record = { operationId, nativeSessionId: '70000000-0000-4000-8000-000000000001', projectPath: '/synthetic/project', state: 'stored', originalOccupied: false, scope: 'claude-root-transcript' }
const request = (): RecycleRequest => ({ source, action: 'remove', operationId, sessionKey: 'key', acknowledgeRecoverable: true })
describe('independent recoverable transcript operation', () => {
  it('NativeRecycleApi_RequiresExplicitScopeAndNeverCallsPermanentDelete_001', async () => {
    const invoke = vi.fn(async () => ({ records: [record], unavailableCount: 0 })), client = createSessionRecycleClient({ instanceId: 'instance', invoke })
    await expect(client.send({ ...request(), acknowledgeRecoverable: false })).rejects.toThrow('RECYCLE_INVALID_REQUEST')
    await expect(client.send({ ...request(), source: { ...source, cli: 'opencode' } })).rejects.toThrow('RECYCLE_INVALID_REQUEST')
    expect(invoke).not.toHaveBeenCalled()
    expect(await client.send(request())).toEqual({ records: [record], unavailableCount: 0 })
    expect(invoke.mock.calls).toEqual([['cli_recycle_session', request()]])
  })
  it('NativeRecycleApi_UnknownResponseIsNeverAutomaticallyReplayed_002', async () => {
    const invoke = vi.fn(async () => { throw { privateError: 'not published' } }), client = createSessionRecycleClient({ instanceId: 'instance', invoke })
    await expect(client.send(request())).rejects.toThrow('RECYCLE_OUTCOME_UNKNOWN')
    await expect(client.send(request())).rejects.toThrow('RECYCLE_OUTCOME_UNKNOWN')
    expect(invoke).toHaveBeenCalledTimes(1)
  })
  it('NativeRecycleApi_RejectsWrongOperationScopeAndUnexpectedPayload_003', async () => {
    for (const extra of [{ operationId: '70000000-0000-4000-8000-000000000003' }, { scope: 'native-subtree' }, { privatePath: 'secret' }, { state: 'deleted' }]) {
      const client = createSessionRecycleClient({ instanceId: 'instance', invoke: async () => ({ records: [{ ...record, ...extra }], unavailableCount: 0 }) })
      await expect(client.send(request())).rejects.toThrow('RECYCLE_OUTCOME_UNKNOWN')
    }
  })
  it('NativeRecycleApi_LostDocumentCannotAcceptLateResultOrSend_004', async () => {
    let current = true
    const invoke = vi.fn(async () => { current = false; return { records: [record], unavailableCount: 0 } })
    const client = createSessionRecycleClient({ instanceId: 'instance', invoke }, () => current)
    await expect(client.send(request())).rejects.toThrow('BACKEND_INSTANCE_CHANGED')
    await expect(client.send({ source, action: 'list', operationId: null, sessionKey: null, acknowledgeRecoverable: false })).rejects.toThrow('BACKEND_INSTANCE_CHANGED')
    expect(invoke).toHaveBeenCalledTimes(1)
  })
})
