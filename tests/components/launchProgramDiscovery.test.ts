import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import LaunchProgramDiscovery from '@/components/sessions/LaunchProgramDiscovery.vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useShellStore } from '@/stores/shell'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'
import en from '@/i18n/locales/en'
import type { CliProfile } from '@/types/profile'

const io = vi.hoisted(() => ({ discover: vi.fn(), list: vi.fn(), patch: vi.fn() }))
vi.mock('@/api/programDiscovery', () => ({ cliDiscoverPrograms: io.discover }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.list, cliPatchProfile: io.patch }))
let wrapper: VueWrapper | undefined
let profile: CliProfile
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); localStorage.clear()
  profile = { id: 'p', revision: '2', cli: 'codex', name: 'Codex', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'set', value: [] }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: false }, env: {} }
  io.list.mockResolvedValue({ revision: '2', profiles: [profile] })
  io.patch.mockImplementation(async (_revision, patch) => ({ revision: '3', profiles: [{ ...profile, ...patch.changes, revision: '3' }] }))
  io.discover.mockResolvedValue({ profileId: 'p', profileRevision: '2', workspaceRevision: '2', projectId: 'project', cli: 'codex', candidates: [{ programPath: '/installed/codex', launcher: { kind: 'native' } }] })
  const projects = useWorkspaceStore(); projects.status = 'loaded'
  projects.projects = [{ projectId: 'project', hostId: 'local', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }]
})
afterEach(() => { wrapper?.unmount(); document.body.innerHTML = '' })
async function render() {
  await useCliProfilesStore().load()
  const catalog = useUnifiedSessionsStore()
  catalog.configureCreationPreparer(async () => { throw new LaunchConfigurationRequiredError('p', { code: 'PROGRAM_TRUST_REQUIRED' }) })
  await expect(catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })).rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
  wrapper = mount(LaunchProgramDiscovery, { attachTo: document.body, props: { session: catalog.activeSession! }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
  await flushPromises()
  return wrapper
}

it('ProgramDiscovery_AutomaticallyFindsAndConfirmsWithoutTyping_001', async () => {
  const view = await render()
  expect(io.discover).toHaveBeenCalledWith('p', '2', 'project')
  expect(view.text()).toContain('/installed/codex')
  expect(io.patch).not.toHaveBeenCalled()
  expect(view.emitted('confirmed')).toBeUndefined()
  await view.get('[data-program-confirm]').trigger('click'); await flushPromises()
  expect(io.patch).toHaveBeenCalledExactlyOnceWith('2', { op: 'update', id: 'p', changes: { programPath: { mode: 'set', value: '/installed/codex' }, launcher: { kind: 'native' } } })
  expect(view.emitted('confirmed')).toEqual([[{ sessionId: useUnifiedSessionsStore().activeSessionId, profileId: 'p', profileRevision: '3', canContinue: expect.any(Function) }]])
})

it('ProgramDiscovery_AbandonedSelectionCannotSaveOrLaunch_002', async () => {
  let resolve!: (value: unknown) => void
  io.discover.mockReturnValue(new Promise(done => { resolve = done }))
  const view = await render(); view.unmount()
  resolve({ profileId: 'p', profileRevision: '2', workspaceRevision: '2', projectId: 'project', cli: 'codex', candidates: [{ programPath: '/installed/codex', launcher: { kind: 'native' } }] })
  await flushPromises()
  expect(io.patch).not.toHaveBeenCalled()
  expect(view.emitted('confirmed')).toBeUndefined()
})

it.each(['navigation', 'selection', 'edit'] as const)('ProgramDiscovery_PendingSaveCancelledBy_%s', async action => {
  let resolve!: (value: unknown) => void
  io.patch.mockReturnValue(new Promise(done => { resolve = done }))
  const view = await render()
  await view.get('[data-program-confirm]').trigger('click'); await flushPromises()
  if (action === 'navigation') {
    useShellStore().navigate('settings'); useShellStore().navigate('workspace')
  } else if (action === 'selection') {
    const sessions = useUnifiedSessionsStore(), id = sessions.activeSessionId!
    sessions.selectProjectContext('/another'); await sessions.activateSession(id)
  } else await view.get('button').trigger('click')
  resolve({ revision: '3', profiles: [{ ...profile, revision: '3', programPath: { mode: 'set', value: '/installed/codex' } }] })
  await flushPromises()
  expect(view.emitted('confirmed')).toBeUndefined()
})

it('ProgramDiscovery_NewerProfileCannotReplaceConfirmedReceipt', async () => {
  let resolve!: (value: unknown) => void
  io.patch.mockReturnValue(new Promise(done => { resolve = done }))
  const view = await render()
  await view.get('[data-program-confirm]').trigger('click'); await flushPromises()
  io.list.mockResolvedValue({ revision: '4', profiles: [{ ...profile, revision: '4', programPath: { mode: 'set', value: '/different/codex' } }] })
  await useCliProfilesStore().load()
  resolve({ revision: '3', profiles: [{ ...profile, revision: '3', programPath: { mode: 'set', value: '/installed/codex' } }] })
  await flushPromises()
  expect(view.emitted('confirmed')).toBeUndefined()
})

it('ProgramDiscovery_LostSaveAcknowledgementNeverReplaysOrLaunches', async () => {
  const view = await render()
  io.patch.mockRejectedValue(new Error('NETWORK_UNKNOWN'))
  io.list.mockResolvedValue({ revision: '3', profiles: [{ ...profile, revision: '3', programPath: { mode: 'set', value: '/installed/codex' } }] })
  await view.get('[data-program-confirm]').trigger('click'); await flushPromises()
  expect(io.patch).toHaveBeenCalledTimes(1)
  expect(view.emitted('confirmed')).toBeUndefined()
})
