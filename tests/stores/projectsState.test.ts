import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { ProjectLaunchPreference, ProjectsState, SessionUiRecord } from '@/types/app'

const emptyState = (): ProjectsState => ({
  pinnedProjects: [],
  archivedSessions: {},
  displayNames: {},
  sessionRecords: {},
  launchPreferences: {},
})

const nativeRecord: SessionUiRecord = {
  runtime: 'native-cli',
  cli: 'codex',
  projectPath: 'D:/work/game',
  adapterSessionId: 'tab-1',
  nativeSessionId: 'native-1',
  title: 'Fix login',
  lastActivityAt: 1234,
}

const codexPreference: ProjectLaunchPreference = {
  lastCli: 'codex',
  claudeLaunchConfigId: null,
  codexLaunchConfigId: 'codex-default',
}

vi.mock('@/api/tauri', () => ({
  getProjectsState: vi.fn(),
  pinProject: vi.fn(),
  unpinProject: vi.fn(),
  archiveSession: vi.fn(),
  restoreSession: vi.fn(),
  setDisplayName: vi.fn(),
  upsertSessionUiRecord: vi.fn(),
  removeSessionUiRecord: vi.fn(),
  setProjectLaunchPreference: vi.fn(),
}))

import { useProjectsStateStore } from '@/stores/projectsState'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

beforeEach(async () => {
  vi.resetAllMocks()
  setActivePinia(createPinia())
  const api = await import('@/api/tauri')
  ;(api.getProjectsState as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.pinProject as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.unpinProject as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.archiveSession as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.restoreSession as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.setDisplayName as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.upsertSessionUiRecord as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.removeSessionUiRecord as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
  ;(api.setProjectLaunchPreference as ReturnType<typeof vi.fn>).mockResolvedValue(emptyState())
})

describe('durable shared projects state', () => {
  it('serializes mutations and applies complete returned snapshots', async () => {
    const api = await import('@/api/tauri')
    const first = deferred<ProjectsState>()
    const second = deferred<ProjectsState>()
    const pin = api.pinProject as ReturnType<typeof vi.fn>
    const archive = api.archiveSession as ReturnType<typeof vi.fn>
    pin.mockReturnValueOnce(first.promise)
    archive.mockReturnValueOnce(second.promise)

    const store = useProjectsStateStore()
    await store.load()
    const pinning = store.pinProject('/work/game')
    const archiving = store.archiveSession('/work/game', 'legacy-1')

    await vi.waitFor(() => expect(pin).toHaveBeenCalledTimes(1))
    expect(archive).not.toHaveBeenCalled()

    first.resolve({
      pinnedProjects: ['/work/game'],
      archivedSessions: {},
      displayNames: {},
      sessionRecords: { native: nativeRecord },
      launchPreferences: { '/work/game': codexPreference },
    })
    await pinning
    await vi.waitFor(() => expect(archive).toHaveBeenCalledTimes(1))

    second.resolve({
      pinnedProjects: ['/work/game'],
      archivedSessions: { '/work/game': ['legacy-1'] },
      displayNames: {},
      sessionRecords: { native: nativeRecord },
      launchPreferences: { '/work/game': codexPreference },
    })
    await archiving

    expect(store.pinnedProjects).toEqual(['/work/game'])
    expect(store.archivedSessions.get('/work/game')).toEqual(['legacy-1'])
    expect(store.sessionRecords.get('native')).toEqual(nativeRecord)
    expect(store.launchPreferences.get('/work/game')).toEqual(codexPreference)
  })

  it('reloads latest state after a revision conflict without replaying the mutation', async () => {
    const api = await import('@/api/tauri')
    const get = api.getProjectsState as ReturnType<typeof vi.fn>
    const pin = api.pinProject as ReturnType<typeof vi.fn>
    get
      .mockResolvedValueOnce({ ...emptyState(), pinnedProjects: ['/old'] })
      .mockResolvedValueOnce({
        ...emptyState(),
        pinnedProjects: ['/external'],
        sessionRecords: { native: nativeRecord },
      })
    pin.mockRejectedValueOnce({ code: 'REVISION_CONFLICT' })

    const store = useProjectsStateStore()
    await store.load()
    await expect(store.pinProject('/mine')).rejects.toMatchObject({ code: 'REVISION_CONFLICT' })

    expect(pin).toHaveBeenCalledTimes(1)
    expect(get).toHaveBeenCalledTimes(2)
    expect(store.pinnedProjects).toEqual(['/external'])
    expect(store.sessionRecords.get('native')).toEqual(nativeRecord)
  })

  it('leaves the adopted state unchanged when a non-conflict mutation fails', async () => {
    const api = await import('@/api/tauri')
    const get = api.getProjectsState as ReturnType<typeof vi.fn>
    const archive = api.archiveSession as ReturnType<typeof vi.fn>
    get.mockResolvedValueOnce({
      ...emptyState(),
      pinnedProjects: ['/stable'],
      sessionRecords: { native: nativeRecord },
    })
    archive.mockRejectedValueOnce(new Error('write failed'))

    const store = useProjectsStateStore()
    await store.load()
    await expect(store.archiveSession('/stable', 'legacy-1')).rejects.toThrow('write failed')

    expect(store.pinnedProjects).toEqual(['/stable'])
    expect(store.archivedSessions.size).toBe(0)
    expect(store.sessionRecords.get('native')).toEqual(nativeRecord)
  })

  it('persists session records and per-project launch preferences through typed actions', async () => {
    const api = await import('@/api/tauri')
    const upsert = api.upsertSessionUiRecord as ReturnType<typeof vi.fn>
    const setPreference = api.setProjectLaunchPreference as ReturnType<typeof vi.fn>
    upsert.mockResolvedValueOnce({
      ...emptyState(),
      sessionRecords: { native: nativeRecord },
    })
    setPreference.mockResolvedValueOnce({
      ...emptyState(),
      sessionRecords: { native: nativeRecord },
      launchPreferences: { 'd:/work/game': codexPreference },
    })

    const store = useProjectsStateStore()
    await store.load()
    await store.upsertSessionRecord('native', nativeRecord)
    await store.setLaunchPreference('D:/work/game', 'codex', 'codex-default')

    expect(upsert).toHaveBeenCalledWith('native', nativeRecord)
    expect(setPreference).toHaveBeenCalledWith('D:/work/game', codexPreference)
    expect(store.sessionRecords.get('native')).toEqual(nativeRecord)
    expect(store.launchPreferences.get('d:/work/game')).toEqual(codexPreference)
  })
})