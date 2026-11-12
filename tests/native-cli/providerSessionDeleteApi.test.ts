import { describe, expect, it, vi } from 'vitest'
import { createSessionDeleteClient } from '@/api/nativeSessionDelete'
import type { SourceRef } from '@/types/nativeProjection'

const operationId = '20000000-0000-4000-8000-000000000011'
const previewId = '20000000-0000-4000-8000-000000000012'
const providers = [
  { cli: 'claude', nativeSessionId: '20000000-0000-4000-8000-000000000013', rootArchived: null, reason: 'DELETE_MAINTENANCE_UNAVAILABLE' },
  { cli: 'opencode', nativeSessionId: 'ses_ExactCase123', rootArchived: null, reason: 'DELETE_SERVICE_UNVERIFIED' },
] as const
const source = (cli: 'claude' | 'opencode'): SourceRef => ({ scopeId: 'scope', instanceId: 'instance', cli, sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '7', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '7', projectIds: ['project'] } })
const confirmation = { operationId, previewId, acknowledgePermanentDelete: true as const, acknowledgeNativeSubtree: true as const }

describe('provider deletion read-only diagnosis', () => {
  it('ProviderDeleteApi_ObservedIdentityNeverGrantsExecution_001', async () => {
    for (const provider of providers) {
      const response = { operationId, previewId: null, state: 'unsupported', scope: 'native-subtree', knownDescendantCount: null, ...provider }
      const invoke = vi.fn(async () => response)
      const client = createSessionDeleteClient({ instanceId: 'instance', invoke })
      const request = { operationId, source: source(provider.cli), sessionKey: 'fixture' }
      expect(await client.preview(request)).toEqual(response)
      await expect(client.confirm(confirmation)).rejects.toThrow('DELETE_PREVIEW_REQUIRED')
      await expect(client.inspect({ operationId })).rejects.toThrow('DELETE_PREVIEW_REQUIRED')
      expect(invoke.mock.calls).toEqual([['cli_preview_session_delete', request]])
    }
  })
  it('ProviderDeleteApi_RejectsTicketOrReadyForUnsupportedProvider_002', async () => {
    for (const provider of providers) for (const state of ['unsupported', 'blocked', 'ready']) {
      const invoke = vi.fn(async () => ({ operationId, previewId, state, scope: 'native-subtree', knownDescendantCount: null, ...provider, rootArchived: true, reason: state === 'ready' ? null : provider.reason }))
      const client = createSessionDeleteClient({ instanceId: 'instance', invoke })
      await expect(client.preview({ operationId, source: source(provider.cli), sessionKey: 'fixture' })).rejects.toThrow('DELETE_INVALID_RESPONSE')
      await expect(client.confirm(confirmation)).rejects.toThrow('DELETE_PREVIEW_REQUIRED')
      expect(invoke).toHaveBeenCalledTimes(1)
    }
  })
  it('ProviderDeleteApi_RejectsWrongProviderLocatorAndRawReason_003', async () => {
    for (const nativeSessionId of ['ses_', '../ses_x', 'ses_x/y', 'ses_x\n', 'ses_' + 'x'.repeat(253), providers[0].nativeSessionId]) {
      const client = createSessionDeleteClient({ instanceId: 'instance', invoke: async () => ({ operationId, previewId: null, state: 'unsupported', scope: 'native-subtree', knownDescendantCount: null, ...providers[1], nativeSessionId }) })
      await expect(client.preview({ operationId, source: source('opencode'), sessionKey: 'fixture' })).rejects.toThrow('DELETE_INVALID_RESPONSE')
    }
    for (const extra of [{ nativeSessionId: providers[1].nativeSessionId }, { reason: 'private error payload' }]) {
      const client = createSessionDeleteClient({ instanceId: 'instance', invoke: async () => ({ operationId, previewId: null, state: 'unsupported', scope: 'native-subtree', knownDescendantCount: null, ...providers[0], ...extra }) })
      await expect(client.preview({ operationId, source: source('claude'), sessionKey: 'fixture' })).rejects.toThrow('DELETE_INVALID_RESPONSE')
    }
  })
})
