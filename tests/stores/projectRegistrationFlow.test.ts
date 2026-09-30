import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useAppStore } from '@/stores/app'
import { useWorkspaceStore } from '@/stores/workspace'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useCliWorkspaceStore } from '@/stores/cliWorkspace'
vi.mock('@/utils/platform', () => ({ isMac: false, isWindows: true }))
const row = { projectId: 'project-a', hostId: 'host', sourcePathKey: 'path', selectedPath: 'C:\\Work\\Desk', canonicalPath: null,
  alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }
beforeEach(() => { clearMocks(); setActivePinia(createPinia()) })
afterEach(() => { clearMocks(); vi.restoreAllMocks() })
describe('Unified project registration', () => {
  // 添加一次即采用真实注册结果，不要求安装工具或创建启动配置。
  it('Projects_AddOnceWithoutProfile_001', async () => {
    const writes: string[] = []
    mockIPC(command => {
      if (command === 'get_app_config') return { hiddenProjects: [] }
      if (command === 'cli_list_projects') return { revision: '0', projects: [] }
      if (command === 'cli_register_project') { writes.push(command); return { revision: '1', projects: [row], projectId: row.projectId } }
      throw new Error(`unexpected:${command}`)
    })
    const app = useAppStore() as any
    expect(typeof app.addManagedProject).toBe('function')
    const result = await app.addManagedProject(row.selectedPath)
    expect(result).toEqual({ projectKey: 'c:/work/desk', projectPath: row.selectedPath })
    expect(app.cachedProjects.map((p: any) => p.path)).toEqual([row.selectedPath])
    expect(writes).toEqual(['cli_register_project'])
  })
  // 同一Windows路径重复添加采用已有项目，注册队列也按规范化身份合并。
  it('Projects_AdoptWindowsDuplicate_002', async () => {
    let registrations = 0
    mockIPC(command => {
      if (command === 'get_app_config') return { hiddenProjects: [] }
      if (command === 'cli_list_projects') return { revision: '1', projects: [row] }
      if (command === 'cli_register_project') { registrations++; throw new Error('duplicate mutation') }
      throw new Error(`unexpected:${command}`)
    })
    const app = useAppStore() as any
    expect(typeof app.addManagedProject).toBe('function')
    await Promise.all([app.addManagedProject('c:/WORK/Desk/'), app.addManagedProject(row.selectedPath)])
    expect(app.cachedProjects).toHaveLength(1)
    expect(app.cachedProjects[0].path).toBe(row.selectedPath)
    expect(registrations).toBe(0)
  })
  // 变更冲突刷新权威状态，但不重放已请求的删除。
  it('Projects_ConflictReloadNoReplay_003', async () => {
    let reads = 0; let writes = 0
    mockIPC(command => {
      if (command === 'cli_list_projects') { reads++; return { revision: reads === 1 ? '1' : '2', projects: [row] } }
      if (command === 'cli_remove_project') { writes++; throw { code: 'REVISION_CONFLICT' } }
      throw new Error(`unexpected:${command}`)
    })
    const workspace = useWorkspaceStore(); await workspace.load()
    await expect(workspace.remove(row.projectId)).rejects.toMatchObject({ code: 'REVISION_CONFLICT' })
    expect(workspace.revision).toBe('2'); expect(writes).toBe(1); expect(reads).toBe(2)
  })
  // 未知提交状态只重读注册状态，不重放添加命令。
  it('Projects_UnknownRegisterNoReplay_004', async () => {
    let reads = 0; let writes = 0
    mockIPC(command => {
      if (command === 'cli_list_projects') { reads++; return { revision: reads === 1 ? '0' : '1', projects: reads === 1 ? [] : [row] } }
      if (command === 'cli_register_project') { writes++; throw { code: 'COMMIT_STATE_UNKNOWN' } }
      throw new Error(`unexpected:${command}`)
    })
    const workspace = useWorkspaceStore()
    await expect(workspace.ensureRegistered(row.selectedPath)).rejects.toThrow('PROJECT_REGISTRATION_FAILED')
    expect(workspace.projects.map(p => p.projectId)).toEqual([row.projectId]); expect(writes).toBe(1)
  })
  // 提供启动配置时检查CLI身份，缺少另一CLI配置不影响注册。
  it('Projects_ProfileBindingScoped_005', async () => {
    const workspace = useCliWorkspaceStore() as any
    expect(typeof workspace.ensureNativeProjectRegistration).toBe('function')
    await expect(workspace.ensureNativeProjectRegistration({ path: row.selectedPath, cli: 'codex', profile: { id: 'claude', revision: '1', cli: 'claude' } })).rejects.toThrow('PROFILE_CLI_MISMATCH')
  })
  // 同时添加路径大小写/斜杠变体共享一次真实注册写入。
  it('Projects_CoalesceNormalizedAdd_006', async () => {
    let writes = 0
    mockIPC(command => {
      if (command === 'get_app_config') return { hiddenProjects: [] }
      if (command === 'cli_list_projects') return { revision: '0', projects: [] }
      if (command === 'cli_register_project') { writes++; return { revision: '1', projects: [row], projectId: row.projectId } }
      throw new Error(`unexpected:${command}`)
    })
    const app = useAppStore()
    const results = await Promise.all([app.addManagedProject(row.selectedPath), app.addManagedProject('c:/WORK/desk/')])
    expect(writes).toBe(1); expect(results[0]).toEqual(results[1]); expect(app.cachedProjects).toHaveLength(1)
  })
  // 已有另一CLI的配置不会阻止明确Codex配置对应项目注册。
  it('Projects_OneCliProfileSuffices_007', async () => {
    useCliProfilesStore().profiles = [{ id: 'cx', revision: '7', cli: 'codex', name: 'Codex', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }]
    mockIPC(command => command === 'cli_list_projects' ? { revision: '1', projects: [row] } : undefined)
    const id = await useCliWorkspaceStore().ensureNativeProjectRegistration({ path: row.selectedPath, cli: 'codex', profile: { id: 'cx', revision: '7', cli: 'codex' } })
    expect(id).toBe(row.projectId)
  })
  // 移除只取消原生注册并隐藏常规列表，归档/名称/配置和CLI历史保持不变。
  it('Projects_RemovePreservesHistory_008', async () => {
    const calls: string[] = []; let hidden: string[] = []
    const saved = { pinnedProjects: [row.selectedPath], archivedSessions: { 'c:/work/desk': ['archive-id'] }, displayNames: { 'c:/work/desk': 'Alias' }, launchPreferences: { 'c:/work/desk': { lastCli: 'codex' as const, codexLaunchConfigId: 'cx' } } }
    mockIPC((command, payload) => {
      calls.push(command)
      if (command === 'get_app_config') return { hiddenProjects: hidden }
      if (command === 'update_app_config') { hidden = (payload as any).updates.hiddenProjects; return }
      if (command === 'get_projects_state') return saved
      if (command === 'cli_list_projects') return { revision: '1', projects: [row] }
      if (command === 'cli_remove_project') { expect(payload).toEqual({ projectId: row.projectId, expectedRevision: '1' }); return { revision: '2', projects: [] } }
      if (command === 'unpin_project') return { ...saved, pinnedProjects: [] }
      throw new Error(`unexpected:${command}`)
    })
    const management = useProjectManagementStore()
    management.beginRemove({ projectKey: 'c:/work/desk', projectPath: row.selectedPath }); await management.remove()
    expect(management.dialog).toBeNull(); expect(management.error).toBeNull()
    expect(useWorkspaceStore().projects).toHaveLength(0); expect(useAppStore().isHidden(row.selectedPath)).toBe(true)
    expect(useProjectsStateStore().archivedSessions.get('c:/work/desk')).toEqual(['archive-id'])
    expect(useProjectsStateStore().displayNames.get('c:/work/desk')).toBe('Alias')
    expect(useProjectsStateStore().launchPreferences.get('c:/work/desk')?.codexLaunchConfigId).toBe('cx')
    expect(calls).not.toContain('delete_sessions'); expect(calls).not.toContain('pty_kill')
  })
  // 确认弹窗之后新打开的终端在变更入口重新检查，不被陈旧确认遗漏。
  it('Projects_RecheckRemovalOwnership_009', async () => {
    const management = useProjectManagementStore()
    management.beginRemove({ projectKey: 'c:/work/desk', projectPath: row.selectedPath })
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: row.projectId, projectPath: row.selectedPath, profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    mockIPC(command => {
      if (command === 'get_app_config') return { hiddenProjects: [] }
      if (command === 'get_projects_state') return { pinnedProjects: [], archivedSessions: {} }
      if (command === 'cli_list_projects') return { revision: '1', projects: [row] }
      throw new Error(`unexpected mutation:${command}`)
    })
    await management.remove()
    expect(management.error).toBe('projectRemoveOpenSessions'); expect(useNativeTabsStore().tab(tab.tabId)).toBeDefined()
    expect(useAppStore().isHidden(row.selectedPath)).toBe(false)
  })

  // 提交状态未知时刷新后保留结果，但绝不自动重放移除。
  it('Projects_UnknownRemoveNoReplay_010', async () => {
    let reads = 0; let writes = 0
    mockIPC(command => {
      if (command === 'cli_list_projects') { reads++; return { revision: reads === 1 ? '1' : '2', projects: reads === 1 ? [row] : [] } }
      if (command === 'cli_remove_project') { writes++; throw { code: 'COMMIT_STATE_UNKNOWN', message: 'private-token' } }
      throw new Error(`unexpected:${command}`)
    })
    const workspace = useWorkspaceStore(); await workspace.load()
    await expect(workspace.remove(row.projectId)).rejects.toMatchObject({ code: 'COMMIT_STATE_UNKNOWN' })
    expect(workspace.projects).toHaveLength(0); expect(writes).toBe(1)
  })
  // 隐藏Legacy目录不会创建Native注册；显示恢复仍通过已有配置可见性集合。
  it('Projects_HideDoesNotRegister_011', async () => {
    let hidden: string[] = []; const calls: string[] = []
    mockIPC((command, payload) => {
      calls.push(command)
      if (command === 'get_app_config') return { hiddenProjects: hidden }
      if (command === 'update_app_config') { hidden = (payload as any).updates.hiddenProjects; return }
      throw new Error(`unexpected:${command}`)
    })
    const management = useProjectManagementStore()
    await management.setHidden({ projectKey: 'c:/work/desk', projectPath: row.selectedPath }, true)
    expect(useAppStore().isHidden(row.selectedPath)).toBe(true)
    await management.setHidden({ projectKey: 'c:/work/desk', projectPath: row.selectedPath }, false)
    expect(useAppStore().isHidden(row.selectedPath)).toBe(false)
    expect(calls).not.toContain('cli_register_project')
  })
  // 可见性写入回执丢失时只重新读取，不重复写入或假装操作未发生。
  it('Projects_UnknownVisibilityReload_012', async () => {
    let hidden: string[] = []; let writes = 0
    mockIPC((command, payload) => {
      if (command === 'get_app_config') return { hiddenProjects: hidden }
      if (command === 'update_app_config') { writes++; hidden = (payload as any).updates.hiddenProjects; throw new Error('unknown acknowledgement') }
      throw new Error(`unexpected:${command}`)
    })
    const app = useAppStore()
    await expect(app.setManagedHidden(row.selectedPath, true)).rejects.toThrow('unknown acknowledgement')
    expect(app.isHidden(row.selectedPath)).toBe(true); expect(writes).toBe(1)
  })

  // 隐藏等待初始可见性读取时，新打开的终端必须阻止最终写入。
  it('Projects_HideAdmissionAfterRead_013', async () => {
    let finish!: (value: { hiddenProjects: string[] }) => void; let writes = 0
    const reading = new Promise<{ hiddenProjects: string[] }>(resolve => { finish = resolve })
    mockIPC(command => {
      if (command === 'get_app_config') return reading
      if (command === 'update_app_config') { writes++; return }
      throw new Error(`unexpected:${command}`)
    })
    const management = useProjectManagementStore()
    const hiding = management.setHidden({ projectKey: 'c:/work/desk', projectPath: row.selectedPath }, true)
    await flushPromises()
    useNativeTabsStore().create({ cli: 'codex', projectId: row.projectId, projectPath: row.selectedPath, profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    finish({ hiddenProjects: [] }); await hiding
    expect(writes).toBe(0); expect(useAppStore().isHidden(row.selectedPath)).toBe(false)
    expect(management.error).toBe('projectRemoveOpenSessions')
  })
  // 较晚完成的启动配置迁移不能覆盖已经确认写入的隐藏状态。
  it('Projects_StartupCannotUndoHide_014', async () => {
    let migrationDone!: () => void; let hidden: string[] = []
    mockIPC((command, payload) => {
      if (command === 'get_app_config') return { hiddenProjects: [], language: 'en', terminalTheme: 'cc-box-light' }
      if (command === 'update_app_config') {
        const updates = (payload as any).updates
        if ('hiddenProjects' in updates) { hidden = updates.hiddenProjects; return }
        return new Promise<void>(resolve => { migrationDone = resolve })
      }
      throw new Error(`unexpected:${command}`)
    })
    const app = useAppStore(); const startup = app.loadAppConfig()
    await app.loadProjectVisibility(); await flushPromises()
    await useProjectManagementStore().setHidden({ projectKey: 'c:/work/desk', projectPath: row.selectedPath }, true)
    expect(hidden).toEqual([row.selectedPath]); expect(app.isHidden(row.selectedPath)).toBe(true)
    migrationDone(); await startup
    expect(app.isHidden(row.selectedPath)).toBe(true)
    await useProjectManagementStore().setHidden({ projectKey: 'c:/work/other', projectPath: 'C:/work/other' }, true)
    expect(hidden).toEqual([row.selectedPath, 'C:/work/other'])
  })

  // 隐藏写入排队等待另一目录时，最终序列化入口仍需重新检查所有权。
  it('Projects_HideChecksWriterQueue_015', async () => {
    let release!: () => void; const writes: string[][] = []
    mockIPC((command, payload) => {
      if (command === 'get_app_config') return { hiddenProjects: [] }
      if (command === 'update_app_config') {
        writes.push((payload as any).updates.hiddenProjects)
        if (writes.length === 1) return new Promise<void>(resolve => { release = resolve })
        return
      }
      throw new Error(`unexpected:${command}`)
    })
    const app = useAppStore(); await app.loadProjectVisibility()
    const first = app.setManagedHidden('C:/work/first', true); await flushPromises()
    const management = useProjectManagementStore()
    const queued = management.setHidden({ projectKey: 'c:/work/desk', projectPath: row.selectedPath }, true); await flushPromises()
    useNativeTabsStore().create({ cli: 'codex', projectId: row.projectId, projectPath: row.selectedPath, profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    release(); await first; await queued
    expect(writes).toEqual([['C:/work/first']]); expect(app.isHidden(row.selectedPath)).toBe(false)
    expect(management.error).toBe('projectRemoveOpenSessions')
  })

})
