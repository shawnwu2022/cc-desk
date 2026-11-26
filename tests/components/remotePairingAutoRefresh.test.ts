import { beforeEach, afterEach, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import RemoteConnectionsView from '@/components/remote/RemoteConnectionsView.vue'
import { DESKTOP_PAIRING_PORT, type PairingSnapshot, type VerifiedPairing } from '@/components/remote/desktopPairingPort'
import en from '@/i18n/locales/en'

vi.mock('@/stores/nativeTabs', () => ({ useNativeTabsStore: () => ({ tabs: new Map() }) }))
let view: VueWrapper | undefined
beforeEach(() => vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] }))
afterEach(() => { view?.unmount(); view = undefined; vi.useRealTimers() })

async function mountPairing(lifetime = 20000) {
  const invitation = { inviteId: 'fixture-invite', expiresUnixMs: Date.now() + lifetime }
  const initial: PairingSnapshot = { invite: invitation, pending: [], scopeChoices: [], devices: [] }
  const called: PairingSnapshot = { ...initial, pending: [{ inviteId: invitation.inviteId, requestId: 'fixture-request',
    phoneId: 'fixture-phone', phoneRelayPublicKeySec1: 'relay-key', phoneEndpointSpki: 'endpoint-key', endpointBinding: 'binding' }] }
  const current = vi.fn<VerifiedPairing['current']>().mockResolvedValue(initial)
  const approve = vi.fn<VerifiedPairing['approve']>()
  const handle: VerifiedPairing = { endpoint: 'wss://relay.test/relay', current, approve, create: vi.fn(), cancel: vi.fn(), reject: vi.fn(), revoke: vi.fn() }
  view = mount(RemoteConnectionsView, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })],
    provide: { [DESKTOP_PAIRING_PORT as symbol]: { verify: async () => ({ kind: 'verified', handle }) } },
  } })
  await view.get('input').setValue(handle.endpoint)
  await view.findAll('button').find(b => b.text() === en.remotePairingVerify)!.trigger('click')
  await flushPromises()
  return { current, approve, called }
}

it('PairingInvite_AutoFindsPendingWithoutApproving_001', async () => {
  const { current, approve, called } = await mountPairing()
  current.mockResolvedValueOnce(called)
  await vi.advanceTimersByTimeAsync(3000)
  expect(current).toHaveBeenCalledTimes(2)
  expect(view!.findAll('button').some(button => button.text() === en.remotePairingApprove)).toBe(true)
  await vi.advanceTimersByTimeAsync(9000)
  expect(current).toHaveBeenCalledTimes(2)
  expect(approve).not.toHaveBeenCalled()
})

it('PairingInvite_StopsAfterExpiryAndIgnoresLateRead_002', async () => {
  const { current, called } = await mountPairing(4500)
  let finish!: (value: PairingSnapshot) => void
  current.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  await vi.advanceTimersByTimeAsync(3000)
  expect(current).toHaveBeenCalledTimes(2)
  await vi.advanceTimersByTimeAsync(3000)
  finish(called); await flushPromises()
  expect(view!.findAll('button').some(button => button.text() === en.remotePairingApprove)).toBe(false)
  expect(current).toHaveBeenCalledTimes(2)
})
