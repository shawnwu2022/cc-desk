import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useNativeWorkbenchStore } from '@/stores/nativeWorkbench'

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

function projects() {
  return {
    revision: '1',
    projects: [{
      projectId: 'project-1',
      hostId: 'host-1',
      sourcePathKey: 'root-1',
      selectedPath: '/repo',
      canonicalPath: '/repo',
      alias: { mode: 'inherit' },
      pinned: { mode: 'inherit' },
      hidden: { mode: 'inherit' },
    }],
    metadata: {},
    warnings: [],
  }
}

beforeEach(() => {
  clearMocks()
  setActivePinia(createPinia())
})

afterEach(() => clearMocks())

describe('D22-D24 native workbench state', () => {
  it('D22_Workbench_CodexOnlyInitializesWithoutLegacyClaudeChecks_14', async () => {
    const calls: string[] = []
    mockIPC(command => {
      calls.push(command)
      if (command === 'cli_list_profiles') {
        return { revision: '1', profiles: [profile('codex-main', 'codex', '5')] }
      }
      if (command === 'cli_list_projects') return projects()
      throw new Error('unexpected:' + command)
    })

    const store = useNativeWorkbenchStore()
    await store.initialize('codex')

    expect(store.cli).toBe('codex')
    expect(store.selectedProject?.projectId).toBe('project-1')
    expect(calls).toEqual(['cli_list_profiles', 'cli_list_projects'])
    expect(calls).not.toContain('get_check_results')
    expect(calls).not.toContain('run_checks')
  })

  it('D22_Workbench_EmptyWorkspaceCanExplicitlyCreateCodexProfile_15', async () => {
    mockIPC((command, args) => {
      if (command === 'cli_list_profiles') {
        return { revision: '0', profiles: [] }
      }
      if (command === 'cli_patch_profile') {
        expect(args).toMatchObject({
          expectedRevision: '0',
          patch: {
            op: 'create',
            profile: {
              id: 'codexDefault',
              revision: '0',
              cli: 'codex',
              launcher: { kind: 'native' },
            },
          },
        })
        return {
          revision: '1',
          profiles: [profile('codexDefault', 'codex', '1')],
        }
      }
      if (command === 'cli_list_projects') return projects()
      throw new Error('unexpected:' + command)
    })

    const store = useNativeWorkbenchStore()
    await expect(store.initialize('codex')).rejects.toThrow('CLI_PROFILE_REQUIRED')

    const created = await store.createDefaultProfile('codex')

    expect(created.id).toBe('codexDefault')
    expect(store.profiles.selected.codex?.id).toBe('codexDefault')
    expect(store.cli).toBe('codex')
    expect(store.status).toBe('ready')
    expect(store.selectedProject?.projectId).toBe('project-1')
  })

  it('D23_Workbench_NewAndResumeActionsFreezeExactCliProfileAndProject_15', async () => {
    mockIPC(command => {
      if (command === 'cli_list_profiles') {
        return {
          revision: '1',
          profiles: [
            profile('claude-main', 'claude', '3'),
            profile('codex-main', 'codex', '8'),
          ],
        }
      }
      if (command === 'cli_list_projects') return projects()
      throw new Error(command)
    })

    const store = useNativeWorkbenchStore()
    await store.initialize('codex')

    const fresh = store.createTab({ kind: 'new' })
    const picker = store.createTab({ kind: 'resume-picker', scope: 'current-project' })
    const known = store.createTab({ kind: 'resume-id', nativeSessionId: 'session-123' })
    const raw = store.createTab({ kind: 'raw', argv: ['', '中文', '--future'] })

    for (const tab of [fresh, picker, known, raw]) {
      expect(tab.cli).toBe('codex')
      expect(tab.profileId).toBe('codex-main')
      expect(tab.profileRevision).toBe('8')
      expect(tab.projectId).toBe('project-1')
      expect(tab.projectPath).toBe('/repo')
    }
    expect(picker.action).toEqual({ kind: 'resume-picker', scope: 'current-project' })
    expect(known.action).toEqual({ kind: 'resume-id', nativeSessionId: 'session-123' })
    expect(raw.action).toEqual({ kind: 'raw', argv: ['', '中文', '--future'] })
  })

  it('D24_Workbench_DoesNotReflectRawWorkspaceFailureText_16', async () => {
    mockIPC(command => {
      if (command === 'cli_list_profiles') {
        return { revision: '1', profiles: [profile('codex-main', 'codex')] }
      }
      if (command === 'cli_list_projects') {
        throw new Error('SECRET C:\\Users\\private\\token.txt')
      }
      throw new Error(command)
    })

    const store = useNativeWorkbenchStore()
    await expect(store.initialize('codex')).rejects.toThrow()

    expect(store.status).toBe('error')
    expect(store.error).toBe('NATIVE_WORKSPACE_UNAVAILABLE')
    expect(store.error).not.toContain('private')
  })

  it('D24_Workbench_ProfileOrProjectAbsenceFailsBeforeCreatingTab_16', async () => {
    mockIPC(command => {
      if (command === 'cli_list_profiles') {
        return { revision: '1', profiles: [profile('codex-main', 'codex')] }
      }
      if (command === 'cli_list_projects') {
        return { revision: '1', projects: [], metadata: {}, warnings: [] }
      }
      throw new Error(command)
    })

    const store = useNativeWorkbenchStore()
    await store.initialize('codex')

    expect(() => store.createTab({ kind: 'new' })).toThrowError('PROJECT_REQUIRED')
    expect(store.tabs.tabs.size).toBe(0)
  })

  it('D22_Workbench_SwitchCliKeepsExistingSiblingTabsIndependent_17', async () => {
    mockIPC(command => {
      if (command === 'cli_list_profiles') {
        return {
          revision: '1',
          profiles: [
            profile('claude-main', 'claude'),
            profile('codex-main', 'codex'),
          ],
        }
      }
      if (command === 'cli_list_projects') return projects()
      throw new Error(command)
    })

    const store = useNativeWorkbenchStore()
    await store.initialize('codex')
    const codex = store.createTab({ kind: 'new' })
    await store.selectCli('claude')
    const claude = store.createTab({ kind: 'new' })

    expect(store.tabs.tab(codex.tabId)?.cli).toBe('codex')
    expect(store.tabs.tab(claude.tabId)?.cli).toBe('claude')
    expect(store.tabs.byProject('/repo')).toHaveLength(2)
  })
})
