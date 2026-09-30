import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { useNewSessionDraftStore, argvFromLines } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useProjectsStateStore } from '@/stores/projectsState'
import type { ProjectsState } from '@/types/app'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import type { CliProfile } from '@/types/profile'
const io = vi.hoisted(() => ({ patch: vi.fn(), list: vi.fn(), availability: vi.fn(), getState: vi.fn(), setPreference: vi.fn() }))
vi.mock('@/api/tauri', () => ({ getProjectsState: io.getState, setProjectLaunchPreference: io.setPreference }))
vi.mock('@/api/cliAvailability', () => ({ cliGetAvailability: io.availability }))
vi.mock('@/api/cli', () => ({ cliPatchProfile: io.patch, cliListProfiles: io.list }))
const project = { projectKey: '/repo', projectPath: '/repo' }
function config(id: string, cli: 'claude' | 'codex' = 'codex'): CliProfile {
  return { id, cli, revision: '1', name: id, launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }
}
let persisted: ProjectsState
beforeEach(() => {
  localStorage.clear(); setActivePinia(createPinia()); vi.clearAllMocks()
  persisted = { pinnedProjects: [], archivedSessions: {}, launchPreferences: {} }
  io.getState.mockImplementation(async () => structuredClone(persisted))
  io.setPreference.mockImplementation(async (path, preference) => {
    persisted.launchPreferences![path] = structuredClone(preference)
    return structuredClone(persisted)
  })
})
describe('New session draft', () => {
  it('Draft_ExactArguments_001', () => {
    expect(argvFromLines('two words\r\n\n--literal= x\n"quoted"')).toEqual(['two words', '', '--literal= x', '"quoted"'])
    const draft = useNewSessionDraftStore(); draft.open(project, 'codex'); draft.rawEnabled = true; draft.argvText = 'two words\n';
    expect(draft.toInput().action).toEqual({ kind: 'raw', argv: ['two words', ''] })
    draft.setArgvFormat('json'); expect(draft.argvText).toBe('["two words",""]')
    draft.argvText = '["a\\nb", "", "two words"]'; expect(draft.toInput().action).toEqual({ kind: 'raw', argv: ['a\nb', '', 'two words'] })
    expect(() => draft.setArgvFormat('lines')).toThrow('ARGV_REQUIRES_JSON')
    expect(draft.argvFormat).toBe('json')
    draft.argvText = 'echo hi'; expect(() => draft.toInput()).toThrow('INVALID_RAW_ARGV_JSON')
  })
  it('Draft_PreferencePriority_002', async () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('default'), config('recent'), config('other')]; profiles.status = 'loaded'
    const draft = useNewSessionDraftStore(); draft.setDefault('codex', 'default')
    await draft.recordSuccess(project.projectPath, 'codex', 'recent')
    expect(draft.preferred(project, 'codex')?.id).toBe('recent')
    expect(draft.preferred({ projectPath: '/different' }, 'codex')?.id).toBe('default')
    profiles.profiles = profiles.profiles.filter(p => p.id !== 'recent')
    expect(draft.preferred(project, 'codex')?.id).toBe('default')
    draft.open(project, 'codex'); draft.launchConfigId = 'other'
    expect(draft.toInput()).toMatchObject({ launchConfigId: 'other', launchConfigRevision: '1' })
    expect(draft.preferred(project, 'codex')?.id).toBe('default')
  })
  it('Draft_SafeDefaultOnlyExplicit_003', async () => {
    const profiles = useCliProfilesStore(); profiles.status = 'loaded'
    const draft = useNewSessionDraftStore(); draft.open(project, 'claude')
    expect(draft.cliAvailability.claude).toBe('unknown'); expect(io.patch).not.toHaveBeenCalled()
    io.patch.mockImplementation(async (_rev, patch) => ({ revision: '1', profiles: [{ ...patch.profile, revision: '1' }] }))
    const result = await draft.prepareInput({ ...project, cli: 'claude' })
    expect(result.launchConfigId).toBeTruthy()
    expect(profiles.profile(result.launchConfigId!)?.skipPermissions).toEqual({ mode: 'set', value: false })
    expect(io.patch).toHaveBeenCalledTimes(1)
    const patch = io.patch.mock.calls[0][1]; expect(patch.profile.env).toEqual({}); expect(patch.profile.defaultArgs).toEqual({ mode: 'set', value: [] })
  })
  it('Draft_ConflictDoesNotReplay_004', async () => {
    useCliProfilesStore().status = 'loaded'; io.patch.mockRejectedValue({ code: 'REVISION_CONFLICT' }); io.list.mockResolvedValue({ revision: '2', profiles: [config('arrived')] })
    const draft = useNewSessionDraftStore()
    await expect(draft.prepareInput({ ...project, cli: 'codex' })).rejects.toThrow('NEW_SESSION_PREPARATION_FAILED')
    expect(io.patch).toHaveBeenCalledTimes(1); expect(io.list).toHaveBeenCalledTimes(1)
    expect(useCliProfilesStore().profiles[0].id).toBe('arrived')
  })
  it('Draft_OnlyVerifiedExecutableFailureDisables_005', () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('cx')]; profiles.status = 'loaded'
    const draft = useNewSessionDraftStore(); expect(draft.cliAvailability.claude).toBe('unknown')
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    tabs.markError(tab.tabId, 'PROGRAM_UNAVAILABLE')
    expect(draft.cliAvailability.codex).toBe('unavailable'); expect(draft.cliAvailability.claude).toBe('unknown')
  })
  it('Draft_AvailabilityUsesTheRequestedProject_006', async () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('cx'), config('broken')]; profiles.status = 'loaded'
    const draft = useNewSessionDraftStore(); await draft.recordSuccess('/other', 'codex', 'broken'); draft.open({ projectPath: '/other', projectKey: '/other' }, 'codex')
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'p', projectPath: '/other', profileId: 'broken', profileRevision: '1', action: { kind: 'new' } }); tabs.markError(tab.tabId, 'PROGRAM_UNAVAILABLE')
    expect(draft.availabilityFor(project).codex).toBe('unknown')
    expect(draft.availabilityFor({ projectPath: '/other' }).codex).toBe('unavailable')
  })

  it('Draft_PreflightUnavailableIsScopedAndNeverLaunchSuccess_007', async () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('cx'), config('cc', 'claude')]; profiles.status = 'loaded'
    io.availability.mockImplementation(async (id, revision) => ({ profileId: id, profileRevision: revision, cli: id === 'cx' ? 'codex' : 'claude', state: id === 'cx' ? 'unavailable' : 'available-unverified', hostStatus: 'available', certified: false, ...(id === 'cx' ? { issue: { code: 'PROGRAM_UNAVAILABLE', retryable: false } } : {}) }))
    const draft = useNewSessionDraftStore(); await draft.refreshAvailability()
    expect(draft.cliAvailability).toEqual({ claude: 'unknown', codex: 'unavailable' })
    expect(localStorage.getItem('cc-desk-launch-preferences-v1')).toBeNull()
    profiles.profiles[0].revision = '2'
    expect(draft.cliAvailability.codex).toBe('unknown')
  })

  it('Draft_ExplicitRefreshCanClearOldUnavailableEvidence_008', async () => {
    const now = vi.spyOn(Date, 'now').mockReturnValue(100)
    const profiles = useCliProfilesStore(); profiles.profiles = [config('cx')]; profiles.status = 'loaded'
    const draft = useNewSessionDraftStore(); const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } }); tabs.markError(tab.tabId, 'PROGRAM_UNAVAILABLE')
    expect(draft.cliAvailability.codex).toBe('unavailable')
    now.mockReturnValue(200); io.availability.mockResolvedValue({ profileId: 'cx', profileRevision: '1', cli: 'codex', state: 'available-unverified', hostStatus: 'available', certified: false })
    await draft.refreshAvailability(); expect(draft.cliAvailability.codex).toBe('unknown')
    now.mockRestore()
  })

  it('Draft_CanonicalPreloadedPreferenceWins_009', async () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('default'), config('canonical'), config('stale-local')]; profiles.status = 'loaded'
    localStorage.setItem('cc-desk-launch-preferences-v1', JSON.stringify({ recent: { '["/repo","codex"]': 'stale-local' }, defaults: { codex: 'default' } }))
    persisted.launchPreferences = { '/repo': { lastCli: 'codex', codexLaunchConfigId: 'canonical', claudeLaunchConfigId: 'untouched-claude' } }
    const draft = useNewSessionDraftStore()
    draft.open(project, 'codex')
    const pendingInput = draft.toInput()
    expect(pendingInput.launchConfigId).toBeUndefined()
    const input = await draft.prepareInput(pendingInput)
    expect(input.launchConfigId).toBe('canonical')
    expect(draft.preferred(project, 'codex')?.id).toBe('canonical')
    expect(io.setPreference).not.toHaveBeenCalled()
  })
  it('Draft_ConfirmedSuccessUsesCanonicalSingleWriter_010', async () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('default'), config('success')]
    persisted.launchPreferences = { '/repo': { lastCli: 'claude', claudeLaunchConfigId: 'existing-claude', codexLaunchConfigId: 'default' } }
    await useProjectsStateStore().load()
    const draft = useNewSessionDraftStore(); await draft.recordSuccess('/repo', 'codex', 'success')
    expect(io.setPreference).toHaveBeenCalledOnce()
    expect(persisted.launchPreferences?.['/repo']).toEqual({ lastCli: 'codex', claudeLaunchConfigId: 'existing-claude', codexLaunchConfigId: 'success' })
    expect(useProjectsStateStore().launchPreferences.get('/repo')?.codexLaunchConfigId).toBe('success')
    expect(localStorage.getItem('cc-desk-launch-preferences-v1')).toBeNull()
  })

  it('Draft_UnknownCommitReconcilesBeforeNextCliWrite_011', async () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('cc', 'claude'), config('cx')]
    useProjectsStateStore().loaded = true
    let releaseRecovery!: (value: ProjectsState) => void
    io.getState.mockReturnValue(new Promise<ProjectsState>(resolve => { releaseRecovery = resolve }))
    io.setPreference.mockImplementation(async (path, preference) => {
      persisted.launchPreferences![path] = structuredClone(preference)
      if (preference.lastCli === 'claude') throw new Error('lost acknowledgement after commit')
      return structuredClone(persisted)
    })
    const draft = useNewSessionDraftStore()
    const results = Promise.allSettled([draft.recordSuccess('/repo', 'claude', 'cc'), draft.recordSuccess('/repo', 'codex', 'cx')])
    await flushPromises(); const writesBeforeRecovery = io.setPreference.mock.calls.length
    releaseRecovery(structuredClone(persisted)); const settled = await results
    expect(writesBeforeRecovery).toBe(1)
    expect(settled.map(result => result.status)).toEqual(['rejected', 'fulfilled'])
    expect(persisted.launchPreferences?.['/repo']).toEqual({ lastCli: 'codex', claudeLaunchConfigId: 'cc', codexLaunchConfigId: 'cx' })
    expect(io.setPreference).toHaveBeenCalledTimes(2)
  })
  it('Draft_UnverifiedRecoveryBlocksQueuedPreferenceWrite_012', async () => {
    const profiles = useCliProfilesStore(); profiles.profiles = [config('cc', 'claude'), config('cx')]
    const projects = useProjectsStateStore(); projects.loaded = true
    io.getState.mockRejectedValue(new Error('read unavailable'))
    io.setPreference.mockImplementation(async (path, preference) => {
      persisted.launchPreferences![path] = structuredClone(preference)
      if (preference.lastCli === 'claude') throw new Error('lost acknowledgement after commit')
      return structuredClone(persisted)
    })
    const draft = useNewSessionDraftStore()
    const settled = await Promise.allSettled([draft.recordSuccess('/repo', 'claude', 'cc'), draft.recordSuccess('/repo', 'codex', 'cx')])
    expect(settled.map(result => result.status)).toEqual(['rejected', 'rejected'])
    expect(io.setPreference).toHaveBeenCalledOnce()
    expect(persisted.launchPreferences?.['/repo']?.claudeLaunchConfigId).toBe('cc')
    expect(projects.loaded).toBe(false)
  })

})
