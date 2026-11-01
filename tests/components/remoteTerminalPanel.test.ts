import { afterAll, afterEach, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import RemoteTerminalPanel from '@/components/settings/RemoteTerminalPanel.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import { readFileSync } from 'node:fs'
// jsdom's Uint8Array and Node's inherited TextEncoder live in different realms.
// Match the browser's same-realm encoder without weakening production checks.
const originalEncoder = vi.hoisted(() => {
  const Original = globalThis.TextEncoder
  globalThis.TextEncoder = class extends Original {
    encode(input?: string) { return Uint8Array.from(super.encode(input)) }
  }
  return Original
})
afterAll(() => { globalThis.TextEncoder = originalEncoder })
let view: VueWrapper | undefined
afterEach(() => { view?.unmount(); delete (window as any).__CC_DESK_DOCUMENT__ })
it('RemoteTerminal_LockedNoAutomaticConnect_001', async () => {
  let calls = 0
  ;(window as any).__CC_DESK_DOCUMENT__ = { instanceId: 'own', async invoke(command: string) { calls++; expect(command).toBe('remote_client_current'); return { state: 'Locked' } } }
  view = mount(RemoteTerminalPanel, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  await flushPromises(); expect(calls).toBe(0)
  expect(view.text()).toContain('Locked')
  await view.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  expect(calls).toBe(1); expect(view.find('textarea').exists()).toBe(false)
})
it('RemoteTerminal_FixedSafeError_002', async () => {
  ;(window as any).__CC_DESK_DOCUMENT__ = { instanceId: 'own', async invoke() { throw Error('PRIVATE_URL_OR_INPUT') } }
  view = mount(RemoteTerminalPanel, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  await view.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  expect(view.text()).not.toContain('PRIVATE_URL_OR_INPUT'); expect(view.text()).toContain('unavailable')
})
it('RemoteTerminal_ActualReducerExactAckAndClose_003', async () => {
  expect(new TextEncoder().encode('x') instanceof Uint8Array).toBe(true)
  const frame = JSON.parse(readFileSync('protocol/remote/inner/v1/test-vectors/projection-snapshot.json', 'utf8'))
  const calls: string[] = []
  ;(window as any).__CC_DESK_DOCUMENT__ = { instanceId: 'own', async invoke(command: string, body: any) {
    calls.push(command)
    if (command === 'remote_client_current') return { state: 'Ready', resourceId: 7, target: frame.body.stamp.target, binding: frame.body.stamp.binding, role: 'Controller' }
    if (command === 'remote_client_snapshot') return { stamp: frame.body.stamp, parts: [Buffer.from(JSON.stringify(frame)).toString('base64url')] }
    if (command === 'remote_client_exchange') {
      const request = JSON.parse(Buffer.from(body.payload, 'base64url').toString())
      expect(request.message).toBe('ProjectionAck'); expect(request.stamp).toEqual(frame.body.stamp)
      return { payload: Buffer.from(JSON.stringify({ message: 'ProjectionAcknowledged', profile: 1, stamp: request.stamp })).toString('base64url') }
    }
    if (command === 'remote_client_close') return null
    throw Error('unexpected request')
  } }
  view = mount(RemoteTerminalPanel, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  await view.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  expect(calls).toEqual(['remote_client_current', 'remote_client_snapshot', 'remote_client_exchange'])
  expect(view.text()).toContain('Snapshot verified'); expect(view.find('[role="img"]').exists()).toBe(true)
  await view.get('[data-remote-disconnect]').trigger('click'); await flushPromises()
  expect(calls[calls.length - 1]).toBe('remote_client_close'); expect(view.find('[role="img"]').exists()).toBe(false)
})
it('RemoteTerminal_LateDiscoveryAfterUnmountDoesNotInstall_004', async () => {
  let finish!: (value: unknown) => void
  let calls = 0
  ;(window as any).__CC_DESK_DOCUMENT__ = { instanceId: 'own', invoke() { calls++; return new Promise(resolve => { finish = resolve }) } }
  view = mount(RemoteTerminalPanel, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  await view.get('[data-remote-refresh]').trigger('click'); view.unmount(); view = undefined
  finish({ state: 'Locked' }); await flushPromises(); expect(calls).toBe(1)
})

function inputFixture(role: 'Controller' | 'Viewer' = 'Controller', pending = false, loseReply = false, waitForWindow?: Promise<void>) {
  const frame = JSON.parse(readFileSync('protocol/remote/inner/v1/test-vectors/projection-snapshot.json', 'utf8'))
  let syncEpoch = BigInt(frame.body.stamp.syncEpoch)
  const messages: any[] = []
  ;(window as any).__CC_DESK_DOCUMENT__ = { instanceId: 'own', async invoke(command: string, body: any) {
    if (command === 'remote_client_current') return { state: 'Ready', resourceId: 7, target: frame.body.stamp.target, binding: frame.body.stamp.binding, role }
    if (command === 'remote_client_snapshot') { messages.push({ message: 'Snapshot' }); frame.body.stamp.syncEpoch = String(++syncEpoch); return { stamp: structuredClone(frame.body.stamp), parts: [Buffer.from(JSON.stringify(frame)).toString('base64url')] } }
    if (command === 'remote_client_close') return null
    const request = JSON.parse(Buffer.from(body.payload, 'base64url').toString())
    messages.push(request)
    let reply: unknown
    if (request.message === 'ProjectionAck') reply = { message: 'ProjectionAcknowledged', profile: 1, stamp: request.stamp }
    else if (request.message === 'InputCursor') reply = { message: 'InputCursorState', profile: 1, request: request.request, nextSequence: pending ? null : '41', pending: pending ? { operation: '07'.repeat(16), sequence: '40', outcome: 'Unknown', proof: null } : null }
    else if (request.message === 'Window') { await waitForWindow; reply = { message: 'WindowGranted', profile: 1, request: request.request, window: '08'.repeat(32), notAfterHostMs: '6000' } }
    else if (request.message === 'InputChunk') {
      if (loseReply) throw Error('PRIVATE_INPUT_REPLY_LOST')
      reply = { message: 'InputReceipt', profile: 1, operation: request.operation, sequence: request.sequence, outcome: 'HostWritten', proof: 'aa'.repeat(32) }
    } else if (request.message === 'ReceiptAck') reply = { message: 'ReceiptSettled', profile: 1, request: request.request, operation: request.operation, sequence: request.sequence, outcome: request.outcome, disposition: 'Advanced' }
    else throw Error('unexpected request')
    return { payload: Buffer.from(JSON.stringify(reply)).toString('base64url') }
  } }
  view = mount(RemoteTerminalPanel, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  return messages
}
it('RemoteTerminal_ControllerUsesHostSequenceAndExactReceipt_005', async () => {
  const messages = inputFixture()
  await view!.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  messages.length = 0
  await view!.get('[data-remote-input]').setValue('test\n')
  await view!.get('[data-remote-send]').trigger('click'); await flushPromises()
  expect(messages.map(x => x.message)).toEqual(['InputCursor', 'Snapshot', 'ProjectionAck', 'Window', 'InputChunk', 'ReceiptAck', 'Snapshot', 'ProjectionAck'])
  const sent = messages.find(x => x.message === 'InputChunk')
  expect(sent.sequence).toBe('41'); expect(Buffer.from(sent.bytes, 'base64url').toString()).toBe('test\n')
  expect(messages[5].operation).toBe(sent.operation); expect(messages[5].proof).toBe('aa'.repeat(32))
  expect(view!.text()).toContain('Host write confirmed')
  expect((view!.get('[data-remote-input]').element as HTMLTextAreaElement).value).toBe('')
  await view!.get('[data-remote-disconnect]').trigger('click'); await flushPromises()
  expect(view!.text()).not.toContain('Host write confirmed')
})
it('RemoteTerminal_ViewerCannotSubmit_006', async () => {
  const messages = inputFixture('Viewer')
  await view!.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  expect(view!.find('[data-remote-input]').exists()).toBe(false)
  expect(messages.some(x => x.message === 'Window' || x.message === 'InputChunk')).toBe(false)
})
it('RemoteTerminal_UnknownHostReceiptNeverResendsDraft_007', async () => {
  const messages = inputFixture('Controller', true)
  await view!.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  await view!.get('[data-remote-input]').setValue('keep me')
  await view!.get('[data-remote-send]').trigger('click'); await flushPromises()
  expect(messages.filter(x => x.message === 'InputCursor')).toHaveLength(1)
  expect(messages.some(x => x.message === 'Window' || x.message === 'InputChunk')).toBe(false)
  expect(view!.text()).toContain('outcome is uncertain')
})
it('RemoteTerminal_LostInputReplyDoesNotClaimUnsentOrRetry_008', async () => {
  const messages = inputFixture('Controller', false, true)
  await view!.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  await view!.get('[data-remote-input]').setValue('once')
  await view!.get('[data-remote-send]').trigger('click'); await flushPromises()
  expect(messages.filter(x => x.message === 'InputChunk')).toHaveLength(1)
  expect(messages.some(x => x.message === 'ReceiptAck')).toBe(false)
  expect(view!.text()).not.toContain('no input was sent')
  expect(view!.text()).not.toContain('PRIVATE_INPUT_REPLY_LOST')
  expect(view!.find('[data-remote-send]').exists()).toBe(false)
})
it('RemoteTerminal_OversizedUtf8DraftHasNoNetworkEffects_009', async () => {
  const messages = inputFixture()
  await view!.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  messages.length = 0
  await view!.get('[data-remote-input]').setValue('界'.repeat(2731))
  await view!.get('[data-remote-send]').trigger('click'); await flushPromises()
  expect(messages).toEqual([])
})
it('RemoteTerminal_UnmountDuringWindowNeverSubmitsLateInput_010', async () => {
  let release!: () => void
  const messages = inputFixture('Controller', false, false, new Promise<void>(resolve => { release = resolve }))
  await view!.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  await view!.get('[data-remote-input]').setValue('late')
  await view!.get('[data-remote-send]').trigger('click'); await flushPromises()
  expect(messages.filter(x => x.message === 'Window')).toHaveLength(1)
  view!.unmount(); view = undefined
  release(); await flushPromises()
  expect(messages.some(x => x.message === 'InputChunk')).toBe(false)
})
it('RemoteTerminal_OverlappingSendUsesOneOriginalOperation_011', async () => {
  let release!: () => void
  const messages = inputFixture('Controller', false, false, new Promise<void>(resolve => { release = resolve }))
  await view!.get('[data-remote-refresh]').trigger('click'); await flushPromises()
  await view!.get('[data-remote-input]').setValue('single')
  await view!.get('[data-remote-send]').trigger('click'); await flushPromises()
  await view!.get('[data-remote-send]').trigger('click'); await flushPromises()
  expect(messages.filter(x => x.message === 'InputCursor')).toHaveLength(1)
  release(); await flushPromises()
  expect(messages.filter(x => x.message === 'InputChunk')).toHaveLength(1)
})
