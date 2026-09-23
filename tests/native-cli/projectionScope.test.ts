import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { useConfigStore } from '@/stores/config'
import type { ProjectConfigResult } from '@/types'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
function config(label: string): ProjectConfigResult {
  return { basic: [{ model: label, source: { type: 'project', label: 'fixture' } }], mcp: [], skills: [], agents: [], hooks: [] }
}

describe('D12 config request ownership', () => {
  beforeEach(() => { clearMocks(); setActivePinia(createPinia()) })

  it('D12_Panel_LateOldResultCannotReplaceNew_001', async () => {
    const old = deferred<ProjectConfigResult>()
    mockIPC((_cmd, args) => (args as { projectPath: string }).projectPath === '/old' ? old.promise : config('new'))
    const store = useConfigStore()
    const pending = store.loadProjectConfig('/old')
    await store.loadProjectConfig('/new')
    old.resolve(config('old'))
    await pending
    expect(store.projectConfig).toEqual(config('new'))
  })

  it('D12_Panel_ClearInvalidatesPending_002', async () => {
    const response = deferred<ProjectConfigResult>()
    mockIPC(() => response.promise)
    const store = useConfigStore()
    const pending = store.loadProjectConfig('/old')
    store.clearConfig()
    response.resolve(config('old'))
    await pending
    expect(store.projectConfig).toBeNull()
    expect(store.isLoading).toBe(false)
  })

  it('D12_Panel_OldFailureCannotPoisonNew_003', async () => {
    const old = deferred<ProjectConfigResult>()
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    try {
      mockIPC((_cmd, args) => (args as { projectPath: string }).projectPath === '/old' ? old.promise : config('new'))
      const store = useConfigStore()
      const pending = store.loadProjectConfig('/old')
      await store.loadProjectConfig('/new')
      old.reject(new Error('synthetic failure'))
      await pending
      expect(store.error).toBeNull()
      expect(store.projectConfig).toEqual(config('new'))
    } finally { consoleSpy.mockRestore() }
  })

  it('D12_Panel_OldFinallyCannotClearCurrentLoading_004', async () => {
    const old = deferred<ProjectConfigResult>()
    const current = deferred<ProjectConfigResult>()
    mockIPC((_cmd, args) => (args as { projectPath: string }).projectPath === '/old' ? old.promise : current.promise)
    const store = useConfigStore()
    const pendingOld = store.loadProjectConfig('/old')
    const pendingCurrent = store.loadProjectConfig('/new')
    old.resolve(config('old'))
    await pendingOld
    expect(store.isLoading).toBe(true)
    current.resolve(config('new'))
    await pendingCurrent
    expect(store.isLoading).toBe(false)
  })

  it('D12_Panel_ReturnToCachedSelectionInvalidatesOtherLoad_005', async () => {
    const other = deferred<ProjectConfigResult>()
    mockIPC((_cmd, args) => (args as { projectPath: string }).projectPath === '/other' ? other.promise : config('selected'))
    const store = useConfigStore()
    await store.loadProjectConfig('/selected')
    const pending = store.loadProjectConfig('/other')
    await store.loadProjectConfig('/selected')
    other.resolve(config('other'))
    await pending
    expect(store.projectConfig).toEqual(config('selected'))
    expect(store.isLoading).toBe(false)
  })
  it('D12_Panel_NewSelectionDoesNotDisplayPreviousConfig_006', async () => {
    const other = deferred<ProjectConfigResult>()
    mockIPC((_cmd, args) => (args as { projectPath: string }).projectPath === '/other' ? other.promise : config('previous'))
    const store = useConfigStore()
    await store.loadProjectConfig('/previous')
    const pending = store.loadProjectConfig('/other')
    expect(store.projectConfig).toBeNull()
    other.resolve(config('other'))
    await pending
    expect(store.projectConfig).toEqual(config('other'))
  })

  it('D12_Panel_EmptySelectionClearsAndInvalidates_007', async () => {
    const old = deferred<ProjectConfigResult>()
    mockIPC(() => old.promise)
    const store = useConfigStore()
    const pending = store.loadProjectConfig('/old')
    await store.loadProjectConfig('')
    old.resolve(config('old'))
    await pending
    expect(store.projectConfig).toBeNull()
    expect(store.isLoading).toBe(false)
  })

  it('D12_Panel_CurrentFailureDoesNotLogNativePayload_008', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    try {
      mockIPC(() => Promise.reject(new Error('synthetic-private-native-value')))
      const store = useConfigStore()
      await store.loadProjectConfig('/current')
      expect(store.error).toBe('Failed to load project config')
      expect(spy).not.toHaveBeenCalled()
      expect(store.isLoading).toBe(false)
    } finally { spy.mockRestore() }
  })

})
