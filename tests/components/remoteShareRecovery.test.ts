import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { reactive } from 'vue'
import RemoteConnectionsView from '@/components/remote/RemoteConnectionsView.vue'
import { DESKTOP_PAIRING_PORT, type ApprovedDevice, type PairingSnapshot, type VerifiedPairing } from '@/components/remote/desktopPairingPort'
import en from '@/i18n/locales/en'

const fixture = vi.hoisted(() => ({ store: { tabs: new Map() } }))
vi.mock('@/stores/nativeTabs', () => ({ useNativeTabsStore: () => fixture.store }))

const phone: ApprovedDevice = { tunnel: 'fixture-tunnel', phoneId: 'fixture-abcdef', phoneRelayPublicKeySec1: 'fixture-relay-key', phoneEndpointSpki: 'fixture-endpoint-key', endpointBinding: 'fixture-binding', sharing: 'none' }
const snapshot: PairingSnapshot = { invite: null, pending: [], scopeChoices: [], devices: [phone] }
const mounted: Array<{ unmount(): void }> = []
afterEach(() => { while (mounted.length) mounted.pop()!.unmount() })
async function setup(ordinary = false) {
  fixture.store = reactive({ tabs: new Map([
    ['running', { tabId: 'running', status: 'running', cli: 'codex', runId: 'run-1', generation: 7, title: 'First' }],
    ['stopped', { tabId: 'stopped', status: 'stopped', cli: 'claude', runId: 'old', generation: 3, title: 'Inactive' }],
  ]) })
  const current = vi.fn<VerifiedPairing['current']>().mockResolvedValue(snapshot)
  const share = vi.fn<NonNullable<VerifiedPairing['share']>>().mockRejectedValue(new Error('private fixture detail'))
  const startOrdinaryCatalogueRead = vi.fn().mockResolvedValue(snapshot)
  const handle: VerifiedPairing = { endpoint: 'wss://relay.test/relay', current, share,
    ...(ordinary ? { startOrdinaryCatalogueRead } : {}),
    create: vi.fn(), cancel: vi.fn(), approve: vi.fn(), reject: vi.fn(), revoke: vi.fn() }
  const view = mount(RemoteConnectionsView, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })],
    provide: { [DESKTOP_PAIRING_PORT as symbol]: { verify: async () => ({ kind: 'verified', handle }) } },
  } })
  mounted.push(view)
  const button = (name: string) => {
    const match = view.findAll('button').find(b => b.text() === name)
    if (!match) throw new Error('Missing button: ' + name)
    return match
  }
  await view.get('input').setValue(handle.endpoint)
  await button(en.remotePairingVerify).trigger('click')
  await flushPromises()
  if (!ordinary) await view.get('input[value="running:7"]').setValue(true)
  return { view, current, share, button, startOrdinaryCatalogueRead }
}
describe('Remote share error recovery', () => {
  it('RemoteShare_ReadsAuthoritativeStateWithoutReplayingShare_001', async () => {
    const { view, current, share, button } = await setup()
    current.mockResolvedValueOnce({ ...snapshot, devices: [{ ...phone, sharing: 'offered' }] })
    await button(en.remotePairingShareStart).trigger('click')
    await flushPromises()
    expect(share).toHaveBeenCalledExactlyOnceWith(phone, [{ runId: 'run-1', generation: 7, label: 'Codex 1' }])
    expect(current).toHaveBeenCalledTimes(2)
    expect(view.text()).toContain('abcdef')
    expect(view.text()).toContain(en.remotePairingShareOffered)
    expect(view.text()).not.toContain('private fixture detail')
    expect(view.text()).not.toContain('Inactive')
  })
  it('RemoteShare_UnreadableStatusKeepsStaleActionsHidden_002', async () => {
    const { view, current, share, button } = await setup()
    current.mockRejectedValueOnce(new Error('private read detail'))
    await button(en.remotePairingShareStart).trigger('click')
    await flushPromises()
    expect(share).toHaveBeenCalledTimes(1)
    expect(current).toHaveBeenCalledTimes(2)
    expect(view.text()).not.toContain('abcdef')
    expect(view.text()).not.toContain('private read detail')
    expect(view.get('[role="alert"]').text()).toBe(en.remotePairingFailed)
  })
  it('RemoteShare_PersistsAliasWithoutChangingTheAuthorizedTarget_003', async () => {
    try {
      const initial = await setup()
      await initial.view.get('[data-device-alias-edit]').trigger('click')
      await initial.view.get('[data-device-alias-input]').setValue(' Test phone ')
      await initial.view.get('[data-device-alias-save]').trigger('click')
      expect(initial.view.get('[data-device-label]').text()).toBe('Test phone')
      mounted.pop()!.unmount()
      const reloaded = await setup()
      expect(reloaded.view.get('[data-device-label]').text()).toBe('Test phone')
      await reloaded.button(en.remotePairingShareStart).trigger('click')
      await flushPromises()
      expect(reloaded.share).toHaveBeenCalledExactlyOnceWith(phone, [{ runId: 'run-1', generation: 7, label: 'Codex 1' }])
    } finally {
      localStorage.clear()
    }
  })
  it('RemoteShare_OrdinaryCatalogueNeedsNoSessionProjectOrCliChoice_004', async () => {
    const { view, share, startOrdinaryCatalogueRead } = await setup(true)
    expect(view.find('input[value="running:7"]').exists()).toBe(false)
    expect(view.find('[data-advanced-share-options]').exists()).toBe(false)
    expect(startOrdinaryCatalogueRead).not.toHaveBeenCalled()
    await view.get('[data-start-ordinary-catalogue]').trigger('click')
    await flushPromises()
    expect(startOrdinaryCatalogueRead).toHaveBeenCalledExactlyOnceWith(phone)
    expect(share).not.toHaveBeenCalled()
    await view.get('[data-toggle-advanced-sharing]').trigger('click')
    expect(view.find('input[value="running:7"]').exists()).toBe(true)
  })
})
