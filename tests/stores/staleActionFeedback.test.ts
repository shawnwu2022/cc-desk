import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { mapSafeUserError } from '@/utils/userError'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useProjectsStateStore } from '@/stores/projectsState'
beforeEach(() => { clearMocks(); setActivePinia(createPinia()) })
afterEach(() => { clearMocks(); vi.unstubAllGlobals() })
// Object.prototype 的继承键不允许绕过安全错误映射。
it('Feedback_RejectsInheritedError_001', () => {
  for (const code of ['constructor', '__proto__', 'toString']) expect(mapSafeUserError(code, 'session').detailCode).toBe('GENERIC_UNAVAILABLE')
})
// 配置读取失败不把带凭据/原始路径的异常保存在可渲染状态中。
it('Feedback_ProfileErrorIsSanitized_002', async () => {
  mockIPC(() => { throw { code: 'REVISION_CONFLICT', message: '/private/TOKEN secret' } })
  const profiles = useCliProfilesStore(); await profiles.load().catch(() => {})
  expect(profiles.lastError).toBe('REVISION_CONFLICT')
})
// 索引写入回执未知后在单一 writer 中只读恢复，绝不自动重放归档。
it('Feedback_UnknownArchiveReloads_003', async () => {
  const commands: string[] = []; const state = useProjectsStateStore(); state.loaded = true
  mockIPC(command => { commands.push(command); if (command === 'archive_session') throw new Error('/private/TOKEN'); if (command === 'get_projects_state') return { pinnedProjects: [], archivedSessions: { '/repo': ['saved'] } }; throw new Error('unexpected') })
  await state.archiveSession('/repo', 'saved').catch(() => {})
  expect(commands).toEqual(['archive_session', 'get_projects_state']); expect(state.archivedSessions.get('/repo')).toEqual(['saved'])
})

// 未经类型化确认的配置删除不得穿透 CAS mutation API。
it('Feedback_ConfigDeleteRequiresConfirm_004', async () => {
  const calls: string[] = []
  const profile = { id: 'cx', revision: '7', cli: 'codex', name: 'Work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }
  mockIPC(command => { calls.push(command); return { revision: '7', profiles: command === 'cli_list_profiles' ? [profile] : [] } })
  const profiles = useCliProfilesStore(); await profiles.load()
  await expect(profiles.patch('7', { op: 'delete', id: 'cx' })).rejects.toThrow('CONFIRMATION_REQUIRED')
  expect(calls).toEqual(['cli_list_profiles'])
})

// 删除配置在确认时重新检查打开会话，成功后只执行一次既有 CAS 删除。
it('Feedback_ConfigDeleteChecksOwnership_005', async () => {
  const { useNativeTabsStore } = await import('@/stores/nativeTabs')
  const calls: unknown[] = []
  const profile = { id: 'cx', revision: '7', cli: 'codex', name: 'Work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }
  mockIPC((command, args) => { if (command === 'cli_list_profiles') return { revision: '7', profiles: [profile] }; calls.push(args); return { revision: '8', profiles: [] } })
  const profiles = useCliProfilesStore(); await profiles.load(); profiles.requestDelete('cx')
  const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  expect(await profiles.confirmDelete()).toBe(false); expect(profiles.deleteError?.detailCode).toBe('PROFILE_IN_USE'); expect(calls).toEqual([])
  tabs.close(tab.tabId); profiles.requestDelete('cx')
  expect(await profiles.confirmDelete()).toBe(true); expect(profiles.profile('cx')).toBeUndefined()
  expect(calls).toEqual([{ expectedRevision: '7', patch: { op: 'delete', id: 'cx' } }])
})
// 配置删除发生冲突，只读加载新版本；旧确认不能自动改用新 revision 重试。
it('Feedback_ConfigConflictNoReplay_006', async () => {
  const profile = { id: 'cx', revision: '7', cli: 'codex', name: 'Work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }
  const calls: string[] = []; let changed = false
  mockIPC(command => { calls.push(command); if (command === 'cli_patch_profile') { changed = true; throw { code: 'REVISION_CONFLICT', message: '/private/SECRET' } } return { revision: changed ? '8' : '7', profiles: [{ ...profile, revision: changed ? '8' : '7' }] } })
  const profiles = useCliProfilesStore(); await profiles.load(); profiles.requestDelete('cx')
  expect(await profiles.confirmDelete()).toBe(false); expect(profiles.deleteError?.detailCode).toBe('REVISION_CONFLICT'); expect(profiles.revision).toBe('8')
  expect(calls).toEqual(['cli_list_profiles', 'cli_patch_profile', 'cli_list_profiles'])
  expect(await profiles.confirmDelete()).toBe(false); expect(calls.filter(command => command === 'cli_patch_profile')).toHaveLength(1)
  expect(JSON.stringify(profiles.deleteError)).not.toContain('/private/SECRET')
})

// 已知注册身份在确认前被替换时，项目移除不能隐藏或注销替代身份。
it('Feedback_ProjectRemovePinsIdentity_007', async () => {
  const { useWorkspaceStore } = await import('@/stores/workspace')
  const { useProjectManagementStore } = await import('@/stores/projectManagement')
  const registry = useWorkspaceStore(); registry.projects = [{ projectId: 'original', hostId: 'h', sourcePathKey: 's', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }]; registry.status = 'loaded'
  const commands: string[] = []
  mockIPC(command => { commands.push(command); if (command === 'get_app_config') return { hiddenProjects: [] }; if (command === 'get_projects_state') return { pinnedProjects: [], archivedSessions: {} }; if (command === 'cli_list_projects') return { revision: '1', projects: registry.projects }; if (command === 'cli_remove_project') return { revision: '2', projects: [] }; return undefined })
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/repo', projectPath: '/repo' })
  registry.projects[0].projectId = 'replacement'
  await management.remove()
  expect(commands).not.toContain('update_app_config'); expect(commands).not.toContain('cli_remove_project')
})
// 取消等待只读前置检查的项目移除后，迟到完成不得继续写隐藏设置。
it('Feedback_ProjectCancelStopsWrites_008', async () => {
  const { useProjectManagementStore } = await import('@/stores/projectManagement')
  let finish!: (value: unknown) => void; const read = new Promise(resolve => { finish = resolve }); const commands: string[] = []
  mockIPC(command => { commands.push(command); if (command === 'get_app_config') return read; if (command === 'get_projects_state') return { pinnedProjects: [], archivedSessions: {} }; if (command === 'cli_list_projects') return { revision: '1', projects: [] }; return undefined })
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/repo', projectPath: '/repo' }); const pending = management.remove()
  management.closeDialog(); finish({ hiddenProjects: [] }); await pending
  expect(commands).not.toContain('update_app_config'); expect(management.error).toBeNull()
})

// Legacy 重命名返回时 PTY 已换代，不得把旧名称写入新尝试。
it('Feedback_LegacyRenameRejectsNewPty_009', async () => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy' })
  const { useSessionStore } = await import('@/stores/session'); const { createLegacyClaudeAdapter } = await import('@/session/adapters/legacyClaudeAdapter')
  const legacy = useSessionStore(); const id = legacy.createTab('/repo', { sessionId: 'saved', name: 'Original' }); const tab = legacy.tabs.get(id)!
  tab.ptyId = 'pty-1'; tab.ptyGeneration = 1; tab.status = 'running'
  let finish!: (value: unknown) => void; const pending = new Promise(resolve => { finish = resolve })
  const commands: string[] = []
  mockIPC(command => { commands.push(command); if (command === 'upsert_session_ui_record') return pending; throw new Error('unexpected') })
  const metadata = useProjectsStateStore(); metadata.loaded = true
  const runtimeRename = vi.fn()
  const adapter = createLegacyClaudeAdapter({ store: legacy, metadata, projectPaths: () => ['/repo'], runtime: { startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: runtimeRename } })
  const rename = adapter.renameSession('legacy-tab:' + id, 'Late name'); const rejected = expect(rename).rejects.toThrow('STALE_SESSION_ATTEMPT')
  await vi.waitFor(() => expect(commands).toEqual(['upsert_session_ui_record']))
  tab.ptyGeneration = 2; tab.ptyId = 'pty-2'; finish({ pinnedProjects: [], archivedSessions: {} }); await rejected
  expect(tab.name).toBe('Original')
  expect(runtimeRename).not.toHaveBeenCalled(); expect(commands).not.toContain('pty_input')
})
// Legacy 确认后的停止回执不能关闭同一个 Tab 下被替换的 PTY。
it('Feedback_LegacyClosePinsPty_010', async () => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy' })
  const { useSessionStore } = await import('@/stores/session'); const { createLegacyClaudeAdapter } = await import('@/session/adapters/legacyClaudeAdapter'); const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const legacy = useSessionStore(); const id = legacy.createTab('/repo', { sessionId: 'saved', name: 'Original' }); const tab = legacy.tabs.get(id)!
  tab.ptyId = 'pty-1'; tab.ptyGeneration = 1; tab.status = 'running'
  let finish!: () => void; const pending = new Promise<void>(resolve => { finish = resolve })
  const adapter = createLegacyClaudeAdapter({ store: legacy, projectPaths: () => ['/repo'], runtime: { startTab: vi.fn(), stopTab: () => pending, restartTab: vi.fn(), renameTab: vi.fn() } })
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([adapter]); await catalog.refresh()
  catalog.beginSessionConfirmation('close-running', 'legacy-tab:' + id); const closing = catalog.confirmSessionAction(); await Promise.resolve()
  tab.ptyId = 'pty-2'; finish(); await closing
  expect(legacy.tabs.get(id)?.ptyId).toBe('pty-2'); expect(catalog.sessionConfirmation).toBeNull()
})

// 注销排队期间取消确认、替换注册或打开会话，均须在实际 writer admission 拒绝后续写入。
it.each(['cancel', 'replace', 'open'] as const)('Feedback_ProjectQueueGuard_011_%s', async change => {
  const { flushPromises } = await import('@vue/test-utils')
  const { useAppStore } = await import('@/stores/app')
  const { useWorkspaceStore } = await import('@/stores/workspace')
  const { useProjectManagementStore } = await import('@/stores/projectManagement')
  const { useNativeTabsStore } = await import('@/stores/nativeTabs')
  const row = { projectId: 'p', hostId: 'h', sourcePathKey: 's', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }
  let hidden: string[] = []; const writes: string[] = []
  let finish!: (value: unknown) => void
  mockIPC((command, args) => {
    if (command === 'get_app_config') return { hiddenProjects: hidden }
    if (command === 'update_app_config') { hidden = (args as { updates: { hiddenProjects: string[] } }).updates.hiddenProjects; return }
    if (command === 'get_projects_state') return { pinnedProjects: ['/repo'], archivedSessions: {} }
    if (command === 'cli_list_projects') return { revision: '1', projects: [row] }
    if (command === 'cli_patch_project') return new Promise(resolve => { finish = resolve })
    if (command === 'cli_remove_project') { writes.push(command); return { revision: '3', projects: [] } }
    if (command === 'unpin_project') { writes.push(command); return { pinnedProjects: [], archivedSessions: {} } }
    throw new Error('unexpected')
  })
  const registry = useWorkspaceStore(); await registry.load()
  const state = useProjectsStateStore(); await state.ensureLoaded()
  const app = useAppStore(); await app.loadProjectVisibility()
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/repo', projectPath: '/repo' })
  const earlier = registry.patch('p', { alias: { mode: 'set', value: 'New name' } }); await flushPromises()
  const removing = management.remove(); await flushPromises()
  expect(app.isHidden('/repo')).toBe(true); expect(writes).toEqual([])
  if (change === 'cancel') management.closeDialog()
  if (change === 'open') useNativeTabsStore().create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
  finish({ revision: '2', projects: [{ ...row, sourcePathKey: change === 'replace' ? 'replacement' : 's' }] })
  await earlier; await removing
  expect(writes, 'invalidated queued removal must not unregister or unpin').toEqual([])
  expect(registry.projects).toHaveLength(1)
  expect(state.pinnedProjects).toEqual(['/repo'])
  expect(app.isProjectRemoving('/repo')).toBe(false)
  if (change === 'cancel') { expect(management.error).toBeNull(); expect(registry.status).toBe('loaded'); expect(registry.lastError).toBeNull() }
})


// Legacy 的同一 canonical writer 也须在等待结束后拒绝取消确认或被替换的 PTY。
it.each(['cancel', 'pty'] as const)('Feedback_LegacyArchiveQueue_012_%s', async change => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy' })
  const { flushPromises } = await import('@vue/test-utils')
  const { useSessionStore } = await import('@/stores/session')
  const { createLegacyClaudeAdapter } = await import('@/session/adapters/legacyClaudeAdapter')
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  let release!: (value: unknown) => void; const writes: string[] = []; let reads = 0
  mockIPC(command => {
    if (command === 'pin_project') return new Promise(resolve => { release = resolve })
    if (command === 'archive_session') { writes.push(command); return { pinnedProjects: ['/other'], archivedSessions: { '/repo': ['saved'] } } }
    if (command === 'get_projects_state') { reads++; return { pinnedProjects: ['/other'], archivedSessions: {} } }
    throw new Error('unexpected')
  })
  const state = useProjectsStateStore(); state.loaded = true
  const legacy = useSessionStore(); const id = legacy.createTab('/repo', { sessionId: 'saved', name: 'Original' }); const tab = legacy.tabs.get(id)!
  tab.ptyId = 'pty-1'; tab.ptyGeneration = 1; tab.status = 'running'
  let stops = 0
  const adapter = createLegacyClaudeAdapter({ store: legacy, projectPaths: () => ['/repo'], runtime: {
    startTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn(), stopTab: async () => { stops++; tab.status = 'stopped'; tab.ptyId = null },
  } })
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([adapter]); await catalog.refresh()
  const pinning = state.pinProject('/other'); await flushPromises()
  catalog.beginSessionConfirmation('stop-and-archive', 'legacy-tab:' + id)
  const archiving = catalog.confirmSessionAction(); await flushPromises()
  expect(stops).toBe(1); expect(writes).toEqual([])
  if (change === 'cancel') catalog.closeSessionConfirmation()
  else { tab.ptyGeneration++; tab.ptyId = 'pty-2' }
  release({ pinnedProjects: ['/other'], archivedSessions: {} }); await pinning; await archiving
  expect(writes, 'invalidated Legacy archive must not write metadata').toEqual([])
  expect(state.archivedSessions.size).toBe(0); expect(state.pinnedProjects).toEqual(['/other'])
  expect(legacy.tabs.has(id)).toBe(true); expect(catalog.confirmationError).toBeNull()
  expect(reads).toBe(0); expect(state.lastErrorCode).toBeNull(); expect(state.error).toBe(false)
})


// 已注销后排队的 pin 清理也受确认和注册身份约束，取消后保留尚未清理的元数据。
it.each(['cancel', 'replace'] as const)('Feedback_UnpinQueueGuard_013_%s', async change => {
  const { flushPromises } = await import('@vue/test-utils')
  const { useAppStore } = await import('@/stores/app')
  const { useWorkspaceStore } = await import('@/stores/workspace')
  const { useProjectManagementStore } = await import('@/stores/projectManagement')
  const row = { projectId: 'p', hostId: 'h', sourcePathKey: 's', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' as const }, pinned: { mode: 'inherit' as const }, hidden: { mode: 'inherit' as const } }
  let hidden: string[] = []; let pinned = ['/repo']; const writes: string[] = []; let reads = 0
  let finish!: (value: unknown) => void
  mockIPC((command, args) => {
    if (command === 'get_app_config') return { hiddenProjects: hidden }
    if (command === 'update_app_config') { hidden = (args as { updates: { hiddenProjects: string[] } }).updates.hiddenProjects; return }
    if (command === 'get_projects_state') { reads++; return { pinnedProjects: pinned, archivedSessions: {} } }
    if (command === 'cli_list_projects') return { revision: '1', projects: [row] }
    if (command === 'pin_project') return new Promise(resolve => { finish = resolve })
    if (command === 'cli_remove_project') { writes.push(command); return { revision: '2', projects: [] } }
    if (command === 'unpin_project') { writes.push(command); return { pinnedProjects: ['/other'], archivedSessions: {} } }
    throw new Error('unexpected')
  })
  const registry = useWorkspaceStore(); await registry.load()
  const state = useProjectsStateStore(); await state.ensureLoaded()
  const app = useAppStore(); await app.loadProjectVisibility()
  const earlier = state.pinProject('/other'); await flushPromises()
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/repo', projectPath: '/repo' })
  const removing = management.remove(); await flushPromises()
  expect(writes).toEqual(['cli_remove_project'])
  if (change === 'cancel') management.closeDialog()
  else registry.projects.push({ ...row, projectId: 'replacement' })
  pinned = ['/repo', '/other']; finish({ pinnedProjects: pinned, archivedSessions: {} }); await earlier; await removing
  expect(writes, 'invalidated queued cleanup must not unpin').toEqual(['cli_remove_project'])
  expect(state.pinnedProjects).toEqual(['/repo', '/other'])
  expect(state.lastErrorCode).toBeNull(); expect(state.error).toBe(false)
  if (change === 'cancel') { expect(management.error).toBeNull(); expect(reads).toBe(1) }
})
