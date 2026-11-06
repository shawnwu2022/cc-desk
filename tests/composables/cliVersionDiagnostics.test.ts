import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { randomUUID } from 'node:crypto'
import { createApp, defineComponent, ref } from 'vue'
import { useCliVersionDiagnostics, type DiagnosticProfile } from '@/composables/useCliVersionDiagnostics'

const mounted: (() => void)[] = []
beforeEach(() => vi.stubGlobal('crypto', { randomUUID }))
afterEach(() => { mounted.splice(0).forEach(unmount => unmount()); Reflect.deleteProperty(window, '__CC_DESK_DOCUMENT__'); vi.unstubAllGlobals() })
function setup() {
  const profiles = ref<DiagnosticProfile[]>([{ cli: 'codex', id: 'private-id', revision: '7', name: 'SECRET profile /private/path' }])
  const active = ref(true), cancelStatus = ref<'requested' | 'settled'>('requested'), calls: { command: string; payload: any; finish: (value: unknown) => void; fail: (value: unknown) => void }[] = []
  Reflect.set(window, '__CC_DESK_DOCUMENT__', { instanceId: 'document-1', invoke: (command: string, payload: unknown) => {
    if (command === 'native_cancel_cli_version_probe') { calls.push({ command, payload, finish: () => {}, fail: () => {} }); return Promise.resolve({ status: cancelStatus.value }) }
    return new Promise((finish, fail) => calls.push({ command, payload, finish, fail }))
  } })
  let api!: ReturnType<typeof useCliVersionDiagnostics>
  const app = createApp(defineComponent({ setup() { api = useCliVersionDiagnostics(() => profiles.value, () => active.value); return () => null } }))
  const element = document.createElement('div'); document.body.append(element); app.mount(element)
  const unmount = () => { app.unmount(); element.remove() }; mounted.push(unmount)
  return { profiles, active, calls, api, unmount, cancelStatus }
}
function known(call: ReturnType<typeof setup>['calls'][number], version = '0.166.0') {
  return { requestId: call.payload.requestId, profileId: call.payload.profileId, profileRevision: call.payload.expectedProfileRevision, cli: 'codex', state: 'known', version, reason: null }
}
describe('selected profile version diagnostics', () => {
  it('VersionCollection_ExplicitBoundedAndRedacted_001', async () => {
    const { api, calls } = setup()
    expect(calls).toHaveLength(0)
    const pending = api.collect(); void api.collect()
    expect(calls).toHaveLength(1)
    expect(api.busy.value).toBe(true)
    calls[0]!.finish(known(calls[0]!)); await pending
    expect(api.summary.value.codex).toEqual({ source: 'selected-profile-1', state: 'known', version: '0.166.0', reason: null })
    expect(JSON.stringify(api.summary.value)).not.toMatch(/private|SECRET|revision|requestId/)
  })
  it('VersionCollection_SelectionChangeRevokesLateSource_002', async () => {
    const { api, calls, profiles } = setup(); const pending = api.collect()
    profiles.value = [{ cli: 'codex', id: 'another-profile', revision: '2', name: 'other' }]
    expect(calls[1]?.command).toBe('native_cancel_cli_version_probe')
    calls[0]!.finish(known(calls[0]!)); await pending
    expect(api.summary.value.codex).toMatchObject({ source: 'selected-profile-2', state: 'not-collected', version: null })
    const second = api.collect(); calls[2]!.finish(known(calls[2]!, '0.167.0')); await second
    expect(api.summary.value.codex).toMatchObject({ source: 'selected-profile-2', version: '0.167.0' })
  })
  it('VersionCollection_CancelAcknowledgementIsNotTerminal_003', async () => {
    const { api, calls } = setup(); const pending = api.collect()
    await api.cancel(); expect(api.busy.value).toBe(true)
    expect(api.rows.value[0]?.state).toBe('cancelling')
    calls[0]!.finish({ ...known(calls[0]!), state: 'cancelled', version: null }); await pending
    expect(api.rows.value[0]?.state).toBe('cancelled'); expect(api.busy.value).toBe(false)
  })
  it('VersionCollection_LateKnownAfterCancelIsDiscarded_004', async () => {
    const { api, calls } = setup(); const pending = api.collect(); await api.cancel()
    calls[0]!.finish(known(calls[0]!)); await pending
    expect(api.summary.value.codex).toMatchObject({ state: 'not-collected', version: null })
  })
  it('VersionCollection_UnknownSafeFailureAndRevisionFence_005', async () => {
    const { api, calls, profiles } = setup(); let pending = api.collect()
    calls[0]!.fail(new Error('TOKEN /private/path')); await pending
    expect(api.summary.value.codex).toMatchObject({ state: 'unknown', reason: 'unavailable' })
    expect(JSON.stringify(api.summary.value)).not.toMatch(/TOKEN|private/)
    pending = api.collect(); profiles.value = [{ ...profiles.value[0]!, revision: '8' }]
    calls[1]!.finish(known(calls[1]!)); await pending
    expect(api.summary.value.codex).toMatchObject({ state: 'not-collected', version: null })
  })
  it('VersionCollection_DeactivateCancelsWithoutAcceptingLateResult_006', async () => {
    const { api, calls, active } = setup(); const pending = api.collect(); active.value = false
    expect(calls[1]?.command).toBe('native_cancel_cli_version_probe')
    calls[0]!.finish(known(calls[0]!)); await pending
    expect(api.summary.value.codex).toMatchObject({ state: 'not-collected', version: null })
  })
  it('VersionCollection_TerminationUnknownRetainsExactOwnerUntilSettled_007', async () => {
    const { api, calls, cancelStatus } = setup(); const pending = api.collect()
    calls[0]!.finish({ ...known(calls[0]!), state: 'unknown', version: null, reason: 'termination-unknown' }); await pending
    expect(api.busy.value).toBe(true)
    await api.collect(); expect(calls).toHaveLength(1)
    await api.cancel(); expect(api.busy.value).toBe(true)
    cancelStatus.value = 'settled'; await api.cancel()
    expect(api.busy.value).toBe(false)
    expect(calls.filter(call => call.command === 'native_cancel_cli_version_probe').every(call => call.payload.requestId === calls[0]!.payload.requestId)).toBe(true)
    expect(api.summary.value.codex).toMatchObject({ state: 'cancelled', version: null })
  })
})
