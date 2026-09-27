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
      op: 'delete',
      id: 'claude-main',
    })).rejects.toMatchObject({ code: 'REVISION_CONFLICT' })

    expect(calls).toBe(1)
    expect(args).toEqual({
      expectedRevision: '9',
      patch: { op: 'delete', id: 'claude-main' },
    })
  })
})
