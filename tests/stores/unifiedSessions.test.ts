import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
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
    listSessions: vi.fn(async () => [...current]),
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
    archiveSession: vi.fn(async id => { current = current.filter(value => value.id !== id) }),
    restoreArchivedSession: vi.fn(async () => undefined),
  }
  return {
    adapter,
    setSessions(next: UnifiedSession[]) { current = [...next] },
  }
}

beforeEach(() => {
  vi.restoreAllMocks()
  setActivePinia(createPinia())
})

describe('unified sessions store', () => {
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

  it('routes create/resume to native for Codex and legacy for Claude without duplicating adapters', async () => {
    const legacy = fakeAdapter('legacy-claude')
    const native = fakeAdapter('native-cli')
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
})
