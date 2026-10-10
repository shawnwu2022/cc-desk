import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'
import type {
  CreateUnifiedSessionInput,
  ResumeUnifiedSessionInput,
  SessionAdapter,
  SessionRuntimeKind,
  UnifiedSession,
} from '@/types/unifiedSession'

function session(overrides: Partial<UnifiedSession> = {}): UnifiedSession {
  return {
    id: 'legacy-1',
    projectKey: '/repo',
    projectPath: '/repo',
    cli: 'claude',
    runtime: 'legacy-claude',
    title: 'Session',
    processState: 'running',
    attentionState: 'none',
    lastActivityAt: 100,
    archived: false,
    resumable: true,
    adapterSessionId: 'legacy-1',
    nativeSessionId: 'native-1',
    launchConfigId: null,
    safeErrorCode: null,
    renameState: 'idle',
    ...overrides,
  }
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function fakeAdapter(runtime: SessionRuntimeKind, initial: UnifiedSession[] = []) {
  let current = [...initial]
  const adapter: SessionAdapter = {
    runtime,
    listSessions: vi.fn(async (projectKey?: string) => current.filter(value => projectKey === undefined || value.projectKey === projectKey)),
    createSession: vi.fn(async (input: CreateUnifiedSessionInput) => {
      const created = session({
        id: runtime + ':created',
        runtime,
        cli: input.cli,
        projectKey: input.projectKey,
        projectPath: input.projectPath,
        title: input.title ?? 'Created',
        adapterSessionId: 'created',
        nativeSessionId: null,
        lastActivityAt: 500,
      })
      current = [created, ...current]
      return created
    }),
    resumeSession: vi.fn(async (input: ResumeUnifiedSessionInput) => {
      const resumed = session({
        id: runtime + ':resumed',
        runtime,
        cli: input.cli,
        projectKey: input.projectKey,
        projectPath: input.projectPath,
        title: input.title ?? 'Resumed',
        adapterSessionId: input.adapterSessionId,
        nativeSessionId: input.nativeSessionId,
        lastActivityAt: 600,
      })
      current = [resumed, ...current]
      return resumed
    }),
    activateSession: vi.fn(async () => undefined),
    stopSession: vi.fn(async () => undefined),
    restartSession: vi.fn(async id => current.find(value => value.id === id) ?? session({ id, runtime })),
    closeSession: vi.fn(async id => { current = current.filter(value => value.id !== id) }),
    renameSession: vi.fn(async (id, title) => {
      current = current.map(value => value.id === id ? { ...value, title } : value)
    }),
    archiveSession: vi.fn(async id => { current = current.map(value => value.id === id ? { ...value, archived: true } : value) }),
    restoreArchivedSession: vi.fn(async id => { current = current.map(value => value.id === id ? { ...value, archived: false } : value) }),
  }
  return {
    adapter,
    setSessions(next: UnifiedSession[]) { current = [...next] },
  }
}

it.each(['navigation', 'selection', 'revision', 'success'] as const)('confirmed discovery pins revision and cancels stale preflight: %s', async reason => {
  setActivePinia(createPinia())
  const store = useUnifiedSessionsStore(), native = fakeAdapter('native-cli')
  store.configureAdapters([native.adapter])
  store.configureCreationPreparer(async () => { throw new LaunchConfigurationRequiredError('p', { code: 'PROGRAM_TRUST_REQUIRED' }) })
  await expect(store.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })).rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
  const id = store.activeSessionId!, pending = deferred<CreateUnifiedSessionInput>()
  const prepare = vi.fn(() => pending.promise)
  store.configureCreationPreparer(prepare)
  let current = true
  const retry = store.retryConfirmedCreation(id, 'p', '3', () => current)
  const result = retry.then(() => 'admitted', () => 'cancelled')
  expect(prepare).toHaveBeenCalledWith(expect.objectContaining({ launchConfigId: 'p', launchConfigRevision: '3' }))
  if (reason === 'navigation') current = false
  if (reason === 'selection') { store.selectProjectContext('/other'); await store.activateSession(id) }
  pending.resolve({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'p', launchConfigRevision: reason === 'revision' ? '4' : '3' })
  expect(await result).toBe(reason === 'success' ? 'admitted' : 'cancelled')
  expect(native.adapter.createSession).toHaveBeenCalledTimes(reason === 'success' ? 1 : 0)
})

beforeEach(() => {
  vi.restoreAllMocks()
  setActivePinia(createPinia())
})

describe('unified sessions store', () => {
  // 项目按固定和显示名排列；刷新活动时间只能调整项目内会话顺序。
  it('StableProjectOrder_Catalog_001', async () => {
    const rows = [
      session({ id: 'zulu', projectPath: '/zulu', lastActivityAt: 100 }),
      session({ id: 'alpha-old', projectPath: '/alpha', lastActivityAt: 1 }),
      session({ id: 'alpha-new', projectPath: '/alpha', lastActivityAt: 2 }),
      session({ id: 'pinned', projectPath: '/omega', lastActivityAt: 0 }),
    ]
    const legacy = fakeAdapter('legacy-claude', rows)
    const projects = useProjectsStateStore()
    projects.pinnedProjects = ['/omega']
    projects.displayNames.set('/zulu', 'Bravo alias')
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter])
    await store.refresh()
    expect(store.projectGroups.map(group => group.projectKey)).toEqual(['/omega', '/alpha', '/zulu'])
    expect(store.projectGroups[1].sessions.map(row => row.id)).toEqual(['alpha-new', 'alpha-old'])
    legacy.setSessions(rows.map(row => ({ ...row, lastActivityAt: row.id === 'alpha-old' ? 1000 : row.lastActivityAt })))
    await store.refresh()
    expect(store.projectGroups.map(group => group.projectKey)).toEqual(['/omega', '/alpha', '/zulu'])
    expect(store.projectGroups[1].sessions.map(row => row.id)).toEqual(['alpha-old', 'alpha-new'])
  })

  it('merges legacy/native sessions into one project group and sorts pinned groups first', async () => {
    const legacy = fakeAdapter('legacy-claude', [
      session({ id: 'legacy', projectPath: '/repo', projectKey: '/repo', lastActivityAt: 100 }),
    ])
    const native = fakeAdapter('native-cli', [
      session({
        id: 'codex',
        runtime: 'native-cli',
        cli: 'codex',
        projectPath: '/repo',
        projectKey: '/repo',
        lastActivityAt: 300,
      }),
      session({
        id: 'other',
        runtime: 'native-cli',
        cli: 'codex',
        projectPath: '/other',
        projectKey: '/other',
        lastActivityAt: 400,
      }),
    ])
    const projects = useProjectsStateStore()
    projects.pinnedProjects = ['/repo']
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter, native.adapter])

    await store.refresh()

    expect(store.sessions.map(value => value.id)).toEqual(['other', 'codex', 'legacy'])
    expect(store.projectGroups[0].projectKey).toBe('/repo')
    expect(store.projectGroups[0].sessions.map(value => value.cli).sort()).toEqual(['claude', 'codex'])
    expect(store.projectGroups[0].runningCount).toBe(2)
  })

  it('deduplicates by unified id while preserving a needs-user attention projection', async () => {
    const legacy = fakeAdapter('legacy-claude', [
      session({ id: 'same', lastActivityAt: 200, attentionState: 'none' }),
    ])
    const native = fakeAdapter('native-cli', [
      session({
        id: 'same',
        runtime: 'native-cli',
        cli: 'codex',
        lastActivityAt: 100,
        attentionState: 'needs-user',
      }),
    ])
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter, native.adapter])

    await store.refresh()

    expect(store.sessions).toHaveLength(1)
    expect(store.sessions[0].runtime).toBe('legacy-claude')
    expect(store.sessions[0].attentionState).toBe('needs-user')
    expect(store.projectGroups[0].needsUserCount).toBe(1)
  })

  it('publishes only the latest completed activation', async () => {
    const first = deferred<void>()
    const legacy = fakeAdapter('legacy-claude', [session({ id: 'legacy' })])
    const native = fakeAdapter('native-cli', [
      session({ id: 'codex', runtime: 'native-cli', cli: 'codex' }),
    ])
    ;(legacy.adapter.activateSession as ReturnType<typeof vi.fn>).mockReturnValueOnce(first.promise)
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter, native.adapter])
    await store.refresh()

    const oldActivation = store.activateSession('legacy')
    await Promise.resolve()
    await store.activateSession('codex')
    expect(store.activeSessionId).toBe('codex')

    first.resolve()
    await oldActivation
    expect(store.activeSessionId).toBe('codex')
  })

  it('serializes lifecycle actions and suppresses stale stop publication after restart is requested', async () => {
    const stopping = deferred<void>()
    const active = session({ id: 'legacy' })
    const legacy = fakeAdapter('legacy-claude', [active])
    ;(legacy.adapter.stopSession as ReturnType<typeof vi.fn>).mockReturnValueOnce(stopping.promise)
    ;(legacy.adapter.restartSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      ...active,
      lastActivityAt: 700,
    })
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter])
    await store.refresh()

    const stop = store.stopSession('legacy')
    const restart = store.restartSession('legacy')
    await Promise.resolve()
    expect(legacy.adapter.restartSession).not.toHaveBeenCalled()

    stopping.resolve()
    await stop
    await restart

    expect(legacy.adapter.restartSession).toHaveBeenCalledTimes(1)
    expect(legacy.adapter.listSessions).toHaveBeenCalledTimes(2)
  })

  it('keeps resumable history visible after closing the active runtime projection', async () => {
    const active = session({ id: 'legacy-active', adapterSessionId: 'tab-1' })
    const history = session({
      id: 'legacy-history',
      processState: 'stopped',
      adapterSessionId: 'native-1',
      lastActivityAt: 90,
    })
    const legacy = fakeAdapter('legacy-claude', [active])
    ;(legacy.adapter.closeSession as ReturnType<typeof vi.fn>).mockImplementationOnce(async () => {
      legacy.setSessions([history])
    })
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter])
    await store.refresh()
    await store.activateSession('legacy-active')

    await store.closeSession('legacy-active')

    expect(store.activeSessionId).toBeNull()
    expect(store.sessions.map(value => value.id)).toEqual(['legacy-history'])
    expect(store.sessions[0].resumable).toBe(true)
  })

  it('routes new Claude/Codex to native and resumes by catalog origin', async () => {
    const legacy = fakeAdapter('legacy-claude', [session({ id: 'old', adapterSessionId: 'history-1', nativeSessionId: 'history-1' })])
    const native = fakeAdapter('native-cli', [session({ id: 'native-old', runtime: 'native-cli', adapterSessionId: 'native-history', nativeSessionId: 'native-history' })])
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter, native.adapter])

    const created = await store.createSession({
      projectKey: '/repo',
      projectPath: '/repo',
      cli: 'codex',
      title: 'Codex',
    })
    expect(native.adapter.createSession).toHaveBeenCalledTimes(1)
    expect(legacy.adapter.createSession).not.toHaveBeenCalled()
    expect(store.activeSessionId).toBe(created.id)

    await store.createSession({ projectKey: '/repo', projectPath: '/repo', cli: 'claude' })
    expect(native.adapter.createSession).toHaveBeenCalledTimes(2)
    expect(legacy.adapter.createSession).not.toHaveBeenCalled()
    await store.resumeSession({ projectKey: '/repo', projectPath: '/repo', cli: 'claude', adapterSessionId: 'native-history', nativeSessionId: 'native-history' })
    expect(native.adapter.resumeSession).toHaveBeenCalledTimes(1)

    await store.resumeSession({
      projectKey: '/repo',
      projectPath: '/repo',
      cli: 'claude',
      adapterSessionId: 'history-1',
      nativeSessionId: 'history-1',
    })
    expect(legacy.adapter.resumeSession).toHaveBeenCalledTimes(1)

    expect(() => store.configureAdapters([legacy.adapter, legacy.adapter])).toThrow('DUPLICATE_SESSION_ADAPTER')
  })

  it('UnifiedSessions_ScopedRefreshPreservesOtherProjectAndSelection_001', async () => {
    const adapter = fakeAdapter('legacy-claude', [session({ id: 'a' }), session({ id: 'b', projectKey: '/other', projectPath: '/other' })])
    const store = useUnifiedSessionsStore()
    store.configureAdapters([adapter.adapter])
    await store.refresh()
    await store.activateSession('b')
    await store.renameSession('a', 'Renamed')
    expect(store.sessions.map(value => value.id).sort()).toEqual(['a', 'b'])
    expect(store.activeSessionId).toBe('b')
    adapter.setSessions([session({ id: 'b', projectKey: '/other', projectPath: '/other' })])
    await store.refresh('/repo')
    expect(store.sessions.map(value => value.id)).toEqual(['b'])
  })

  it.each(['legacy-claude', 'native-cli'] as const)('UnifiedSessions_ArchiveRefreshRestore_002 %s', async runtime => {
    const adapter = fakeAdapter(runtime, [session({ id: 'history', runtime, processState: 'stopped' })])
    const store = useUnifiedSessionsStore()
    store.configureAdapters([adapter.adapter])
    await store.refresh()
    await store.archiveSession('history')
    await store.refresh()
    expect(store.sessions.find(value => value.id === 'history')?.archived).toBe(true)
    expect(store.projectGroups.flatMap(group => group.sessions)).toEqual([])
    await store.restoreArchivedSession('history')
    expect(store.projectGroups[0].sessions[0].archived).toBe(false)
  })

  it('UnifiedSessions_ConcurrentScopes_003', async () => {
    const a = deferred<UnifiedSession[]>()
    const b = deferred<UnifiedSession[]>()
    const adapter = fakeAdapter('legacy-claude', [session({ id: 'a' }), session({ id: 'b', projectPath: '/other', projectKey: '/other' })])
    const store = useUnifiedSessionsStore()
    store.configureAdapters([adapter.adapter])
    await store.refresh()
    vi.mocked(adapter.adapter.listSessions).mockImplementation(key => key === '/repo' ? a.promise : b.promise)
    const first = store.refresh('/repo')
    const second = store.refresh('/other')
    b.resolve([session({ id: 'b', title: 'B updated', projectPath: '/other', projectKey: '/other' })])
    await second
    a.resolve([session({ id: 'a', title: 'A updated' })])
    await first
    expect(store.sessions.find(value => value.id === 'a')?.title).toBe('A updated')
    expect(store.sessions.find(value => value.id === 'b')?.title).toBe('B updated')
  })

  it.each([true, false])('UnifiedRefresh_FullScopeOrdering_004 %s', async fullFirst => {
    const full = deferred<UnifiedSession[]>()
    const scoped = deferred<UnifiedSession[]>()
    const adapter = fakeAdapter('legacy-claude', [session({ id: 'a' }), session({ id: 'b', projectPath: '/other', projectKey: '/other' })])
    const store = useUnifiedSessionsStore()
    store.configureAdapters([adapter.adapter])
    await store.refresh()
    vi.mocked(adapter.adapter.listSessions).mockImplementation(key => key === undefined ? full.promise : scoped.promise)
    const first = fullFirst ? store.refresh() : store.refresh('/repo')
    const second = fullFirst ? store.refresh('/repo') : store.refresh()
    full.resolve([session({ id: 'a', title: 'Full' }), session({ id: 'b', title: 'B full', projectPath: '/other', projectKey: '/other' })])
    scoped.resolve([session({ id: 'a', title: 'Scoped' })])
    await Promise.all([first, second])
    expect(store.sessions.find(value => value.id === 'a')?.title).toBe(fullFirst ? 'Scoped' : 'Full')
    expect(store.sessions.find(value => value.id === 'b')?.title).toBe('B full')
  })

  it('creation transfer cannot overtake a newer in-flight activation', async () => {
    const store = useUnifiedSessionsStore()
    const { adapter } = fakeAdapter('native-cli', [session({ id: 'other', runtime: 'native-cli' })])
    const preparing = deferred<CreateUnifiedSessionInput>(); const activating = deferred<void>()
    store.configureAdapters([adapter]); await store.refresh()
    store.configureCreationPreparer(() => preparing.promise)
    const input: CreateUnifiedSessionInput = { cli: 'codex', projectKey: '/repo', projectPath: '/repo' }
    const creating = store.createSession(input)
    ;(adapter.activateSession as ReturnType<typeof vi.fn>).mockReturnValueOnce(activating.promise)
    const activation = store.activateSession('other')
    preparing.resolve(input); const created = await creating
    expect(store.activeSessionId).not.toBe(created.id)
    activating.resolve(); await activation
    expect(store.activeSessionId).toBe('other')
  })

})

// F2重复意图不能在同一显示名称保存尚未确认时再开编辑器并丢失当前保存所有权。
it('Unified_RenameSavingAdmission_014', async () => {
  const value = session(), legacy = fakeAdapter('legacy-claude', [value]), store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter]); await store.refresh(); await store.activateSession(value.id); store.beginRename(value.id)
  const pending = deferred<void>(); legacy.adapter.renameSession = () => pending.promise
  const saving = store.renameSession(value.id, 'New title')
  await store.refresh()
  store.beginRename(value.id); const duringSave = store.sessions.find(row => row.id === value.id)?.renameState
  pending.resolve(); await saving
  expect(duringSave).toBe('saving')
})

it('Unified_RenameCancellationReadLifetime_015', async () => {
  const value = session(), legacy = fakeAdapter('legacy-claude', [value]), store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter]); await store.refresh(); await store.activateSession(value.id); store.beginRename(value.id)
  const reading = deferred<UnifiedSession[]>()
  vi.mocked(legacy.adapter.listSessions).mockImplementationOnce(() => reading.promise)
  const refreshing = store.refresh()
  store.cancelRename(value.id)
  reading.resolve([{ ...value }]); await refreshing
  expect(store.sessions[0].renameState).toBe('idle')
  // A new explicit save gets its own lifetime and cannot restore the canceled editing state.
  await store.renameSession(value.id, 'New display name')
  expect(store.sessions[0].title).toBe('New display name')
  expect(store.sessions[0].renameState).toBe('idle')
})

it('Unified_RenameDisappearanceAndAttempt_016', async () => {
  const value = session(), legacy = fakeAdapter('legacy-claude', [value]), store = useUnifiedSessionsStore()
  let owns = true
  legacy.adapter.captureOwnership = () => () => owns
  store.configureAdapters([legacy.adapter]); await store.refresh(); await store.activateSession(value.id); store.beginRename(value.id)
  owns = false; await store.refresh()
  expect(store.sessions[0].renameState).toBe('idle')
  owns = true; store.beginRename(value.id)
  legacy.setSessions([]); await store.refresh()
  legacy.setSessions([{ ...value }]); await store.refresh()
  expect(store.sessions[0].renameState).toBe('idle')
})

it('Unified_RenameNativeOriginInvalidation_017', async () => {
  const value = session({ runtime: 'native-cli', nativeOrigin: { cli: 'claude', profileId: 'config', profileRevision: '1', projectId: 'registered', projectPath: '/repo' } })
  const native = fakeAdapter('native-cli', [value]), store = useUnifiedSessionsStore()
  store.configureAdapters([native.adapter]); await store.refresh(); await store.activateSession(value.id); store.beginRename(value.id)
  native.setSessions([{ ...value, nativeOrigin: { ...value.nativeOrigin!, profileRevision: '2' } }]); await store.refresh()
  expect(store.sessions[0].renameState).toBe('idle')
})

it('Unified_RenameQueuedSourceAdmission_018', async () => {
  const value = session(), legacy = fakeAdapter('legacy-claude', [value]), store = useUnifiedSessionsStore()
  const stopping = deferred<void>()
  legacy.adapter.stopSession = () => stopping.promise
  store.configureAdapters([legacy.adapter]); await store.refresh(); await store.activateSession(value.id); store.beginRename(value.id)
  const stop = store.stopSession(value.id)
  const save = store.renameSession(value.id, 'Old draft')
  const rejected = expect(save).rejects.toThrow('STALE_SESSION_ATTEMPT')
  await Promise.resolve()
  legacy.setSessions([{ ...value, adapterSessionId: 'replacement', title: 'Replacement' }]); await store.refresh()
  stopping.resolve(); await stop; await rejected
  expect(legacy.adapter.renameSession).not.toHaveBeenCalled()
  expect(store.sessions[0].renameState).toBe('idle')
  store.beginRename(value.id)
  expect(store.sessions[0].renameState).toBe('editing')
})

it('Unified_RenameAttemptBeforeProjection_019', async () => {
  const value = session({ runtime: 'native-cli', cli: 'codex' }), native = fakeAdapter('native-cli', [value]), store = useUnifiedSessionsStore()
  let attempt = 1
  native.adapter.captureOwnership = () => {
    const frozen = attempt
    return () => attempt === frozen
  }
  store.configureAdapters([native.adapter]); await store.refresh(); await store.activateSession(value.id); store.beginRename(value.id)
  // The actual attempt changes before an independent catalog read publishes it.
  attempt = 2
  await expect(store.renameSession(value.id, 'Old attempt draft')).rejects.toThrow('STALE_SESSION_ATTEMPT')
  expect(native.adapter.renameSession).not.toHaveBeenCalled()
  expect(store.sessions[0].renameState).toBe('idle')
  store.beginRename(value.id)
  await store.renameSession(value.id, 'Explicit new edit')
  expect(native.adapter.renameSession).toHaveBeenCalledExactlyOnceWith(value.id, 'Explicit new edit', expect.any(Function), expect.any(Function))
  expect(store.sessions[0].renameState).toBe('idle')
})

// 非当前选择的重命名请求不得自动切换或启动会话，当前选择才允许显示编辑器。
it('Unified_RenameSelectedOnly_020', async () => {
  const value = session(), other = session({ id: 'other', adapterSessionId: 'other' })
  const legacy = fakeAdapter('legacy-claude', [value, other]), store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter]); await store.refresh()
  await store.activateSession(other.id)
  store.beginRename(value.id)
  expect(store.sessions.find(row => row.id === value.id)?.renameState).not.toBe('editing')
  expect(store.activeSessionId).toBe(other.id)
  await store.activateSession(value.id); store.beginRename(value.id)
  expect(store.activeSession?.renameState).toBe('editing')
})

// 切换选择立即撤销旧编辑器；迟到UI提交不能继续写入旧会话。
it('Unified_SelectionRevokesEditor_021', async () => {
  const value = session(), other = session({ id: 'other', adapterSessionId: 'other' })
  const legacy = fakeAdapter('legacy-claude', [value, other]), store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter]); await store.refresh(); await store.activateSession(value.id)
  store.beginRename(value.id)
  await store.activateSession(other.id)
  expect(store.sessions.find(row => row.id === value.id)?.renameState).toBe('idle')
  await expect(store.renameSession(value.id, 'Stale editor', true)).rejects.toThrow('STALE_SESSION_ATTEMPT')
  expect(legacy.adapter.renameSession).not.toHaveBeenCalled()
})

// 显式恢复准入尚未完成时，新选择意图已撤销旧编辑器，不能等activeId切换后才撤销。
it('Unified_ResumeIntentRevokesEdit_022', async () => {
  const value = session({ runtime: 'native-cli' }), native = fakeAdapter('native-cli', [value]), store = useUnifiedSessionsStore()
  store.configureAdapters([native.adapter]); await store.refresh(); await store.activateSession(value.id); store.beginRename(value.id)
  const admission = deferred<UnifiedSession>()
  native.adapter.createSession = () => admission.promise
  const launching = store.launchResume({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', launchConfigId: 'config', launchConfigRevision: '1', action: { kind: 'resume-id', nativeSessionId: 'history' } })
  const stateWhileWaiting = store.sessions.find(row => row.id === value.id)?.renameState
  admission.resolve(session({ runtime: 'native-cli', id: 'resumed', adapterSessionId: 'resumed' })); await launching
  expect(stateWhileWaiting).toBe('idle')
})
