import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useCliProfilesStore } from '@/stores/cliProfiles'

function profile(id: string, cli: 'claude' | 'codex', revision = '1') {
  return {
    id,
    revision,
    cli,
    name: id,
    launcher: { kind: 'native' as const },
    programPath: { mode: 'inherit' as const },
    defaultArgs: { mode: 'inherit' as const },
    skipPermissions: { mode: 'inherit' as const },
    observer: { mode: 'inherit' as const },
    env: {},
  }
}

beforeEach(() => {
  clearMocks()
  setActivePinia(createPinia())
})

afterEach(() => clearMocks())

describe('D22 dual-CLI profile store', () => {
  it('D22_Profiles_LoadsClaudeAndCodexWithoutCollapsingIdentity_01', async () => {
    mockIPC((command) => {
      expect(command).toBe('cli_list_profiles')
      return {
        revision: '7',
        profiles: [
          profile('claude-main', 'claude', '3'),
          profile('codex-main', 'codex', '4'),
        ],
      }
    })

    const store = useCliProfilesStore()
    await store.load()

    expect(store.revision).toBe('7')
    expect(store.byCli.claude.map(p => p.id)).toEqual(['claude-main'])
    expect(store.byCli.codex.map(p => p.id)).toEqual(['codex-main'])
    expect(store.profile('claude-main')?.revision).toBe('3')
    expect(store.profile('codex-main')?.revision).toBe('4')
  })

  it('D22_Profiles_RejectsDuplicateProfileIdsAndDoesNotAdoptPartialList_02', async () => {
    mockIPC(() => ({
      revision: '2',
      profiles: [
        profile('same', 'claude'),
        profile('same', 'codex'),
      ],
    }))

    const store = useCliProfilesStore()
    await expect(store.load()).rejects.toMatchObject({
      code: 'INVALID_PROFILE_RESPONSE',
      retryable: false,
    })
    expect(store.profiles).toEqual([])
    expect(store.revision).toBe('0')
    expect(store.status).toBe('error')
  })

  it('D22_Profiles_RejectsShellFromNativeProductStore_03', async () => {
    mockIPC(() => ({
      revision: '1',
      profiles: [{ ...profile('shell', 'claude'), cli: 'shell' }],
    }))

    const store = useCliProfilesStore()
    await expect(store.load()).rejects.toMatchObject({
      code: 'INVALID_PROFILE_RESPONSE',
      retryable: false,
    })
    expect(store.profiles).toEqual([])
  })

  it('D22_Profiles_SelectsIndependentlyPerCliAndNeverCrossSelects_04', async () => {
    mockIPC(() => ({
      revision: '3',
      profiles: [
        profile('claude-a', 'claude'),
        profile('claude-b', 'claude'),
        profile('codex-a', 'codex'),
      ],
    }))
    const store = useCliProfilesStore()
    await store.load()

    store.select('claude', 'claude-b')
    store.select('codex', 'codex-a')

    expect(store.selected.claude?.id).toBe('claude-b')
    expect(store.selected.codex?.id).toBe('codex-a')
    expect(() => store.select('codex', 'claude-a')).toThrowError('PROFILE_CLI_MISMATCH')
  })

  it('D22_Profiles_ReconcilesRemovedSelectionWithoutFallingAcrossCli_05', async () => {
    let call = 0
    mockIPC(() => {
      call += 1
      if (call === 1) {
        return {
          revision: '1',
          profiles: [
            profile('claude-a', 'claude'),
            profile('claude-b', 'claude'),
            profile('codex-a', 'codex'),
          ],
        }
      }
      return {
        revision: '2',
        profiles: [
          profile('claude-a', 'claude'),
          profile('codex-a', 'codex'),
        ],
      }
    })

    const store = useCliProfilesStore()
    await store.load()
    store.select('claude', 'claude-b')
    store.select('codex', 'codex-a')
    await store.load()

    expect(store.selected.claude?.id).toBe('claude-a')
    expect(store.selected.codex?.id).toBe('codex-a')
  })

  it('D22_Profiles_OlderListReplyCannotEraseNewerMutation_06', async () => {
    let resolveList!: (value: unknown) => void
    mockIPC((command) => {
      if (command === 'cli_list_profiles') {
        return new Promise(resolve => { resolveList = resolve })
      }
      if (command === 'cli_patch_profile') {
        return {
          revision: '2',
          profiles: [
            profile('claude-a', 'claude', '2'),
            profile('codex-a', 'codex', '1'),
          ],
        }
      }
      throw new Error(command)
    })

    const store = useCliProfilesStore()
    const loading = store.load()
    await Promise.resolve()
    await store.patch('0', {
      op: 'create',
      profile: profile('claude-a', 'claude', '2'),
    })
    resolveList({
      revision: '1',
      profiles: [profile('codex-a', 'codex', '1')],
    })
    await loading

    expect(store.revision).toBe('2')
    expect(store.profile('claude-a')?.revision).toBe('2')
  })

  it('D22_Profiles_PatchUsesCallerExpectedRevisionAndNeverAutoRetriesConflict_07', async () => {
    let calls = 0
    let args: unknown
    mockIPC((command, value) => {
      expect(command).toBe('cli_patch_profile')
      calls += 1
      args = value
      throw { code: 'REVISION_CONFLICT', retryable: true }
    })

    const store = useCliProfilesStore()
    await expect(store.patch('9', {
      op: 'update',
      id: 'claude-main',
      changes: { name: 'Renamed' },
    })).rejects.toMatchObject({ code: 'REVISION_CONFLICT' })

    expect(calls).toBe(1)
    expect(args).toEqual({
      expectedRevision: '9',
      patch: { op: 'update', id: 'claude-main', changes: { name: 'Renamed' } },
    })
  })
})

// 删除已承认运行的保存配置不会改变准确 run 身份或停止当前进程。
it('LaunchConfig_DeleteAdmittedRun_008', async () => {
  const { useNativeTabsStore } = await import('@/stores/nativeTabs')
  let rows = [profile('cc', 'claude', '7')]; const commands: string[] = []; let revision = '7'
  mockIPC((command, payload) => { commands.push(command); if (command === 'cli_patch_profile') { revision = String(Number(revision) + 1); const patch = (payload as any).patch; rows = patch.op === 'delete' ? [] : rows.map(row => ({ ...row, revision, ...patch.changes })) }; return { revision, profiles: rows } })
  const profiles = useCliProfilesStore(); await profiles.load()
  const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'claude', projectId: 'p', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'new' } })
  tabs.applyLaunchStatus(tab.tabId, { instanceId: 'instance', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
  const before = JSON.stringify(tabs.tab(tab.tabId)); const active = tabs.activeTabId
  await profiles.saveConfiguration({ expectedRevision: '7', source: { id: 'cc', revision: '7' }, patch: { op: 'update', id: 'cc', changes: { name: 'Saved edit' } } }, () => true)
  expect(JSON.stringify(tabs.tab(tab.tabId))).toBe(before)
  expect(profiles.requestDelete('cc')).not.toBeNull(); expect(await profiles.confirmDelete()).toBe(true)
  expect(JSON.stringify(tabs.tab(tab.tabId))).toBe(before); expect(tabs.activeTabId).toBe(active)
  expect(commands).toEqual(['cli_list_profiles', 'cli_patch_profile', 'cli_patch_profile'])
})
// 删除最后一个默认仅清理全局偏好，安全默认配置在下一次明确新建时才准备。
it('LaunchConfig_LastDefaultFallback_009', async () => {
  localStorage.clear()
  const { useNewSessionDraftStore } = await import('@/stores/newSessionDraft')
  const { useProjectsStateStore } = await import('@/stores/projectsState')
  const rows = [profile('cc', 'claude', '7')]; const patches: unknown[] = []
  mockIPC((command, payload) => {
    if (command === 'cli_list_profiles') return { revision: '7', profiles: rows }
    if (command === 'cli_patch_profile') {
      const patch = (payload as any).patch; patches.push(patch)
      return { revision: patches.length === 1 ? '8' : '9', profiles: patch.op === 'create' ? [{ ...patch.profile, revision: '9' }] : [] }
    }
    if (command === 'cli_get_availability') return { profileId: 'desk-safe-claude', profileRevision: '9', cli: 'claude', state: 'configuration-required', hostStatus: 'available', certified: false, issue: { code: 'PROGRAM_TRUST_REQUIRED', retryable: false } }
    throw new Error('unexpected')
  })
  const profiles = useCliProfilesStore(); await profiles.load(); const defaults = useNewSessionDraftStore(); defaults.setDefault('claude', 'cc')
  profiles.requestDelete('cc'); expect(await profiles.confirmDelete()).toBe(true)
  expect(patches).toEqual([{ op: 'delete', id: 'cc' }]); expect(localStorage.getItem('cc-desk-launch-preferences-v1')).not.toContain('"cc"')
  useProjectsStateStore().loaded = true
  await expect(defaults.prepareInput({ projectKey: '/repo', projectPath: '/repo', cli: 'claude' })).rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
  expect(profiles.profile('desk-safe-claude')?.programPath).toEqual({ mode: 'inherit' }); expect(patches).toHaveLength(2)
  expect(patches[1]).toMatchObject({ op: 'create', profile: { cli: 'claude', observer: { mode: 'set', value: false }, defaultArgs: { mode: 'set', value: [] }, skipPermissions: { mode: 'set', value: false } } })
})

// 尚未分配 Native Tab 的新建准备也阻止删除它可能使用的配置。
it('LaunchConfig_BlocksPreparing_010', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  mockIPC(() => ({ revision: '7', profiles: [profile('cc', 'claude', '7')] }))
  const profiles = useCliProfilesStore(); await profiles.load()
  let release!: (value: any) => void
  const catalog = useUnifiedSessionsStore(); catalog.configureCreationPreparer(input => new Promise(resolve => { release = () => resolve(input) }))
  const preparing = catalog.createSession({ cli: 'claude', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'cc', launchConfigRevision: '7' }).catch(() => undefined)
  const allowed = profiles.requestDelete('cc')
  await catalog.closeSession(catalog.activeSessionId!); release(undefined); await preparing
  expect(allowed).toBeNull(); expect(profiles.deleteError?.detailCode).toBe('PROFILE_IN_USE')
})
// 已经排队的编辑在取消后不发写、不重读，也不污染保存状态。
it('LaunchConfig_CancelQueuedSave_011', async () => {
  const { flushPromises } = await import('@vue/test-utils')
  let release!: (value: unknown) => void; const commands: string[] = []
  mockIPC(command => { commands.push(command); if (command === 'cli_list_profiles') return { revision: '7', profiles: [profile('cc', 'claude', '7')] }; return new Promise(resolve => { release = resolve }) })
  const profiles = useCliProfilesStore(); await profiles.load()
  const previous = profiles.patch('7', { op: 'update', id: 'cc', changes: { name: 'First' } }); await flushPromises()
  let current = true
  const next = profiles.saveConfiguration({ expectedRevision: '7', source: { id: 'cc', revision: '7' }, patch: { op: 'update', id: 'cc', changes: { name: 'Canceled' } } }, () => current)
  const rejected = expect(next).rejects.toThrow('ACTION_CANCELLED'); current = false
  release({ revision: '8', profiles: [{ ...profile('cc', 'claude', '8'), name: 'First' }] }); await previous; await rejected
  expect(commands).toEqual(['cli_list_profiles', 'cli_patch_profile']); expect(profiles.lastError).toBeNull()
})
// 准确运行回执可在后续状态未知时保留删除权限，身份改变或未确认启动则不可以。
it('LaunchConfig_ReceiptOwnership_012', async () => {
  const { useNativeTabsStore } = await import('@/stores/nativeTabs')
  mockIPC(() => ({ revision: '7', profiles: [profile('cc', 'claude', '7')] }))
  const profiles = useCliProfilesStore(); await profiles.load(); const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'claude', projectId: 'p', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'new' } })
  tabs.applyLaunchStatus(tab.tabId, { instanceId: 'instance', requestId: tab.requestId, run: { runId: tab.runId, generation: 1 }, revision: '1', phase: 'indeterminate', failure: 'outcome-unknown' })
  expect(profiles.requestDelete('cc')).toBeNull()
  tabs.applyLaunchStatus(tab.tabId, { instanceId: 'instance', requestId: tab.requestId, run: { runId: tab.runId, generation: 1 }, revision: '2', phase: 'running', failure: null })
  tabs.markUnknown(tab.tabId); expect(profiles.requestDelete('cc')).not.toBeNull(); profiles.closeDeleteConfirmation()
  tabs.tab(tab.tabId)!.requestId = 'replacement'
  expect(profiles.requestDelete('cc')).toBeNull()
})


// 删除默认只选同 CLI 现有配置，不改写项目最近成功配置或发出额外创建。
it('LaunchConfig_DeleteDefaultSibling_013', async () => {
  localStorage.clear()
  const { useNewSessionDraftStore } = await import('@/stores/newSessionDraft')
  const { useProjectsStateStore } = await import('@/stores/projectsState')
  const initial = [profile('cc-a', 'claude'), profile('cc-b', 'claude'), profile('cx', 'codex')]; const commands: string[] = []
  mockIPC(command => { commands.push(command); return { revision: command === 'cli_list_profiles' ? '7' : '8', profiles: command === 'cli_list_profiles' ? initial : initial.filter(row => row.id !== 'cc-b') } })
  const profiles = useCliProfilesStore(); await profiles.load(); const defaults = useNewSessionDraftStore()
  defaults.setDefault('claude', 'cc-b'); defaults.setDefault('codex', 'cx')
  const projects = useProjectsStateStore(); projects.launchPreferences.set('/repo', { lastCli: 'claude', claudeLaunchConfigId: 'cc-b', codexLaunchConfigId: 'cx' })
  profiles.requestDelete('cc-b'); expect(await profiles.confirmDelete()).toBe(true)
  expect(defaults.defaultFor('claude')?.id).toBe('cc-a'); expect(defaults.defaultFor('codex')?.id).toBe('cx')
  expect(projects.launchPreferences.get('/repo')?.claudeLaunchConfigId).toBe('cc-b')
  expect(commands).toEqual(['cli_list_profiles', 'cli_patch_profile'])
})
