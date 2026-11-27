import { beforeEach, afterEach, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import RemoteConnectionsView from '@/components/remote/RemoteConnectionsView.vue'
import { DESKTOP_PAIRING_PORT, type PairingSnapshot, type VerifiedPairing } from '@/components/remote/desktopPairingPort'
import en from '@/i18n/locales/en'

vi.mock('@/stores/nativeTabs', () => ({ useNativeTabsStore: () => ({ tabs: new Map() }) }))
vi.mock('@/stores/workspace', () => ({ useWorkspaceStore: () => ({ projects: [
  { projectId: 'project-a', selectedPath: 'fixture/workspace-a', alias: { mode: 'set', value: 'Allowed workspace' }, hidden: { mode: 'inherit' } },
  { projectId: 'project-hidden', selectedPath: 'fixture/workspace-hidden', alias: { mode: 'set', value: 'Hidden workspace' }, hidden: { mode: 'set', value: true } },
] }) }))
vi.mock('@/stores/cliProfiles', () => ({ useCliProfilesStore: () => ({ selected: { codex: { id: 'profile-codex', revision: '4' }, opencode: { id: 'profile-opencode', revision: '7' }, claude: { id: 'profile-claude', revision: '8' } } }) }))
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


it('PairingCatalogue_RequiresOneRegisteredProjectAndRefreshesAfterTheOfferCloses', async () => {
  const device = { phoneId: 'fixture-phone', phoneRelayPublicKeySec1: 'relay-key',
    phoneEndpointSpki: 'endpoint-key', endpointBinding: 'binding', tunnel: 'fixture-tunnel',
    sharing: 'none' as const, revocationPending: false }
  const initial: PairingSnapshot = { invite: null, pending: [], scopeChoices: [], devices: [device] }
  const offered: PairingSnapshot = { ...initial, devices: [{ ...device, sharing: 'offered' }] }
  const current = vi.fn<VerifiedPairing['current']>().mockResolvedValue(initial)
  const shareCatalogue = vi.fn<NonNullable<VerifiedPairing['shareCatalogue']>>().mockResolvedValue(offered)
  const share = vi.fn<NonNullable<VerifiedPairing['share']>>()
  const handle: VerifiedPairing = { endpoint: 'wss://relay.test/relay', current, shareCatalogue, share,
    approve: vi.fn(), create: vi.fn(), cancel: vi.fn(), reject: vi.fn(), revoke: vi.fn() }
  view = mount(RemoteConnectionsView, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })],
    provide: { [DESKTOP_PAIRING_PORT as symbol]: { verify: async () => ({ kind: 'verified', handle }) } },
  } })
  await view.get('input').setValue(handle.endpoint)
  await view.findAll('button').find(b => b.text() === en.remotePairingVerify)!.trigger('click')
  await flushPromises()
  const button = () => view!.findAll('button').find(b => b.text() === en.remoteCatalogueStart)!
  expect(button().attributes('disabled')).toBeDefined()
  const options = view.findAll('select').find(control => control.find('option[value="project-a"]').exists())!
  expect(options.text()).toContain('Allowed workspace')
  expect(options.text()).not.toContain('Hidden workspace')
  await options.setValue('project-a')
  expect(button().attributes('disabled')).toBeUndefined()
  await button().trigger('click')
  await flushPromises()
  expect(shareCatalogue).toHaveBeenCalledExactlyOnceWith(device, {
    kind: 'profile', profileId: 'profile-codex', expectedProfileRevision: '4', projectIds: ['project-a'],
  })
  expect(share).not.toHaveBeenCalled()
  expect(view.text()).toContain(en.remotePairingShareOffered)
  await vi.advanceTimersByTimeAsync(3000)
  expect(current).toHaveBeenCalledTimes(2)
  expect(view.text()).not.toContain(en.remotePairingShareOffered)
  await view.findAll('select').find(control => control.find('option[value="opencode"]').exists())!.setValue('opencode')
  await view.findAll('button').find(b => b.text() === en.remoteCatalogueArchiveStart)!.trigger('click')
  await flushPromises()
  expect(shareCatalogue).toHaveBeenNthCalledWith(2, device, {
    kind: 'profile', profileId: 'profile-opencode', expectedProfileRevision: '7', projectIds: ['project-a'],
  }, 'archived')
  await vi.advanceTimersByTimeAsync(3000)
  await view.findAll('select').find(control => control.find('option[value="claude"]').exists())!.setValue('claude')
  await button().trigger('click')
  await flushPromises()
  expect(shareCatalogue).toHaveBeenNthCalledWith(3, device, {
    kind: 'profile', profileId: 'profile-claude', expectedProfileRevision: '8', projectIds: ['project-a'],
  })
  expect(share).not.toHaveBeenCalled()
})


it('PairingManagement_UsesAllEligibleSourcesWithoutSupplyingAProjectOrProfile', async () => {
  const device = { phoneId: 'fixture-phone', phoneRelayPublicKeySec1: 'relay-key',
    phoneEndpointSpki: 'endpoint-key', endpointBinding: 'binding', tunnel: 'fixture-tunnel',
    sharing: 'none' as const, revocationPending: false }
  const initial: PairingSnapshot = { invite: null, pending: [], scopeChoices: [], devices: [device] }
  const startManagement = vi.fn<NonNullable<VerifiedPairing['startManagement']>>().mockResolvedValue(initial)
  const handle: VerifiedPairing = { endpoint: 'wss://relay.test/relay', current: vi.fn().mockResolvedValue(initial),
    startManagement, approve: vi.fn(), create: vi.fn(), cancel: vi.fn(), reject: vi.fn(), revoke: vi.fn() }
  view = mount(RemoteConnectionsView, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })],
    provide: { [DESKTOP_PAIRING_PORT as symbol]: { verify: async () => ({ kind: 'verified', handle }) } },
  } })
  await view.get('input').setValue(handle.endpoint)
  await view.findAll('button').find(b => b.text() === en.remotePairingVerify)!.trigger('click')
  await flushPromises()
  const manage = view.findAll('button').find(b => b.text() === en.remoteManagementStart)!
  expect(manage.attributes('disabled')).toBeUndefined()
  await manage.trigger('click')
  await flushPromises()
  expect(startManagement).toHaveBeenCalledExactlyOnceWith(device)
})

it('PairingRelayAddress_ExplainsDisabledStateAndPreservesPath', async () => {
  const verify = vi.fn().mockResolvedValue({ kind: 'declined' })
  view = mount(RemoteConnectionsView, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })],
    provide: { [DESKTOP_PAIRING_PORT as symbol]: { verify } },
  } })
  const input = view.get('input')
  const hint = input.attributes('aria-describedby')
  expect(hint).toBeTruthy()
  expect(view.get('[id="' + hint + '"]').text()).toContain('wss://')
  const button = view.findAll('button').find(b => b.text() === en.remotePairingVerify)!
  await input.setValue('wss://')
  expect(button.attributes('disabled')).toBeDefined()
  expect(verify).not.toHaveBeenCalled()
  await input.setValue('  wss://relay.test/relay  ')
  expect(button.attributes('disabled')).toBeUndefined()
  await button.trigger('click')
  await flushPromises()
  expect(verify).toHaveBeenCalledExactlyOnceWith('wss://relay.test/relay')
})
