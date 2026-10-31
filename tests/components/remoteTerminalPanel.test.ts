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
