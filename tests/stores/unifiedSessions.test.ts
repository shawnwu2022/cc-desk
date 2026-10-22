import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises } from '@vue/test-utils'
import { watch } from 'vue'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import type { NativeCliTab } from '@/stores/nativeTabs'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'
import type {
  CreateUnifiedSessionInput,
  ResumeUnifiedSessionInput,
  SessionAdapter,
  SessionRuntimeKind,
  UnifiedSession,
} from '@/types/unifiedSession'

// 折叠前提是根身份已证明：测试里把 native 行的来源根设为与该值相同或不同，
// 分别验证「同源折叠」与「无法证明同源时不折叠」。
const LEGACY_ROOT = 'local:windows:1:2:3'

function session(overrides: Partial<UnifiedSession> = {}): UnifiedSession {
  return {
    id: 'legacy-1', renameAvailable: true,
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

// WebView2 实测：打开时间写入尚未返回时，运行行与准备占位行同时显示约 132ms。
it.each(['new', 'raw', 'metadata-failure', 'close-before-save'] as const)('Catalog_CreationHandoffBeforeMetadata_071 %s', async mode => {
  setActivePinia(createPinia())
  const catalog = useUnifiedSessionsStore(), tabs = useNativeTabsStore()
  const saved = deferred<void>()
  const recordSessionOpened = vi.fn(() => saved.promise)
  const adapter = createNativeCliAdapter({
    tabs, history: { all: () => [] },
    metadata: { sessionRecords: new Map(), upsertSessionRecord: recordSessionOpened, recordSessionOpened },
    archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} },
    runtime: {
      createTab: input => tabs.create({ cli: input.cli, projectId: 'project', projectPath: input.projectPath, profileId: 'profile', profileRevision: '1', action: input.action }),
      stopTab: async () => {}, restartTab: async () => { throw new Error('unexpected restart') }, renameTab: async () => {},
    },
  })
  catalog.configureAdapters([adapter])
  const publications: string[][] = []
  const stop = watch(() => catalog.sessions, rows => publications.push(rows.map(row => row.id)), { flush: 'sync' })
  const creating = catalog.createSession({ cli: 'claude', projectKey: '/repo', projectPath: '/repo', action: mode === 'raw' ? { kind: 'raw', argv: [] } : { kind: 'new' } })
  await flushPromises()
  expect(recordSessionOpened).toHaveBeenCalledOnce()
  await catalog.refresh('/repo', { background: true })
  const opened = catalog.sessions.find(row => row.opened)!
  try {
    expect(catalog.sessions).toHaveLength(1)
    expect(catalog.activeSessionId).toBe(opened.id)
    expect(catalog.isPreparingSession(opened.id)).toBe(false)
    if (mode === 'close-before-save') await catalog.closeSession(opened.id)
  } finally {
    if (mode === 'metadata-failure') saved.reject(new Error('write unavailable'))
    else saved.resolve()
    await creating
    stop()
  }
  expect(publications.every(rows => rows.length <= 1)).toBe(true)
  if (mode === 'close-before-save') {
    const removed = publications.findIndex(rows => !rows.length)
    expect(removed).toBeGreaterThanOrEqual(0)
    expect(publications.slice(removed).every(rows => !rows.includes(opened.id))).toBe(true)
    expect(catalog.sessions).toHaveLength(0)
  }
})

it('Catalog_BackgroundRefreshKeepsVisibleLoadingAndRowsStable_046', async () => {
  setActivePinia(createPinia())
  const store = useUnifiedSessionsStore()
  const adapter = fakeAdapter('legacy-claude', [session()])
  store.configureAdapters([adapter.adapter])
  await store.refresh()
  const original = store.sessions[0]
  const wait = deferred<UnifiedSession[]>()
  adapter.adapter.listSessions = () => wait.promise
  const update = store.refresh('/repo', { background: true })
  expect(store.loading).toBe(false)
  expect(store.sessions[0]).toBe(original)
  wait.resolve([session()]); await update
  expect(store.sessions[0]).toBe(original)
})

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

it.each(['root','id','key','project','unknown-root','opened-owners'] as const)('Catalog_PhysicalFoldPreservesDistinctOwners_048 %s',async difference=>{
  setActivePinia(createPinia())
  const a=session({id:'a',runtime:'native-cli',opened:false,processState:'stopped',nativeRootKey:'root',nativeHistorySessionKey:'key',nativeOrigin:{cli:'claude',profileId:'a',profileRevision:'1',projectId:'project',projectPath:'/repo'}})
  const b={...a,id:'b',nativeOrigin:{...a.nativeOrigin!,profileId:'b'}}
  if(difference==='root') b.nativeRootKey='other-root'
  if(difference==='id') b.nativeSessionId='other-id'
  if(difference==='key') b.nativeHistorySessionKey='other-key'
  if(difference==='project') b.nativeOrigin.projectId='other-project'
  if(difference==='unknown-root') delete b.nativeRootKey
  if(difference==='opened-owners'){a.opened=true;b.opened=true}
  const catalog=useUnifiedSessionsStore();catalog.configureAdapters([fakeAdapter('native-cli',[a,b]).adapter]);await catalog.refresh()
  expect(catalog.sessions).toHaveLength(2)
})

// 完整同源证明成立时归档属于同一关闭记录；保留归档 owner，普通孪生行不能复活。
it('Catalog_PhysicalArchiveRetainsArchivedRepresentative_072', async () => {
  setActivePinia(createPinia())
  const a = session({ id:'a', runtime:'native-cli', opened:false, processState:'stopped', nativeRootKey:'root', nativeHistorySessionKey:'key', nativeOrigin:{ cli:'claude',profileId:'a',profileRevision:'1',projectId:'project',projectPath:'/repo' } })
  const b = { ...a, id:'b', archived:true, nativeOrigin:{ ...a.nativeOrigin!,profileId:'b' } }
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([fakeAdapter('native-cli', [a,b]).adapter])
  await catalog.refresh()
  expect(catalog.sessions.map(row => [row.id,row.archived])).toEqual([['b',true]])
})

// CLI 范围请求与项目/全量请求交叠时，迟到读取不能删掉较新的另一范围投影。
it.each([undefined, '/repo'])('Catalog_CliRefreshPreservesNewerRows_073 %s', async project => {
  setActivePinia(createPinia())
  const initial = session({ id:'codex',cli:'codex',runtime:'native-cli',title:'Old',opened:false })
  const native = fakeAdapter('native-cli',[initial]), catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([native.adapter]); await catalog.refresh()
  const old = deferred<UnifiedSession[]>()
  native.adapter.listSessions = vi.fn().mockReturnValueOnce(old.promise).mockResolvedValue([{ ...initial,title:'New' }])
  const pending = catalog.refresh(project)
  await catalog.refresh(undefined,{ cli:'codex' })
  old.resolve([initial]); await pending
  expect(catalog.sessions.find(row => row.id === 'codex')?.title).toBe('New')
})

it.each(['claude', 'codex'] as const)('Catalog_ProjectCliRefreshesRemainIndependent_074 %s', async firstCli => {
  const secondCli = firstCli === 'claude' ? 'codex' : 'claude'
  const first = session({ id: firstCli, cli: firstCli, runtime: 'native-cli', title: 'Old first', opened: false })
  const second = session({ id: secondCli, cli: secondCli, runtime: 'native-cli', title: 'Old second', opened: false })
  const native = fakeAdapter('native-cli', [first, second]), catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([native.adapter]); await catalog.refresh()
  const pendingRows = deferred<UnifiedSession[]>()
  native.adapter.listSessions = vi.fn((_project, cli) => cli === firstCli ? pendingRows.promise : Promise.resolve([{ ...second, title: 'New second' }]))
  const pending = catalog.refresh('/repo', { cli: firstCli })
  await catalog.refresh('/repo', { cli: secondCli })
  pendingRows.resolve([{ ...first, title: 'New first' }]); await pending
  expect(catalog.sessions.find(row => row.cli === firstCli)?.title).toBe('New first')
  expect(catalog.sessions.find(row => row.cli === secondCli)?.title).toBe('New second')
})

it.each(['same-cli', 'project', 'full', 'global-cli'] as const)('Catalog_ProjectCliRefreshPreservesNewerScope_075 %s', async scope => {
  const original = session({ id: 'claude', cli: 'claude', runtime: 'native-cli', title: 'Initial', opened: false })
  const native = fakeAdapter('native-cli', [original]), catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([native.adapter]); await catalog.refresh()
  const stale = deferred<UnifiedSession[]>()
  native.adapter.listSessions = vi.fn().mockReturnValueOnce(stale.promise).mockResolvedValue([{ ...original, title: 'Fresh' }])
  const pending = catalog.refresh('/repo', { cli: 'claude' })
  await catalog.refresh(scope === 'full' || scope === 'global-cli' ? undefined : '/repo',
    scope === 'same-cli' || scope === 'global-cli' ? { cli: 'claude' } : {})
  stale.resolve([{ ...original, title: 'Stale' }]); await pending
  expect(catalog.sessions.find(row => row.id === 'claude')?.title).toBe('Fresh')
})

it.each(['claude', 'codex'] as const)('Catalog_ProjectRefreshKeepsOtherCliAfterScopedUpdate_076 %s', async refreshedCli => {
  const otherCli = refreshedCli === 'claude' ? 'codex' : 'claude'
  const refreshed = session({ id: refreshedCli, cli: refreshedCli, runtime: 'native-cli', title: 'Initial refreshed', opened: false })
  const other = session({ id: otherCli, cli: otherCli, runtime: 'native-cli', title: 'Initial other', opened: false })
  const native = fakeAdapter('native-cli', [refreshed, other]), catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([native.adapter]); await catalog.refresh()
  const projectRows = deferred<UnifiedSession[]>()
  native.adapter.listSessions = vi.fn().mockReturnValueOnce(projectRows.promise).mockResolvedValue([{ ...refreshed, title: 'Fresh scoped' }])
  const pending = catalog.refresh('/repo')
  await catalog.refresh('/repo', { cli: refreshedCli })
  projectRows.resolve([{ ...refreshed, title: 'Stale scoped' }, { ...other, title: 'Fresh other' }]); await pending
  expect(catalog.sessions.find(row => row.cli === refreshedCli)?.title).toBe('Fresh scoped')
  expect(catalog.sessions.find(row => row.cli === otherCli)?.title).toBe('Fresh other')
})

it('Catalog_PhysicalFoldRefreshUsesLinearPreviousLookup_049',async()=>{
  setActivePinia(createPinia())
  const rows=Array.from({length:1000},(_,index)=>session({id:'row-'+index,runtime:'native-cli',opened:false,processState:'stopped',nativeSessionId:'session-'+index,nativeRootKey:'root',nativeHistorySessionKey:'key-'+index,nativeOrigin:{cli:'claude',profileId:'profile',profileRevision:'1',projectId:'project',projectPath:'/repo'}}))
  const catalog=useUnifiedSessionsStore();catalog.configureAdapters([fakeAdapter('native-cli',rows).adapter]);await catalog.refresh()
  let reads=0
  for(const row of catalog.sessions){const id=row.id;Object.defineProperty(row,'id',{configurable:true,enumerable:true,get(){reads++;return id}})}
  await catalog.refresh()
  expect(catalog.sessions).toHaveLength(1000)
  expect(reads).toBeLessThan(1000*20)
})

// 历史显示名编辑不能先恢复 CLI，也不能抢占当前运行终端的选择。
it('Rename_ClosedHistoryEditsWithoutResume_013', async () => {
  setActivePinia(createPinia())
  const history = session({ id: 'history', opened: false, processState: 'stopped' })
  const live = session({ id: 'live', opened: true })
  const fake = fakeAdapter('legacy-claude', [history, live])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([fake.adapter]); await store.refresh()
  store.activeSessionId = live.id
  store.beginRename(history.id)
  expect(store.sessions.find(row => row.id === history.id)?.renameState).toBe('editing')
  // 缺少安全改名能力的 OpenCode 来源仍不得打开编辑器；不再按 CLI 名称一律禁止。
  const opencodeRow = session({ id: 'oc-row', opened: false, processState: 'stopped', cli: 'opencode', renameAvailable: false })
  fakeAdapter('legacy-claude', [opencodeRow])
  store.sessions = [...store.sessions, opencodeRow]
  store.beginRename(opencodeRow.id)
  expect(store.sessions.find(row => row.id === opencodeRow.id)?.renameState).toBe('idle')
  await store.renameSession(history.id, 'History display name', true)
  expect(store.sessions.find(row => row.id === history.id)?.title).toBe('History display name')
  expect(store.activeSessionId).toBe(live.id)
  expect(fake.adapter.resumeSession).not.toHaveBeenCalled()
  expect(fake.adapter.activateSession).not.toHaveBeenCalled()
})

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
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify({ claude: true, codex: true }))
  setActivePinia(createPinia())
})

describe('unified sessions store', () => {
  it('Notice_ClearRejectsChangedReceipt_001', async () => {
    const first = { kind: 'reply-ended' as const, eventId: 'first', receivedAt: 1, runId: 'run-1', generation: 1 }
    const second = { ...first, eventId: 'second', receivedAt: 2, runId: 'run-2', generation: 2 }
    const native = fakeAdapter('native-cli', [session({ id: 'history', runtime: 'native-cli', opened: false,
      observationNotice: { recent: first, unreadReplyEnd: first } })])
    native.adapter.clearUnread = vi.fn()
    const store = useUnifiedSessionsStore(); store.configureAdapters([native.adapter]); await store.refresh()
    native.setSessions([session({ id: 'history', runtime: 'native-cli', opened: false,
      observationNotice: { recent: second, unreadReplyEnd: second } })]); await store.refresh()
    await expect(store.clearUnread('history', first)).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(native.adapter.clearUnread).not.toHaveBeenCalled()
    await store.clearUnread('history', second)
    expect(native.adapter.clearUnread).toHaveBeenCalledWith('history', expect.objectContaining({ observationNotice: { recent: second, unreadReplyEnd: second } }))
  })
  // 项目按固定和显示名排列；后台活动既不能移动项目，也不能移动会话。
  it('StableProjectOrder_Catalog_001', async () => {
    const rows = [
      session({ id: 'zulu', projectPath: '/zulu', lastActivityAt: 100 }),
      session({ id: 'alpha-old', projectPath: '/alpha', lastActivityAt: 1, lastOpenedAt: 1 }),
      session({ id: 'alpha-new', projectPath: '/alpha', lastActivityAt: 2, lastOpenedAt: 2 }),
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
    expect(store.projectGroups[1].sessions.map(row => row.id)).toEqual(['alpha-new', 'alpha-old'])
  })

  it('merges legacy/native sessions into one project group and sorts pinned groups first', async () => {
    const legacy = fakeAdapter('legacy-claude', [
      session({ id: 'legacy', projectPath: '/repo', projectKey: '/repo', lastActivityAt: 100, lastOpenedAt: 100 }),
    ])
    const native = fakeAdapter('native-cli', [
      session({
        id: 'codex',
        runtime: 'native-cli',
        cli: 'codex',
        projectPath: '/repo',
        projectKey: '/repo',
        lastActivityAt: 300,
        lastOpenedAt: 300,
      }),
      session({
        id: 'other',
        runtime: 'native-cli',
        cli: 'codex',
        projectPath: '/other',
        projectKey: '/other',
        lastActivityAt: 400,
        lastOpenedAt: 400,
      }),
    ])
    const projects = useProjectsStateStore()
    projects.pinnedProjects = ['/repo']
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })

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
    store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })

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
    store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
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
    store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })

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
  expect(native.adapter.renameSession).toHaveBeenCalledExactlyOnceWith(value.id, 'Explicit new edit', expect.any(Function), expect.any(Function), expect.any(AbortSignal))
  expect(store.sessions[0].renameState).toBe('idle')
})

// 缺失历史没有有效 adapter owner，不能让用户进入必然失败的编辑。
it('Unified_MissingHistoryRejectsRenameAdmission_035', async () => {
  const value = session({ opened: false, safeErrorCode: 'SESSION_NOT_FOUND' })
  const legacy = fakeAdapter('legacy-claude', [value]), store = useUnifiedSessionsStore()
  legacy.adapter.captureOwnership = () => () => false
  store.configureAdapters([legacy.adapter]); await store.refresh()
  store.beginRename(value.id)
  expect(store.sessions[0].renameState).toBe('idle')
  await expect(store.renameSession(value.id, 'Invalid draft', true)).rejects.toThrow('STALE_SESSION_ATTEMPT')
  expect(legacy.adapter.renameSession).not.toHaveBeenCalled()
})

// 仍拥有同一历史来源的失败保存保留编辑器；用户可修改草稿并显式重试。
it('Unified_OwnedRenameFailureAllowsExplicitRetry_036', async () => {
  const value = session({ opened: false })
  const legacy = fakeAdapter('legacy-claude', [value]), store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter]); await store.refresh(); store.beginRename(value.id)
  vi.mocked(legacy.adapter.renameSession).mockRejectedValueOnce(new Error('REVISION_CONFLICT'))
  await expect(store.renameSession(value.id, 'Recoverable draft', true)).rejects.toThrow('REVISION_CONFLICT')
  expect(store.sessions[0].renameState).toBe('editing')
  expect(store.activeSessionId).toBeNull()
  await store.renameSession(value.id, 'Explicit retry', true)
  expect(store.sessions[0].renameState).toBe('idle')
  expect(store.sessions[0].title).toBe('Explicit retry')
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

// 同一 CLI 数据源（默认 ~/.claude）的同 ID 历史,legacy 与 native 各扫一次会
// 得到两行；显示以 native 行为唯一投影，legacy 历史行让位,归档/恢复能力由
// native adapter 的旧 key 兼容承接。
it('Unified_UnknownRootsKeepClosedRows_023', async () => {
  const legacyRow = session({
    id: 'legacy-history:/repo:sess-1',
    runtime: 'legacy-claude',
    processState: 'stopped',
    adapterSessionId: 'sess-1',
    nativeSessionId: 'sess-1',
    lastActivityAt: 100,
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
    lastActivityAt: 200,
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0].runtime).toBe('native-cli')
  expect(rows[0].lastActivityAt).toBe(200)
})

// 打开终端永不折叠：同 ID 历史在 legacy 端打开时保留可交互的打开行，
// 关闭的 native 孪生行让位（023b）；打开行本身不被任何历史行顶替。
it('Unified_UnknownRootsKeepLiveRow_024', async () => {
  const liveLegacyTab = session({
    id: 'legacy-tab:t1',
    runtime: 'legacy-claude',
    processState: 'running',
    opened: true,
    usesDefaultClaudeRoot: true,
    adapterSessionId: 't1',
    nativeSessionId: 'sess-1',
    lastActivityAt: 300,
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
    lastActivityAt: 200,
  })
  const legacy = fakeAdapter('legacy-claude', [liveLegacyTab])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0].id).toBe('legacy-tab:t1')
})

// 打开的 legacy tab 与关闭的 native 历史是同一会话的两行投影；显示只保留
// 打开行（可选中/可关闭），native 行在其关闭后的刷新中回归。
it('Unified_OpenLegacyTabHidesNativeTwin_023b', async () => {
  const liveLegacyTab = session({
    id: 'legacy-tab:t1',
    runtime: 'legacy-claude',
    processState: 'running',
    opened: true,
    usesDefaultClaudeRoot: true,
    adapterSessionId: 't1',
    nativeSessionId: 'sess-1',
    lastActivityAt: 300,
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
    lastActivityAt: 200,
  })
  const legacy = fakeAdapter('legacy-claude', [liveLegacyTab])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0]).toMatchObject({ id: 'legacy-tab:t1', opened: true })
})

// 折叠必须接管 legacy 行独占的证据：归档保存的是裸 session ID（native 侧不识别），
// 已保存的 Desk 显示名与最近打开时间也只挂在 legacy 记录上。
// 同 UUID 的其他根、配置、修订及注册项目保存名不得覆盖当前来源；使用真实 adapter 解析历史身份。
it.each(['root', 'profile', 'revision', 'project'].flatMap(source => [
  { opened: true, ownSaved: true },
  { opened: true, ownSaved: false },
  { opened: false, ownSaved: false },
].map(state => ({ source, ...state }))))('Unified_FoldNameSource_044 $source/$opened/$ownSaved', async ({ source, opened, ownSaved }) => {
  const legacyRow = session({
    id: 'legacy-history:/repo:sess-3',
    runtime: 'legacy-claude',
    processState: 'stopped',
    title: 'legacy 保存名',
    adapterSessionId: 'sess-3',
    nativeSessionId: 'sess-3',
  })
  const entry: NativeHistoryEntry = {
    key: 'current', context: { cli: 'claude', profileId: 'profile-a', profileRevision: '7', projectId: 'project-1', projectPath: '/repo' },
    sourceRootKey: LEGACY_ROOT, sessions: [{ type: 'session', sessionKey: JSON.stringify(['local', 'claude', LEGACY_ROOT, 'sess-3']), nativeSessionId: 'sess-3', title: 'CLI 标题',
      truncated: false, archived: false, cwd: '/repo', updatedAt: '2026-10-16T00:00:00Z' }],
    loading: false, loaded: true, error: null, requestEpoch: '1',
  }
  const other: NativeHistoryEntry = { ...entry, context: { ...entry.context }, sessions: [{ ...entry.sessions[0] }] }
  if (source === 'root') {
    other.sourceRootKey = 'another-root'
    other.sessions[0].sessionKey = JSON.stringify(['local', 'claude', 'another-root', 'sess-3'])
  } else if (source === 'profile') other.context.profileId = 'another-profile'
  else if (source === 'revision') other.context.profileRevision = '6'
  else other.context.projectId = 'previous-project'
  const projects = useProjectsStateStore(), records = projects.sessionRecords
  const entries = [entry], tabs = new Map<string, NativeCliTab>()
  const native = createNativeCliAdapter({
    tabs: { tabs, activeTabId: null, setActive: vi.fn(), close: vi.fn(), rename: vi.fn() },
    history: { all: () => entries }, metadata: projects,
    runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: vi.fn() },
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
  })
  const ownHistory = (await native.listSessions())[0]
  entries[0] = other
  const otherHistory = (await native.listSessions())[0]
  entries[0] = entry
  records.set(otherHistory.id, {
    runtime: 'native-cli', cli: 'claude', projectPath: '/repo',
    adapterSessionId: other.sessions[0].sessionKey, nativeSessionId: 'sess-3',
    title: '其他来源保存名', lastActivityAt: 400,
  })
  if (ownSaved) records.set(ownHistory.id, {
    runtime: 'native-cli', cli: 'claude', projectPath: '/repo',
    adapterSessionId: entry.sessions[0].sessionKey, nativeSessionId: 'sess-3',
    title: 'native 保存名', lastActivityAt: 400,
  })
  if (opened) tabs.set('tab-1', {
    tabId: 'tab-1', cli: 'claude', projectPath: '/repo', projectId: 'project-1', profileId: 'profile-a', profileRevision: '7',
    requestId: 'request-1', runId: 'run-1', generation: 1, action: { kind: 'resume-id', nativeSessionId: 'sess-3' },
    sourceSessionKey: entry.sessions[0].sessionKey, status: 'running', errorCode: null, launchRevision: '7',
    title: 'CLI 标题', createdAt: 1, lastActivityAt: 500,
  })
  records.set(legacyRow.id, {
    runtime: 'legacy-claude', cli: 'claude', projectPath: '/repo',
    adapterSessionId: 'sess-3', nativeSessionId: 'sess-3',
    title: 'legacy 保存名', lastActivityAt: 100,
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0]).toMatchObject({ title: ownSaved ? 'native 保存名' : 'legacy 保存名', opened })
  records.delete(ownHistory.id)
  records.delete(legacyRow.id)
  await store.refresh()
  expect(store.sessions[0].title).toBe('CLI 标题')
})

// 折叠必须接管 legacy 行独占的证据：归档（存裸 session ID）、显式保存的 Desk
// 覆盖名与最近打开时间（键是 legacy 历史键）。显式覆盖名优先于 CLI 记录的标题，
// 但没有保存记录时保持 native 的标题，不把 legacy 的普通标题当覆盖名。
it('Unified_FoldingAdoptsLegacyArchiveAndMetadata_041', async () => {
  const legacyRow = session({
    id: 'legacy-history:/repo:sess-1',
    runtime: 'legacy-claude',
    processState: 'stopped',
    title: '本地改过的名字',
    lastOpenedAt: 900,
    adapterSessionId: 'sess-1',
    nativeSessionId: 'sess-1',
    archived: true,
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    title: 'CLI 记录的标题',
    lastOpenedAt: 100,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
    archived: false,
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  // legacy 键上有一条显式保存的 Desk 覆盖名。
  useProjectsStateStore().sessionRecords.set(legacyRow.id, {
    runtime: 'legacy-claude', cli: 'claude', projectPath: '/repo',
    adapterSessionId: 'sess-1', nativeSessionId: 'sess-1',
    title: '我改的名字', lastActivityAt: 100, lastOpenedAt: 700,
  })
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0]).toMatchObject({ runtime: 'native-cli', archived: true, lastOpenedAt: 900 })
  expect(rows[0].title).toBe('我改的名字')
})

// 没有显式保存的覆盖名时，接管不得把 legacy 的普通标题当成 Desk 名。
it('Unified_FoldingWithoutSavedNameKeepsNativeTitle_043', async () => {
  const legacyRow = session({
    id: 'legacy-history:/repo:sess-2',
    runtime: 'legacy-claude',
    processState: 'stopped',
    title: '首条消息摘要',
    adapterSessionId: 'sess-2',
    nativeSessionId: 'sess-2',
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-2',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    title: 'CLI 记录的标题',
    adapterSessionId: 'native-history-key-2',
    nativeSessionId: 'sess-2',
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0].title).toBe('CLI 记录的标题')
})

// legacy 终端本次启动跑在非默认根（CLAUDE_CONFIG_DIR 覆盖）时，同 UUID 的默认根
// native 历史不是同一来源，不得被隐藏。
it('Unified_LegacyTabOnCustomRootKeepsNativeRow_042', async () => {
  const liveLegacyTab = session({
    id: 'legacy-tab:t1',
    runtime: 'legacy-claude',
    processState: 'running',
    opened: true,
    adapterSessionId: 't1',
    nativeSessionId: 'sess-1',
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
  })
  const legacy = fakeAdapter('legacy-claude', [liveLegacyTab])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  expect(store.sessions.filter(row => row.projectKey === '/repo').map(row => row.runtime).sort())
    .toEqual(['legacy-claude', 'native-cli'])
})

// 同 ID 历史去重后归档状态由 native 行承担；legacy 注册的继承在 native
// adapter 的 archiveState 内完成（旧 key 兼容），store 层按其产物显示。
it('Unified_UnknownRootsKeepArchiveState_025', async () => {
  const legacyRow = session({
    id: 'legacy-history:/repo:sess-1',
    runtime: 'legacy-claude',
    processState: 'stopped',
    adapterSessionId: 'sess-1',
    nativeSessionId: 'sess-1',
    archived: true,
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
    archived: true,
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0].archived).toBe(true)
})

// 自然退出后仍打开的 legacy 终端（opened=true、processState=stopped）是真实
// 终端 owner：底层 tab 仍在，永远不被折叠，用户可继续选中与关闭；活动选中
// 跨 refresh 保持。同 ID 的关闭 native 历史行让位（023b），关闭后回归。
it('Unified_UnknownRootsKeepOpenedTerminal_026', async () => {
  const legacyRow = session({
    id: 'legacy-tab:t1',
    runtime: 'legacy-claude',
    processState: 'stopped',
    opened: true,
    usesDefaultClaudeRoot: true,
    adapterSessionId: 't1',
    nativeSessionId: 'sess-1',
    lastActivityAt: 300,
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    opened: false,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
    lastActivityAt: 200,
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  store.activeSessionId = 'legacy-tab:t1'
  await store.refresh()
  expect(store.sessions.map(value => value.id).sort()).toEqual(['legacy-tab:t1'])
  expect(store.activeSessionId).toBe('legacy-tab:t1')
})

// 同运行时不同 profile/来源身份保留各自准确的恢复来源。
it('Unified_NativeOriginsRemainDistinct_027', async () => {
  const first = session({
    id: 'nh-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    opened: false,
    adapterSessionId: 'k1',
    nativeSessionId: 'sess-1',
  })
  const second = session({
    id: 'nh-2',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    opened: false,
    adapterSessionId: 'k2',
    nativeSessionId: 'sess-1',
  })
  const native = fakeAdapter('native-cli', [first, second])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([native.adapter])
  await store.refresh()
  expect(store.sessions.map(value => value.id).sort()).toEqual(['nh-1', 'nh-2'])
})

// 同 ID 历史去重后,恢复入口收敛到 native 行(其 archiveState 已兼容 legacy
// 注册表),不再保留第二个 legacy 恢复入口。
it('Unified_UnknownRootsKeepRestoreOwner_028', async () => {
  const legacyRow = session({
    id: 'legacy-history:/repo:sess-1',
    runtime: 'legacy-claude',
    processState: 'stopped',
    adapterSessionId: 'sess-1',
    nativeSessionId: 'sess-1',
    archived: true,
  })
  const nativeRow = session({
    id: 'native-history:/repo:sess-1',
    runtime: 'native-cli',
    processState: 'stopped',
    nativeRootKey: LEGACY_ROOT,
    adapterSessionId: 'native-history-key',
    nativeSessionId: 'sess-1',
    archived: true,
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  const rows = store.sessions.filter(row => row.projectKey === '/repo')
  expect(rows).toHaveLength(1)
  expect(rows[0].archived).toBe(true)
  await store.restoreArchivedSession(rows[0].id)
  expect(native.adapter.restoreArchivedSession).toHaveBeenCalledWith('native-history:/repo:sess-1')
  expect(legacy.adapter.restoreArchivedSession).not.toHaveBeenCalled()
})

// 同项目同 session ID 的多条历史去重为 native 行;不同 profile 的 native 行
// 仍保持各自来源身份与恢复入口。
it('Unified_MixedOriginsStayDistinct_029', async () => {
  const legacyRow: UnifiedSession = {
    id: 'legacy-history:/repo:sess-1', projectKey: '/repo', projectPath: '/repo',
    cli: 'claude', runtime: 'legacy-claude', title: 'Legacy history',
    processState: 'stopped', attentionState: 'none', lastActivityAt: 100,
    archived: false, opened: false, resumable: true, adapterSessionId: 'sess-1',
    nativeSessionId: 'sess-1', launchConfigId: null,
  }
  const first: UnifiedSession = {
    id: 'native-history:profile-a', projectKey: '/repo', projectPath: '/repo',
    cli: 'claude', runtime: 'native-cli', title: 'Profile A history',
    processState: 'stopped', attentionState: 'none', lastActivityAt: 200,
    archived: false, opened: false, resumable: true,
    adapterSessionId: '["local","claude","root-a","sess-1"]', nativeSessionId: 'sess-1',
    nativeRootKey: LEGACY_ROOT,
    launchConfigId: 'profile-a',
    nativeOrigin: { cli: 'claude', profileId: 'profile-a', profileRevision: '7', projectId: 'project-1', projectPath: '/repo' },
  }
  const second: UnifiedSession = {
    id: 'native-history:profile-b', projectKey: '/repo', projectPath: '/repo',
    cli: 'claude', runtime: 'native-cli', title: 'Profile B history',
    processState: 'stopped', attentionState: 'none', lastActivityAt: 300,
    archived: false, opened: false, resumable: true,
    adapterSessionId: '["local","claude","root-b","sess-1"]', nativeSessionId: 'sess-1',
    nativeRootKey: 'local:windows:9:9:9',
    launchConfigId: 'profile-b',
    nativeOrigin: { cli: 'claude', profileId: 'profile-b', profileRevision: '9', projectId: 'project-1', projectPath: '/repo' },
  }
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [first, second])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  expect(store.sessions.map(row => row.id).sort()).toEqual([
    'native-history:profile-a', 'native-history:profile-b',
  ])
  expect(store.projectGroups[0].sessions.filter(row => row.runtime === 'native-cli').map(row => [
    row.launchConfigId, row.nativeOrigin?.profileRevision, row.adapterSessionId,
  ]).sort()).toEqual([
    ['profile-a', '7', '["local","claude","root-a","sess-1"]'],
    ['profile-b', '9', '["local","claude","root-b","sess-1"]'],
  ])
})

// 同 ID 归档去重后由 native 行作为唯一恢复入口，恢复即解除唯一归档。
it('Unified_BothArchivesKeepOwners_030', async () => {
  const legacyRow: UnifiedSession = {
    id: 'legacy-history:/repo:sess-1', projectKey: '/repo', projectPath: '/repo',
    cli: 'claude', runtime: 'legacy-claude', title: 'Legacy archive',
    processState: 'stopped', attentionState: 'none', lastActivityAt: 100,
    archived: true, opened: false, resumable: true, adapterSessionId: 'sess-1',
    nativeSessionId: 'sess-1', launchConfigId: null,
  }
  const nativeRow: UnifiedSession = {
    id: 'native-history:profile-a', projectKey: '/repo', projectPath: '/repo',
    cli: 'claude', runtime: 'native-cli', title: 'Native archive',
    processState: 'stopped', attentionState: 'none', lastActivityAt: 200,
    archived: true, opened: false, resumable: true,
    adapterSessionId: '["local","claude","root-a","sess-1"]', nativeSessionId: 'sess-1',
    nativeRootKey: LEGACY_ROOT,
    launchConfigId: 'profile-a',
    nativeOrigin: { cli: 'claude', profileId: 'profile-a', profileRevision: '7', projectId: 'project-1', projectPath: '/repo' },
  }
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  expect(store.sessions.filter(row => row.archived).map(row => row.id)).toEqual(['native-history:profile-a'])
  expect(store.projectGroups).toEqual([])
  await store.restoreArchivedSession('native-history:profile-a')
  // Desk 归档注册表是两端共享的唯一来源：native 恢复同时清除旧 key 兼容条目，
  // legacy 适配器下一轮读到的就不再是归档行（真实适配器行为）。
  legacy.setSessions([{ ...legacyRow, archived: false }])
  await store.refresh()
  expect(store.sessions.some(row => row.archived)).toBe(false)
  expect(store.projectGroups[0].sessions.map(row => row.id)).toEqual(['native-history:profile-a'])
})

// 旧存储值没有 opencode 键：迁移默认停用，既有的 claude/codex 偏好保持不变。
it('CliSupport_LegacyStorageDefaultsOpencode_01', async () => {
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify({ claude: true, codex: false }))
  try {
    const { useCliSupportStore } = await import('@/stores/cliSupport')
    const store = useCliSupportStore()
    expect(store.isEnabled('opencode')).toBe(false)
    expect(store.isEnabled('codex')).toBe(false)
    expect(store.isEnabled('claude')).toBe(true)
  } finally {
    localStorage.removeItem('cc-desk-cli-support-v1')
  }
})

// opencode 准备行使用自己的显示名，不能落回 Codex CLI 的兜底分支。
it('Unified_OpencodePreparingRowTitle_01', async () => {
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify({ opencode: true }))
  const store = useUnifiedSessionsStore()
  store.configureAdapters([fakeAdapter('native-cli').adapter])
  store.configureCreationPreparer(() => new Promise<CreateUnifiedSessionInput>(() => {}))
  void store.createSession({ cli: 'opencode', projectKey: '/repo', projectPath: '/repo' })
  expect(store.sessions[0]?.title).toBe('opencode')
})

it('CliSupport_OpencodeCatalogTogglePreservesOpenTerminals_032', async () => {
  localStorage.removeItem('cc-desk-cli-support-v1')
  const { useCliSupportStore } = await import('@/stores/cliSupport')
  const closed = session({ id: 'oc-history', runtime: 'native-cli', cli: 'opencode', opened: false, processState: 'stopped' })
  const opened = session({ id: 'oc-open', runtime: 'native-cli', cli: 'opencode', opened: true, processState: 'running' })
  const store = useUnifiedSessionsStore()
  store.configureAdapters([fakeAdapter('native-cli', [closed, opened]).adapter])
  await store.refresh()
  expect(store.sessions.map(row => row.id)).toEqual(['oc-open'])
  const support = useCliSupportStore()
  support.setCliEnabled('opencode', true)
  await store.refresh()
  expect(store.sessions.map(row => row.id).sort()).toEqual(['oc-history', 'oc-open'])
  support.setCliEnabled('opencode', false)
  await store.refresh()
  expect(store.sessions.map(row => row.id)).toEqual(['oc-open'])
})

// CLI 支持开关关闭的 CLI 不列出历史行；已打开终端是活进程，保留可选中与关闭。
it('UnifiedSessions_DisabledCliHidesHistoryKeepsOpenTabs_029', async () => {
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify({ claude: true, codex: false }))
  try {
    const codexHistory = session({
      id: 'native-history:/repo:codex-sess', runtime: 'native-cli', cli: 'codex',
      processState: 'stopped', opened: false, adapterSessionId: 'codex-key', nativeSessionId: 'codex-sess',
    })
    const codexTab = session({
      id: 'native-tab:t1', runtime: 'native-cli', cli: 'codex',
      processState: 'running', opened: true, adapterSessionId: 't1', nativeSessionId: 'codex-live',
    })
    const claudeRow = session({ id: 'claude-history', runtime: 'legacy-claude', processState: 'stopped', nativeSessionId: 'claude-sess' })
    const native = fakeAdapter('native-cli', [codexHistory, codexTab])
    const legacy = fakeAdapter('legacy-claude', [claudeRow])
    const store = useUnifiedSessionsStore()
    store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
    await store.refresh()
    const ids = store.sessions.map(value => value.id).sort()
    expect(ids).toEqual(['claude-history', 'native-tab:t1'])
  } finally {
    localStorage.removeItem('cc-desk-cli-support-v1')
  }
})

// 目录层的支持过滤：停用 CLI 的非打开行在 refresh 时被剔除，重新启用后恢复。
// 切换触发的历史重装载由组合根负责（unifiedWorkspaceRuntime 冷缓存回归）。
it('UnifiedSessions_CliSupportFiltersCatalogRows_030', async () => {
  const { useCliSupportStore } = await import('@/stores/cliSupport')
  const codexRow = session({
    id: 'native-history:/repo:codex-sess', runtime: 'native-cli', cli: 'codex',
    processState: 'stopped', opened: false, adapterSessionId: 'codex-key', nativeSessionId: 'codex-sess',
  })
  const native = fakeAdapter('native-cli', [codexRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([native.adapter])
  await store.refresh()
  expect(store.sessions).toHaveLength(1)
  useCliSupportStore().setCliEnabled('codex', false)
  await store.refresh()
  expect(store.sessions).toHaveLength(0)
  useCliSupportStore().setCliEnabled('codex', true)
  await store.refresh()
  expect(store.sessions.map(value => value.id)).toEqual(['native-history:/repo:codex-sess'])
  localStorage.removeItem('cc-desk-cli-support-v1')
})

// 恢复返回 SESSION_NOT_FOUND 后的缺失记录与普通行同一支持过滤，停用 CLI 的
// missing 行不得在手动刷新后继续显示。
it('UnifiedSessions_MissingRecordHonorsCliSupport_031', async () => {
  const { useCliSupportStore } = await import('@/stores/cliSupport')
  const codexRow = session({
    id: 'native-history:/repo:codex-miss', runtime: 'native-cli', cli: 'codex',
    processState: 'stopped', opened: false, adapterSessionId: 'codex-miss', nativeSessionId: 'codex-miss',
    launchConfigId: 'p',
  })
  const native = fakeAdapter('native-cli', [codexRow])
  native.adapter.resumeSession = vi.fn(async () => { throw new Error('SESSION_NOT_FOUND') })
  const store = useUnifiedSessionsStore()
  store.configureAdapters([native.adapter])
  await store.refresh()
  await expect(store.resumeCatalogSession('native-history:/repo:codex-miss')).rejects.toThrow('SESSION_NOT_FOUND')
  expect(store.sessions).toHaveLength(1)
  useCliSupportStore().setCliEnabled('codex', false)
  await store.refresh()
  expect(store.sessions).toHaveLength(0)
  localStorage.removeItem('cc-desk-cli-support-v1')
})

// 同 ID 归档去重后,恢复入口收敛到 native 行；legacy 行不再单独保留。
// native 行来自独立配置根（与 legacy 固定根不同）时不得折叠：两个恢复入口各自保留。
it('Unified_OneEachRootKeepsRestoreOrigins_040', async () => {
  const legacyRow = session({ id: 'legacy-origin', runtime: 'legacy-claude', processState: 'stopped', opened: false, archived: true, adapterSessionId: 'same-id', nativeSessionId: 'same-id' })
  const nativeRow = session({ id: 'native-origin', runtime: 'native-cli', processState: 'stopped', opened: false, archived: true, nativeRootKey: 'local:windows:9:9:9', adapterSessionId: '["local","claude","independent-root","same-id"]', nativeSessionId: 'same-id', launchConfigId: 'independent-profile', nativeOrigin: { cli: 'claude', profileId: 'independent-profile', profileRevision: '1', projectId: 'project-1', projectPath: '/repo' } })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  expect(store.sessions.map(row => row.id).sort()).toEqual(['legacy-origin', 'native-origin'])
  await store.restoreArchivedSession('legacy-origin')
  expect(legacy.adapter.restoreArchivedSession).toHaveBeenCalledWith('legacy-origin')
  expect(native.adapter.restoreArchivedSession).not.toHaveBeenCalled()
})

// 多个 profile 指向同一根时，Native 无法接管裸归档键，必须保留 Legacy 恢复 owner。
it('Unified_AmbiguousTwinKeepsLegacyRestore_045', async () => {
  const legacyRow = session({ id: 'legacy-origin', runtime: 'legacy-claude', processState: 'stopped', opened: false, archived: true, adapterSessionId: 'same-id', nativeSessionId: 'same-id' })
  const entries: NativeHistoryEntry[] = ['profile-a', 'profile-b'].map(profileId => ({
    key: profileId, sourceRootKey: LEGACY_ROOT,
    context: { cli: 'claude', profileId, profileRevision: '7', projectId: profileId, projectPath: '/repo' },
    sessions: [{ type: 'session', sessionKey: JSON.stringify(['local', 'claude', LEGACY_ROOT, 'same-id']), nativeSessionId: 'same-id', title: profileId, truncated: false, archived: false, cwd: '/repo', updatedAt: null }],
    loading: false, loaded: true, error: null, requestEpoch: '1',
  }))
  const native = createNativeCliAdapter({
    tabs: { tabs: new Map(), activeTabId: null, setActive: vi.fn(), close: vi.fn(), rename: vi.fn() },
    history: { all: () => entries }, legacyClaudeRootKey: () => LEGACY_ROOT,
    runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: vi.fn() },
    archive: { getArchivedSessions: () => ['same-id'], archiveSession: vi.fn(), restoreSession: vi.fn() },
  })
  const legacy = fakeAdapter('legacy-claude', [legacyRow]), store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  expect(store.sessions.filter(row => row.safeErrorCode === 'SESSION_ORIGIN_AMBIGUOUS')).toHaveLength(2)
  expect(store.sessions.find(row => row.id === legacyRow.id)).toMatchObject({ archived: true })
  await store.restoreArchivedSession(legacyRow.id)
  expect(legacy.adapter.restoreArchivedSession).toHaveBeenCalledWith(legacyRow.id)
})

// 批量操作只承诺确认时的本项目owner，失败不归档，也不追逐后来新增的会话。
it('Catalog_ProjectCloseFreezesOwnersAndContinuesAfterFailure_047', async () => {
  setActivePinia(createPinia());const store=useUnifiedSessionsStore()
  let rows=[session({id:'legacy-tab:a',adapterSessionId:'a',opened:true}),session({id:'legacy-tab:b',adapterSessionId:'b',opened:true}),session({id:'legacy-tab:other',adapterSessionId:'other',projectPath:'/other',projectKey:'/other',opened:true}),session({id:'history',opened:false,processState:'stopped'})]
  const closed:string[]=[],archived=vi.fn(),owners=new Map(rows.map(row=>[row.id,{}]))
  const adapter={runtime:'legacy-claude',listSessions:async()=>rows,captureOwnership:(id:string)=>{const owner=owners.get(id);return()=>owners.get(id)===owner},activateSession:vi.fn(),closeSession:async(id:string)=>{closed.push(id);if(id==='legacy-tab:a')throw new Error('STOP_FAILED');rows=rows.filter(row=>row.id!==id)},archiveSession:archived} as unknown as SessionAdapter
  store.configureAdapters([adapter]);await store.refresh()
  const close=store.captureProjectClose('/repo')
  rows.push(session({id:'legacy-tab:late',adapterSessionId:'late',opened:true}));await store.refresh()
  await expect(close()).rejects.toThrow('PROJECT_CLOSE_PARTIAL')
  expect(closed).toEqual(['legacy-tab:a','legacy-tab:b']);expect(archived).not.toHaveBeenCalled()
  expect(store.sessions.map(row=>row.id)).toEqual(expect.arrayContaining(['legacy-tab:other','legacy-tab:late','history','legacy-tab:a']))
})
it('Catalog_ProjectCloseDoesNotCloseReplacementGeneration_048',async()=>{
  setActivePinia(createPinia());const store=useUnifiedSessionsStore();const row=session({id:'legacy-tab:a',adapterSessionId:'a',opened:true}),close=vi.fn();let owner={}
  store.configureAdapters([{runtime:'legacy-claude',listSessions:async()=>[row],captureOwnership:()=>{const expected=owner;return()=>owner===expected},closeSession:close} as unknown as SessionAdapter]);await store.refresh()
  const batch=store.captureProjectClose('/repo');owner={}
  await expect(batch()).rejects.toThrow('PROJECT_CLOSE_PARTIAL');expect(close).not.toHaveBeenCalled()
})

it('Catalog_ProjectCloseCancellationStopsNextDispatch_049',async()=>{
  setActivePinia(createPinia());const store=useUnifiedSessionsStore(),wait=deferred<void>();let active=true
  let rows=[session({id:'legacy-tab:a',adapterSessionId:'a',opened:true}),session({id:'legacy-tab:b',adapterSessionId:'b',opened:true})]
  const close=vi.fn(async(id:string)=>{await wait.promise;rows=rows.filter(row=>row.id!==id)})
  store.configureAdapters([{runtime:'legacy-claude',listSessions:async()=>rows,captureOwnership:()=>()=>true,activateSession:vi.fn(),closeSession:close} as unknown as SessionAdapter]);await store.refresh()
  const closing=store.captureProjectClose('/repo')(()=>active)
  await flushPromises();expect(close).toHaveBeenCalledTimes(1)
  active=false;wait.resolve();await expect(closing).rejects.toThrow('ACTION_CANCELLED')
  expect(close).toHaveBeenCalledTimes(1);expect(store.sessions.some(row=>row.id==='legacy-tab:b')).toBe(true)
})
it('Catalog_ProjectCloseDoesNotActivateIntermediateTargets_050',async()=>{
  setActivePinia(createPinia());const store=useUnifiedSessionsStore()
  let rows=['a','b','other'].map(id=>session({id:'legacy-tab:'+id,adapterSessionId:id,opened:true,projectPath:id==='other'?'/other':'/repo',projectKey:id==='other'?'/other':'/repo'}))
  const activate=vi.fn()
  store.configureAdapters([{runtime:'legacy-claude',listSessions:async()=>rows,captureOwnership:()=>()=>true,activateSession:activate,
    closeSession:async(id:string)=>{rows=rows.filter(row=>row.id!==id)}} as unknown as SessionAdapter]);await store.refresh()
  store.activeSessionId='legacy-tab:a';await store.captureProjectClose('/repo')()
  expect(activate.mock.calls).toEqual([['legacy-tab:other']])
  expect(store.activeSessionId).toBe('legacy-tab:other')
})

it('Catalog_LegacyArchiveOwnerSurvivesOpenedNativeFold_061', async () => {
  setActivePinia(createPinia())
  const legacyRow = session({ id: 'legacy-archive', adapterSessionId: 'same-id', nativeSessionId: 'same-id', opened: false, archived: true, processState: 'stopped' })
  const nativeRow = session({ id: 'native-tab:open', adapterSessionId: 'open', runtime: 'native-cli', opened: true, nativeSessionId: 'same-id',
    nativeRootKey: LEGACY_ROOT, nativeHistorySessionKey: 'same-key', nativeOrigin: { cli: 'claude', profileId: 'one', profileRevision: '1', projectId: 'project', projectPath: '/repo' } })
  const legacy = fakeAdapter('legacy-claude', [legacyRow])
  const native = fakeAdapter('native-cli', [nativeRow, ...['one', 'two'].map(profileId => ({ ...nativeRow, id: `native-history:${profileId}`, adapterSessionId: 'same-key', opened: false, processState: 'stopped' as const, nativeOrigin: { ...nativeRow.nativeOrigin!, profileId } }))])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  expect(store.sessions.map(row => row.id).sort()).toEqual(['legacy-archive', 'native-tab:open'])
  expect(store.sessions.find(row => row.id === legacyRow.id)).toMatchObject({ archived: true })
  await store.restoreArchivedSession(legacyRow.id)
  expect(legacy.adapter.restoreArchivedSession).toHaveBeenCalledWith(legacyRow.id)
  expect(native.adapter.restoreArchivedSession).not.toHaveBeenCalled()
})

it.each(['a', 'b'])('Catalog_ProjectCloseSuppressesJoinedIndividualFallback_062 %s', async selected => {
  setActivePinia(createPinia())
  const store = useUnifiedSessionsStore()
  const waits = { a: deferred<void>(), b: deferred<void>() }
  let rows = ['a', 'b', 'other'].map(id => session({ id: `legacy-tab:${id}`, adapterSessionId: id, opened: true,
    projectPath: id === 'other' ? '/other' : '/repo', projectKey: id === 'other' ? '/other' : '/repo' }))
  const activate = vi.fn()
  const close = vi.fn(async (id: string) => {
    await waits[id.slice('legacy-tab:'.length) as 'a' | 'b'].promise
    rows = rows.filter(row => row.id !== id)
  })
  store.configureAdapters([{ runtime: 'legacy-claude', listSessions: async () => rows, captureOwnership: (id: string) => { const row = rows.find(value => value.id === id); return () => !!row && rows.includes(row) },
    activateSession: activate, closeSession: close } as unknown as SessionAdapter])
  await store.refresh()
  store.activeSessionId = `legacy-tab:${selected}`
  const single = store.closeSession(`legacy-tab:${selected}`)
  await flushPromises()
  const batch = store.captureProjectClose('/repo')()
  await flushPromises()
  waits[selected as 'a' | 'b'].resolve()
  await single
  await flushPromises()
  const intermediateActivations = [...activate.mock.calls]
  waits[selected === 'a' ? 'b' : 'a'].resolve()
  await batch
  expect(intermediateActivations).toEqual([])
  expect(activate.mock.calls).toEqual([['legacy-tab:other']])
  expect(store.activeSessionId).toBe('legacy-tab:other')
  expect(close.mock.calls.map(([id]) => id).sort()).toEqual(['legacy-tab:a', 'legacy-tab:b'])
})

it.each(['empty', 'different-owner'])('Catalog_ProjectCloseRejectsActualOwnerInventoryMismatch_063 %s', async catalogState => {
  setActivePinia(createPinia())
  const store = useUnifiedSessionsStore()
  const tabs = useNativeTabsStore()
  const actual = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'profile', profileRevision: '1', action: { kind: 'new' } })
  const close = vi.fn()
  const activate = vi.fn()
  const stale = session({ id: 'native-tab:stale', adapterSessionId: 'stale', runtime: 'native-cli', cli: 'codex', opened: true })
  store.configureAdapters([{ runtime: 'native-cli', listSessions: async () => catalogState === 'empty' ? [] : [stale],
    captureOwnership: () => () => true, closeSession: close, activateSession: activate } as unknown as SessionAdapter])
  await store.refresh()
  store.activeSessionId = 'native-tab:stale'
  const management = useProjectManagementStore()
  expect(management.openCount('/repo')).toBe(1)
  management.beginCloseSessions({ projectKey: '/repo', projectPath: '/repo' })
  await management.closeSessions()
  expect(management.dialog?.kind).toBe('close-sessions')
  expect(management.error).toBe('projectCloseSessionsPartial')
  expect(tabs.tab(actual.tabId)).toBeDefined()
  expect(store.activeSessionId).toBe('native-tab:stale')
  expect(close).not.toHaveBeenCalled()
  expect(activate).not.toHaveBeenCalled()
})

it('Catalog_ProjectCloseDoesNotTreatMissingProjectionAsClosedOwner_064', async () => {
  setActivePinia(createPinia())
  const store = useUnifiedSessionsStore()
  const wait = deferred<void>()
  let rows = ['a', 'b'].map(id => session({ id: `legacy-tab:${id}`, adapterSessionId: id, opened: true }))
  const owners = new Set(rows.map(row => row.id))
  const close = vi.fn(async (id: string) => { await wait.promise; owners.delete(id); rows = rows.filter(row => row.id !== id) })
  store.configureAdapters([{ runtime: 'legacy-claude', listSessions: async () => rows, captureOwnership: (id: string) => () => owners.has(id),
    closeSession: close } as unknown as SessionAdapter])
  await store.refresh()
  const closing = store.captureProjectClose('/repo')()
  await flushPromises()
  rows = []
  await store.refresh('/repo')
  wait.resolve()
  await expect(closing).rejects.toThrow('PROJECT_CLOSE_PARTIAL')
  expect(close.mock.calls).toEqual([['legacy-tab:a', expect.any(Function)]])
  expect(owners.has('legacy-tab:b')).toBe(true)
})

it('Catalog_UnarchivedLegacyFoldsIntoUniqueOpenedNative_065', async () => {
  setActivePinia(createPinia())
  const legacyRow = session({ id: 'legacy-history', adapterSessionId: 'same-id', nativeSessionId: 'same-id', opened: false, processState: 'stopped' })
  const nativeRow = session({ id: 'native-tab:open', adapterSessionId: 'open', runtime: 'native-cli', opened: true,
    nativeSessionId: 'same-id', nativeRootKey: LEGACY_ROOT, nativeHistorySessionKey: 'same-key' })
  const legacy = fakeAdapter('legacy-claude', [legacyRow]), native = fakeAdapter('native-cli', [nativeRow])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([legacy.adapter, native.adapter], { legacyClaudeRootKey: LEGACY_ROOT })
  await store.refresh()
  expect(store.sessions).toHaveLength(1)
  expect(store.sessions[0]).toMatchObject({ id: nativeRow.id, adapterSessionId: 'open', opened: true, archived: false })
  await store.closeSession(nativeRow.id)
  expect(native.adapter.closeSession).toHaveBeenCalledWith(nativeRow.id, expect.any(Function))
  expect(legacy.adapter.closeSession).not.toHaveBeenCalled()
})

it('Catalog_ProjectCloseConfirmationCaptureLeavesIndividualFallback_066', async () => {
  setActivePinia(createPinia())
  const store = useUnifiedSessionsStore(), wait = deferred<void>(), activate = vi.fn()
  let rows = ['a', 'b'].map(id => session({ id: `legacy-tab:${id}`, adapterSessionId: id, opened: true }))
  store.configureAdapters([{ runtime: 'legacy-claude', listSessions: async () => rows,
    captureOwnership: (id: string) => { const row = rows.find(value => value.id === id); return () => !!row && rows.includes(row) },
    activateSession: activate, closeSession: async (id: string) => { await wait.promise; rows = rows.filter(row => row.id !== id) },
  } as unknown as SessionAdapter])
  await store.refresh()
  store.activeSessionId = 'legacy-tab:a'
  const single = store.closeSession('legacy-tab:a')
  await flushPromises()
  store.captureProjectClose('/repo')
  wait.resolve()
  await single
  expect(activate.mock.calls).toEqual([['legacy-tab:b']])
  expect(store.activeSessionId).toBe('legacy-tab:b')
})

it.each(['catalog', 'adapter'] as const)('Rename_ConfirmedWriteSurvivesDisplayFailure_067 %s', async failureAt => {
  setActivePinia(createPinia())
  const value = session({ id: 'native-history:one', runtime: 'native-cli', opened: false, processState: 'stopped' })
  const native = fakeAdapter('native-cli', [value]), legacy = fakeAdapter('legacy-claude', [])
  const store = useUnifiedSessionsStore()
  store.configureAdapters([native.adapter, legacy.adapter]); await store.refresh(); store.beginRename(value.id)
  vi.mocked(native.adapter.renameSession).mockImplementation(async (_id, _title, _current, issued) => {
    issued?.()
    if (failureAt === 'adapter') throw new Error('RENAME_DISPLAY_SYNC_FAILED')
    return 'native-title-saved'
  })
  if (failureAt === 'catalog') vi.mocked(legacy.adapter.listSessions).mockRejectedValueOnce(new Error('SOURCE_UNAVAILABLE'))
  await expect(store.renameSession(value.id, 'Confirmed native title', true)).rejects.toThrow('RENAME_DISPLAY_SYNC_FAILED')
  expect(store.sessions.find(row => row.id === value.id)?.renameState).toBe('idle')
  await expect(store.renameSession(value.id, 'Confirmed native title', true)).rejects.toThrow('STALE_SESSION_ATTEMPT')
  await store.refresh('/repo')
  expect(native.adapter.renameSession).toHaveBeenCalledTimes(1)
})
