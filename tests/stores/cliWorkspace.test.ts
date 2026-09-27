import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useCliWorkspaceStore } from '@/stores/cliWorkspace'

const bridgeKey = '__CC_DESK_DOCUMENT__'

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

function emptyProjects(revision = '1') {
  return { revision, projects: [], metadata: {}, warnings: [] }
}

async function loadProfiles(rows = [
  profile('claude-main', 'claude', '7'),
  profile('codex-main', 'codex', '9'),
]) {
  mockIPC(command => {
    if (command === 'cli_list_profiles') return { revision: '10', profiles: rows }
    if (command === 'cli_list_projects') return emptyProjects()
    throw new Error('unexpected:' + command)
  })
  const profiles = useCliProfilesStore()
  await profiles.load()
  return profiles
}

beforeEach(() => {
  clearMocks()
  setActivePinia(createPinia())
  delete (window as any)[bridgeKey]
})

afterEach(() => {
  clearMocks()
  delete (window as any)[bridgeKey]
})

describe('D24 native resource/error workspace integration', () => {
  it('D24_Workspace_OpensExactSelectedCliProfile_01', async () => {
    const profiles = await loadProfiles()
    profiles.select('codex', 'codex-main')

    const workspace = useCliWorkspaceStore()
    await workspace.open('codex')

    expect(workspace.status).toBe('ready')
    expect(workspace.cli).toBe('codex')
    expect(workspace.profileIdentity).toEqual({
      profileId: 'codex-main',
      revision: '9',
      cli: 'codex',
    })
    expect(workspace.error).toBeNull()
  })

  it('D24_Workspace_MissingProfileFailsBeforeProjectOrResourceIo_02', async () => {
    const calls: string[] = []
    mockIPC(command => {
      calls.push(command)
      if (command === 'cli_list_profiles') {
        return { revision: '1', profiles: [profile('codex-main', 'codex')] }
      }
      throw new Error('must-not-run')
    })
    await useCliProfilesStore().load()

    const workspace = useCliWorkspaceStore()
    await expect(workspace.open('claude')).rejects.toThrow('CLI_PROFILE_REQUIRED')

    expect(workspace.status).toBe('error')
    expect(workspace.error).toBe('CLI_PROFILE_REQUIRED')
    expect(calls).toEqual(['cli_list_profiles'])
  })

  it('D24_Workspace_ProfileSelectionChangeInvalidatesOldResourceScope_03', async () => {
    const profiles = await loadProfiles([
      profile('claude-a', 'claude', '3'),
      profile('claude-b', 'claude', '4'),
    ])
    profiles.select('claude', 'claude-a')

    const workspace = useCliWorkspaceStore()
    await workspace.open('claude')
    profiles.select('claude', 'claude-b')

    await expect(workspace.loadResource('config')).rejects.toThrow('PROFILE_SELECTION_CHANGED')
    expect(workspace.error).toBe('PROFILE_SELECTION_CHANGED')
    expect(workspace.resource).toBeNull()
  })

  it('D24_Workspace_ResourceReadUsesFrozenProfileRevisionAndAuthenticatedProjection_04', async () => {
    const profiles = await loadProfiles()
    profiles.select('claude', 'claude-main')
    const invoked: Array<[string, any]> = []

    Object.defineProperty(window, bridgeKey, {
      configurable: true,
      value: {
        instanceId: 'backend-d24',
        async invoke(command: string, payload: any) {
          invoked.push([command, payload])
          if (command === 'native_get_scope') {
            return {
              scopeId: 'scope-d24',
              instanceId: 'backend-d24',
              cli: 'claude',
              sourceRootKey: 'root-d24',
              identityEpoch: '1',
              profileId: 'claude-main',
              profileRevision: '7',
              target: {
                kind: 'profile',
                profileId: 'claude-main',
                expectedProfileRevision: '7',
                projectId: null,
              },
              basis: 'configured-profile',
            }
          }
          if (command === 'native_list_resources') {
            return {
              source: payload.source,
              resourceKind: payload.resourceKind,
              requestEpoch: payload.requestEpoch,
              observedAt: '1',
              state: 'ready',
              reason: null,
              items: [],
              hasMore: false,
            }
          }
          throw new Error(command)
        },
      },
    })

    const workspace = useCliWorkspaceStore()
    await workspace.open('claude')
    await workspace.loadResource('config')

    expect(invoked[0]).toEqual([
      'native_get_scope',
      {
        kind: 'profile',
        profileId: 'claude-main',
        expectedProfileRevision: '7',
        projectId: null,
      },
    ])
    expect(invoked[1][0]).toBe('native_list_resources')
    expect(workspace.resource?.resourceKind).toBe('config')
    expect(workspace.error).toBeNull()
  })

  it('D24_Workspace_DoesNotExposeArbitraryNativeErrorText_05', async () => {
    mockIPC(command => {
      if (command === 'cli_list_profiles') {
        return { revision: '1', profiles: [profile('claude-main', 'claude')] }
      }
      if (command === 'cli_list_projects') {
        throw new Error('SECRET native path C:\\Users\\private\\token.txt')
      }
      throw new Error(command)
    })
    await useCliProfilesStore().load()

    const workspace = useCliWorkspaceStore()
    await expect(workspace.open('claude')).rejects.toThrow()

    expect(workspace.status).toBe('error')
    expect(workspace.error).toBe('NATIVE_WORKSPACE_UNAVAILABLE')
    expect(JSON.stringify({
      error: workspace.error,
      identity: workspace.profileIdentity,
    })).not.toContain('private')
  })

  it('D24_Workspace_SafeBackendErrorCodeIsPreservedWithoutRawMessage_06', async () => {
    mockIPC(command => {
      if (command === 'cli_list_profiles') {
        return { revision: '1', profiles: [profile('claude-main', 'claude')] }
      }
      if (command === 'cli_list_projects') {
        throw { code: 'REVISION_CONFLICT', message: 'secret backend details' }
      }
      throw new Error(command)
    })
    await useCliProfilesStore().load()

    const workspace = useCliWorkspaceStore()
    await expect(workspace.open('claude')).rejects.toMatchObject({ code: 'REVISION_CONFLICT' })

    expect(workspace.error).toBe('REVISION_CONFLICT')
    expect(workspace.error).not.toContain('secret')
  })

  it('D24_Workspace_ProfileSelectionChangeImmediatelyInvalidatesOldWorkspace_07', async () => {
    const profiles = await loadProfiles([
      profile('claude-a', 'claude', '3'),
      profile('claude-b', 'claude', '4'),
    ])
    profiles.select('claude', 'claude-a')
    const workspace = useCliWorkspaceStore()
    await workspace.open('claude')

    profiles.select('claude', 'claude-b')
    await nextTick()

    expect(workspace.status).toBe('error')
    expect(workspace.error).toBe('PROFILE_SELECTION_CHANGED')
    expect(workspace.resource).toBeNull()
    expect(workspace.profileIdentity).toEqual({
      profileId: 'claude-a',
      revision: '3',
      cli: 'claude',
    })
  })
})
