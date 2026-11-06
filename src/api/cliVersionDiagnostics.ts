import { parseU64 } from '@/utils/nativeIdentity'
import type { NativeCliKind } from '@/types/cli'

export const VERSION_PROBE_REASONS = ['program-unavailable', 'runner-unavailable', 'unsupported-launcher', 'profile-changed',
  'timeout', 'output-limit', 'invalid-output', 'command-failed', 'document-revoked', 'termination-unknown', 'probe-in-progress', 'unavailable'] as const
export type VersionProbeReason = typeof VERSION_PROBE_REASONS[number]
export type VersionProbeRequest = { requestId: string; profileId: string; expectedProfileRevision: string; cli: NativeCliKind }
type Identity = { requestId: string; profileId: string; profileRevision: string; cli: NativeCliKind }
export type VersionProbeReceipt = Identity & (
  | { state: 'known'; version: string; reason: null }
  | { state: 'unknown'; version: null; reason: VersionProbeReason }
  | { state: 'cancelled'; version: null; reason: null }
)
export interface VersionDiagnosticsBridge { readonly instanceId: string; invoke(command: string, payload: unknown): Promise<unknown> }
export interface VersionDiagnosticsClient {
  probe(request: VersionProbeRequest): Promise<VersionProbeReceipt>
  cancel(requestId: string): Promise<{ status: 'requested' | 'settled' }>
}

function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === 'object' && !Array.isArray(value) }
const fail = (code: string): never => { throw new Error(code) }
const requestIdValid = (value: string) => /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value)
export function isDiagnosticVersion(value: unknown): value is string {
  return typeof value === 'string' && value.length <= 64
    && /^(?:0|[1-9]\d{0,8})\.(?:0|[1-9]\d{0,8})\.(?:0|[1-9]\d{0,8})(?:-[0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)?$/.test(value)
}
export function isVersionProbeReason(value: unknown): value is VersionProbeReason {
  return VERSION_PROBE_REASONS.some(reason => reason === value)
}
function receipt(value: unknown, query: VersionProbeRequest): VersionProbeReceipt {
  if (!record(value) || Object.keys(value).length !== 7 || value.requestId !== query.requestId || value.profileId !== query.profileId
    || value.profileRevision !== query.expectedProfileRevision || value.cli !== query.cli) return fail('VERSION_PROBE_RESPONSE_INVALID')
  const identity: Identity = { requestId: query.requestId, profileId: query.profileId, profileRevision: query.expectedProfileRevision, cli: query.cli }
  if (value.state === 'known' && isDiagnosticVersion(value.version) && value.reason === null) return { ...identity, state: 'known', version: value.version, reason: null }
  if (value.state === 'unknown' && value.version === null && isVersionProbeReason(value.reason)) return { ...identity, state: 'unknown', version: null, reason: value.reason }
  if (value.state === 'cancelled' && value.version === null && value.reason === null) return { ...identity, state: 'cancelled', version: null, reason: null }
  return fail('VERSION_PROBE_RESPONSE_INVALID')
}

export function createVersionDiagnosticsClient(bridge: VersionDiagnosticsBridge, current: () => boolean = () => true): VersionDiagnosticsClient {
  const instance = bridge.instanceId
  function assertCurrent() {
    if (!instance || bridge.instanceId !== instance || !current()) fail('DOCUMENT_BRIDGE_UNAVAILABLE')
  }
  async function invoke(command: string, payload: unknown): Promise<unknown> {
    assertCurrent()
    let value: unknown
    try { value = await bridge.invoke(command, payload) }
    catch { assertCurrent(); return fail('VERSION_PROBE_UNAVAILABLE') }
    assertCurrent()
    return value
  }
  return {
    async probe(query) {
      if (!requestIdValid(query.requestId) || !/^[A-Za-z0-9_-]{1,128}$/.test(query.profileId)
        || !['claude', 'codex', 'opencode'].includes(query.cli)) return fail('VERSION_PROBE_REQUEST_INVALID')
      try { parseU64(query.expectedProfileRevision) } catch { return fail('VERSION_PROBE_REQUEST_INVALID') }
      const value = await invoke('native_probe_cli_version', { requestId: query.requestId, profileId: query.profileId, expectedProfileRevision: query.expectedProfileRevision })
      return receipt(value, query)
    },
    async cancel(requestId) {
      if (!requestIdValid(requestId)) return fail('VERSION_PROBE_REQUEST_INVALID')
      const value = await invoke('native_cancel_cli_version_probe', { requestId })
      if (!record(value) || Object.keys(value).length !== 1 || value.status !== 'requested' && value.status !== 'settled') return fail('VERSION_PROBE_RESPONSE_INVALID')
      return { status: value.status }
    },
  }
}

export function createNativeVersionDiagnosticsClient(): VersionDiagnosticsClient {
  const candidate: unknown = Reflect.get(window, '__CC_DESK_DOCUMENT__')
  if (!record(candidate) || typeof candidate.instanceId !== 'string' || typeof candidate.invoke !== 'function') return fail('DOCUMENT_BRIDGE_UNAVAILABLE')
  const instanceId = candidate.instanceId, invoke = candidate.invoke
  return createVersionDiagnosticsClient({ instanceId, invoke: async (command, payload) => {
    const value: unknown = await invoke.call(candidate, command, payload)
    return value
  } }, () => Reflect.get(window, '__CC_DESK_DOCUMENT__') === candidate && candidate.instanceId === instanceId)
}
