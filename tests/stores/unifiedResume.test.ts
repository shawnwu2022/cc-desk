import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useSessionStore } from '@/stores/session'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'

afterEach(() => vi.unstubAllGlobals())
beforeEach(() => { vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy-tab' }); clearMocks(); setActivePinia(createPinia()); useProjectsStateStore().loaded = true })
const sourceKey = JSON.stringify(['local', 'codex', 'root', 'same-id'])
const entry = (profileId = 'cx', revision = '7'): NativeHistoryEntry => ({ key: profileId + revision,
  context: { cli: 'codex', profileId, profileRevision: revision, projectId: 'project', projectPath: '/repo' },
  sessions: [{ type: 'session', sessionKey: sourceKey, nativeSessionId: 'same-id', title: 'History', cwd: '/repo', updatedAt: '2026-09-30T00:00:00Z', truncated: false }],
  loaded: true, loading: false, error: null, requestEpoch: '1', absenceEvidence: { cli: 'codex', sourceRootKey: 'root' } })

describe('Unified resume', () => {
  // 相同来源 Session ID 在不同配置修订下仍拥有独立目录身份。
  it('Resume_PreservesFullOrigin_001', async () => {
    const tabs = useNativeTabsStore(); const catalog = useUnifiedSessionsStore()
    const otherProject = entry(); otherProject.context.projectId = 'second-registration'
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [entry('cx', '7'), entry('cx', '8'), entry('other', '7'), otherProject] },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })
    catalog.configureAdapters([adapter]); await catalog.refresh()
    expect(catalog.sessions).toHaveLength(4)
    expect(new Set(catalog.sessions.map(row => row.id)).size).toBe(4)
  })
  // 同时点击同一来源只准入一个终端，未知结果保留同一个尝试。
  it('Resume_CoalescesAdmission_002', async () => {
    const tabs = useNativeTabsStore(); const source = entry(); let release!: () => void
    const barrier = new Promise<void>(resolve => { release = resolve })
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [source] }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
      runtime: { async createTab(input) { await barrier; return tabs.create({ cli: input.cli, projectId: 'project', projectPath: input.projectPath, profileId: 'cx', profileRevision: '7', action: input.action, sourceSessionKey: input.sourceSessionKey }) }, restartTab: vi.fn(), stopTab: vi.fn() } })
    const input = { runtime: 'native-cli' as const, projectKey: '/repo', projectPath: '/repo', cli: 'codex' as const, adapterSessionId: sourceKey, nativeSessionId: 'same-id' }
    const a = adapter.resumeSession(input); const b = adapter.resumeSession(input); release()
    const results = await Promise.all([a, b]); expect(results[0].id).toBe(results[1].id); expect(tabs.tabs.size).toBe(1)
    tabs.markUnknown(results[0].adapterSessionId)
    expect((await adapter.resumeSession(input)).id).toBe(results[0].id); expect(tabs.tabs.size).toBe(1)
  })
  // 缓存中不存在的来源不得用当前配置猜测恢复。
  it('Resume_RejectsMissingOrigin_003', async () => {
    const tabs = useNativeTabsStore()
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [] }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
      runtime: { createTab(input) { return tabs.create({ cli: input.cli, projectId: 'project', projectPath: input.projectPath, profileId: 'cx', profileRevision: '7', action: input.action }) }, restartTab: vi.fn(), stopTab: vi.fn() } })
    await expect(adapter.resumeSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', adapterSessionId: 'missing', nativeSessionId: 'missing' })).rejects.toThrow('SESSION_NOT_FOUND')
    expect(tabs.tabs.size).toBe(0)
  })
  // 旧会话占用仅过滤同一个项目；目录端口仍包含已归档记录。
  it('Resume_LegacyClaimsAreScoped_004', async () => {
    mockIPC((command, args) => command === 'get_sessions' ? [{ sessionId: 'same', name: 'Other project', projectPath: (args as any).projectPath, lastActiveAt: 1 }] : undefined)
    const legacy = useSessionStore(); legacy.createTab('/one', { sessionId: 'same' }); await legacy.loadHistoryFor('/two')
    expect(legacy.getHistoryFor('/two').map(row => row.sessionId)).toEqual(['same'])
    useProjectsStateStore().archivedSessions.set('/two', ['same'])
    expect(legacy.getHistoryFor('/two')).toEqual([]); expect(legacy.getCatalogHistoryFor('/two')).toHaveLength(1)
    await legacy.loadHistorySessions('/two'); expect(legacy.historySessions).toEqual([])
  })
})

// 历史分页读取完整后才能判定会话不存在，重复来源页不引入重复行。
it('Resume_ReadsAllNativePages_005', async () => {
  const { useNativeHistoryStore } = await import('@/stores/nativeHistory')
  const { createNativeProjectionClient } = await import('@/api/tauri')
  // Use the real authenticated client with a protocol-shaped document boundary.
  const source = { scopeId: 'scope', instanceId: 'instance', cli: 'codex', sourceRootKey: 'root', identityEpoch: '1', profileId: 'cx', profileRevision: '7', target: { kind: 'profile', profileId: 'cx', expectedProfileRevision: '7', projectId: 'project' }, basis: 'configured-profile' }
  const offsets: number[] = []
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', async invoke(command: string, q: any) {
    if (command === 'native_get_scope') return source
    offsets.push(q.offset)
    const id = q.offset === 0 ? 'first' : 'last'
    return { source: q.source, resourceKind: q.resourceKind, requestEpoch: q.requestEpoch, observedAt: '1', state: 'ready', reason: null,
      items: [{ type: 'session', sessionKey: JSON.stringify(['local', 'codex', 'root', id]), nativeSessionId: id, title: id, truncated: false, cwd: '/repo', updatedAt: null }], hasMore: q.offset === 0 }
  } } })
  try {
    expect(createNativeProjectionClient).toBeDefined()
    const loaded = await useNativeHistoryStore().load(entry().context)
    expect(loaded.sessions.map(row => row.nativeSessionId)).toEqual(['first', 'last'])
    expect(offsets).toEqual([0, 1])
  } finally { delete (window as any).__CC_DESK_DOCUMENT__ }
})

// 缺失记录删除仅移除精确来源的应用元数据，不调用历史文件删除接口。
it('Resume_RemovesOnlyMissingRecord_006', async () => {
  const source = entry(); const other = entry('other'); const tabs = useNativeTabsStore(); const projects = useProjectsStateStore()
  let absent = false; const reads: string[] = []; const commands: string[] = []
  const adapter = createNativeCliAdapter({ tabs, history: { all: () => [source, other], async load(context) { reads.push(context.profileId); return { ...source, sessions: absent ? [] : source.sessions } } },
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([adapter]); await catalog.refresh()
  const target = catalog.sessions.find(row => row.launchConfigId === 'cx')!; const sibling = catalog.sessions.find(row => row.launchConfigId === 'other')!
  const records = { [target.id]: { runtime: target.runtime, cli: target.cli, projectPath: target.projectPath, adapterSessionId: target.adapterSessionId, nativeSessionId: target.nativeSessionId, title: target.title, lastActivityAt: 1 }, [sibling.id]: { runtime: sibling.runtime, cli: sibling.cli, projectPath: sibling.projectPath, adapterSessionId: sibling.adapterSessionId, nativeSessionId: sibling.nativeSessionId, title: sibling.title, lastActivityAt: 1 } }
  Object.entries(records).forEach(([key, value]) => projects.sessionRecords.set(key, value))
  mockIPC((command, args) => { commands.push(command); if (command === 'remove_session_ui_record') { delete records[(args as any).recordKey]; return { pinnedProjects: [], archivedSessions: {}, sessionRecords: records } } throw new Error('unexpected destructive command') })
  absent = true
  await expect(catalog.resumeCatalogSession(target)).rejects.toThrow('SESSION_NOT_FOUND')
  await catalog.removeMissingRecord(target.id)
  expect(reads).toEqual(['cx', 'cx']); expect(commands).toEqual(['remove_session_ui_record'])
  expect(catalog.sessions.map(row => row.id)).toEqual([sibling.id]); expect([...projects.sessionRecords.keys()]).toEqual([sibling.id])
})

// 删除前重新发现会话时必须保留记录，且恢复其可用目录状态。
it('Resume_RechecksMissingEvidence_007', async () => {
  const source = entry(); const tabs = useNativeTabsStore(); let present = false
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([createNativeCliAdapter({ tabs,
    history: { all: () => [source], async load() { return { ...source, sessions: present ? source.sessions : [] } } },
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })])
  await catalog.refresh(); const target = catalog.sessions[0]
  await expect(catalog.resumeCatalogSession(target)).rejects.toThrow('SESSION_NOT_FOUND')
  present = true
  await expect(catalog.removeMissingRecord(target.id)).rejects.toThrow('SESSION_EXISTS')
  expect(catalog.sessions.find(row => row.id === target.id)?.safeErrorCode).toBeNull()
})

// 旧归档身份仅在唯一匹配时兼容；存在两个配置来源时不猜测移除哪一个。
it('Resume_PreservesOldArchiveKeys_008', async () => {
  const { makeSessionCatalogKey } = await import('@/utils/sessionPresentation')
  const source = entry(); let entries = [source]
  const old = 'native-history:' + makeSessionCatalogKey({ runtime: 'native-cli', cli: 'codex', projectPath: '/repo', adapterSessionId: sourceKey, nativeSessionId: 'same-id' })
  let keys = [old]; const changed: string[] = []
  const adapter = createNativeCliAdapter({ tabs: useNativeTabsStore(), history: { all: () => entries },
    archive: { getArchivedSessions: () => keys, archiveSession: vi.fn(), async restoreSession(_path, id) { changed.push(id); keys = keys.filter(key => key !== id) } }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })
  const [unique] = await adapter.listSessions(); expect(unique.archived).toBe(true)
  await adapter.restoreArchivedSession(unique.id); expect(keys).toEqual([]); expect(changed).toEqual([old])
  entries = [source, entry('other')]; keys = [old]; changed.length = 0
  const rows = await adapter.listSessions(); expect(rows).toHaveLength(2)
  expect(rows.every(row => row.archived && row.safeErrorCode === 'SESSION_ORIGIN_AMBIGUOUS')).toBe(true)
  await expect(adapter.restoreArchivedSession(rows[0].id)).rejects.toThrow('SESSION_ORIGIN_AMBIGUOUS')
  expect(keys).toEqual([old]); expect(changed).toEqual([])
})

// 搜索标题和 Session ID，默认项目范围与全部项目范围保留 CLI 和时间过滤。
it('Resume_SearchesScopedHistory_009', async () => {
  const first = entry(); const second = entry('other'); second.context.projectPath = '/other'; second.sessions[0].title = 'Elsewhere'; second.sessions[0].nativeSessionId = 'older-id'; second.sessions[0].updatedAt = '2025-01-01T00:00:00Z'
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([createNativeCliAdapter({ tabs: useNativeTabsStore(), history: { all: () => [first, second] }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })])
  expect((await catalog.searchSessions({ projectPath: '/repo', scope: 'current-project' })).sessions.map(row => row.title)).toEqual(['History'])
  expect((await catalog.searchSessions({ projectPath: '/repo', scope: 'all', query: 'older-ID' })).sessions.map(row => row.title)).toEqual(['Elsewhere'])
  expect((await catalog.searchSessions({ projectPath: '/repo', scope: 'all', query: 'history', cli: 'codex', since: Date.parse('2026-01-01') })).sessions).toHaveLength(1)
  expect((await catalog.searchSessions({ projectPath: '/repo', scope: 'all', cli: 'claude' })).sessions).toHaveLength(0)
})

// 应用元数据尚未载入时先读 canonical 状态，再移除精确记录。
it('Resume_LoadsMetadataBeforeRemove_010', async () => {
  const source = entry(); const tabs = useNativeTabsStore(); let absent = false
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([createNativeCliAdapter({ tabs, history: { all: () => [source], async load() { return { ...source, sessions: absent ? [] : source.sessions } } },
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })])
  await catalog.refresh(); const target = catalog.sessions[0]; absent = true
  await expect(catalog.resumeCatalogSession(target)).rejects.toThrow('SESSION_NOT_FOUND')
  const projects = useProjectsStateStore(); projects.loaded = false
  const commands: string[] = []
  mockIPC((command) => { commands.push(command); return { pinnedProjects: [], archivedSessions: {}, sessionRecords: command === 'get_projects_state' ? { [target.id]: { runtime: target.runtime, cli: target.cli, projectPath: target.projectPath, adapterSessionId: target.adapterSessionId, nativeSessionId: target.nativeSessionId, title: target.title, lastActivityAt: 1 } } : {} } })
  await catalog.removeMissingRecord(target.id)
  expect(commands).toEqual(['get_projects_state', 'remove_session_ui_record'])
})

// 校验历史期间另一个入口已打开同源会话时，恢复必须接管原有尝试。
it('Resume_RechecksOpenAfterRead_011', async () => {
  const source = entry(); const tabs = useNativeTabsStore(); let release!: () => void
  const barrier = new Promise<void>(resolve => { release = resolve })
  const adapter = createNativeCliAdapter({ tabs, history: { all: () => [source], async load() { await barrier; return source } },
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab(input) { return tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: input.action, sourceSessionKey: sourceKey }) }, restartTab: vi.fn(), stopTab: vi.fn() } })
  const pending = adapter.resumeSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', adapterSessionId: sourceKey, nativeSessionId: 'same-id', nativeOrigin: source.context })
  const opened = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'resume-id', nativeSessionId: 'same-id' }, sourceSessionKey: sourceKey }); tabs.markUnknown(opened.tabId)
  release(); const resumed = await pending
  expect(resumed.adapterSessionId).toBe(opened.tabId); expect(tabs.tabs.size).toBe(1); expect(tabs.tab(opened.tabId)?.status).toBe('unknown')
})

// 缺失来源重新可用后显式恢复成功，旧错误行不得继续与已打开会话重复显示。
it('Resume_ClearsRecoveredMissingRow_012', async () => {
  const source = entry(); const tabs = useNativeTabsStore(); let present = false
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([createNativeCliAdapter({ tabs, history: { all: () => [source], async load() { return { ...source, sessions: present ? source.sessions : [] } } },
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab(input) { return tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', sourceSessionKey: input.sourceSessionKey, action: input.action }) }, restartTab: vi.fn(), stopTab: vi.fn() } })])
  await catalog.refresh(); const target = catalog.sessions[0]
  await expect(catalog.resumeCatalogSession(target)).rejects.toThrow('SESSION_NOT_FOUND')
  present = true; const opened = await catalog.resumeCatalogSession(target)
  await catalog.refresh()
  expect(catalog.sessions.map(row => row.id)).toEqual([opened.id])
})

// 旧会话历史检查期间已有 Tab 认领同一个会话时，不调用第二次启动。
it('Resume_LegacyRechecksOpenClaim_013', async () => {
  const { createLegacyClaudeAdapter } = await import('@/session/adapters/legacyClaudeAdapter')
  const legacy = useSessionStore(); let release!: () => void
  const barrier = new Promise<void>(resolve => { release = resolve }); const started: string[] = []
  const adapter = createLegacyClaudeAdapter({ store: { ...legacy, async loadHistoryFor() { await barrier; return { ok: true as const, sessions: [{ sessionId: 'same', name: 'Saved', projectPath: '/repo', lastActiveAt: 1 }] } } }, projectPaths: () => ['/repo'],
    runtime: { async startTab(id) { started.push(id) }, stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn() } })
  const pending = adapter.resumeSession({ runtime: 'legacy-claude', cli: 'claude', projectKey: '/repo', projectPath: '/repo', adapterSessionId: 'same', nativeSessionId: 'same' })
  const tabId = legacy.createTab('/repo', { sessionId: 'same', name: 'Already open' }); release()
  expect((await pending).id).toBe('legacy-tab:' + tabId)
  expect(started).toEqual([]); expect(legacy.tabs.get(tabId)?.name).toBe('Already open')
})

// 同一配置/项目的物理根目录替换，不能证明原始根中的会话已不存在。
it('Resume_RejectsChangedSourceAbsence_014', async () => {
  const { useNativeHistoryStore } = await import('@/stores/nativeHistory')
  let root = 'original'
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', async invoke(command: string, q: any) {
    if (command === 'native_get_scope') return { scopeId: 'scope-' + root, instanceId: 'instance', cli: 'codex', sourceRootKey: root, identityEpoch: '1', profileId: 'cx', profileRevision: '7', target: { kind: 'profile', profileId: 'cx', expectedProfileRevision: '7', projectId: 'project' }, basis: 'configured-profile' }
    return { source: q.source, resourceKind: 'history', requestEpoch: q.requestEpoch, observedAt: '1', state: 'ready', reason: null,
      items: [{ type: 'session', sessionKey: JSON.stringify(['local', 'codex', root, 'saved']), nativeSessionId: 'saved', title: 'Saved', cwd: '/repo', updatedAt: null, truncated: false }], hasMore: false }
  } } })
  try {
    const history = useNativeHistoryStore(); const tabs = useNativeTabsStore(); const catalog = useUnifiedSessionsStore()
    catalog.configureAdapters([createNativeCliAdapter({ history, tabs, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })])
    await history.load(entry().context); await catalog.refresh(); const chosen = catalog.sessions[0]
    root = 'replacement'
    await expect(catalog.resumeCatalogSession(chosen)).rejects.toThrow('SOURCE_CHANGED')
    await expect(catalog.removeMissingRecord(chosen.id)).rejects.toThrow('SESSION_MISSING_NOT_VERIFIED')
    expect(tabs.tabs.size).toBe(0); expect(catalog.sessions.some(row => row.safeErrorCode === 'SESSION_NOT_FOUND')).toBe(false)
  } finally { delete (window as any).__CC_DESK_DOCUMENT__ }
})

// 非快照 offset 分页可能漏掉排序移动的会话，完整页链也不能授权缺失移除。
it('Resume_RejectsPagedAbsence_015', async () => {
  const { useNativeHistoryStore } = await import('@/stores/nativeHistory')
  let scanning = false
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', async invoke(command: string, q: any) {
    if (command === 'native_get_scope') return { scopeId: 'scope', instanceId: 'instance', cli: 'codex', sourceRootKey: 'root', identityEpoch: '1', profileId: 'cx', profileRevision: '7', target: { kind: 'profile', profileId: 'cx', expectedProfileRevision: '7', projectId: 'project' }, basis: 'configured-profile' }
    const ids = !scanning ? ['saved'] : q.offset === 0 ? Array.from({ length: 200 }, (_, i) => 'other-' + i) : ['other-199']
    return { source: q.source, resourceKind: 'history', requestEpoch: q.requestEpoch, observedAt: '1', state: 'ready', reason: null,
      items: ids.map(id => ({ type: 'session', sessionKey: JSON.stringify(['local', 'codex', 'root', id]), nativeSessionId: id, title: id, cwd: '/repo', updatedAt: null, truncated: false })), hasMore: scanning && q.offset === 0 }
  } } })
  try {
    const history = useNativeHistoryStore(); const tabs = useNativeTabsStore(); const catalog = useUnifiedSessionsStore()
    catalog.configureAdapters([createNativeCliAdapter({ history, tabs, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })])
    await history.load(entry().context); await catalog.refresh(); const chosen = catalog.sessions[0]; scanning = true
    await expect(catalog.resumeCatalogSession(chosen)).rejects.toThrow('HISTORY_ABSENCE_UNVERIFIED')
    await expect(catalog.removeMissingRecord(chosen.id)).rejects.toThrow('SESSION_MISSING_NOT_VERIFIED')
    expect(tabs.tabs.size).toBe(0); expect(history.all()[0].sessions).toHaveLength(200)
  } finally { delete (window as any).__CC_DESK_DOCUMENT__ }
})

// Legacy 同源复用异步检查时，新确认不能继承已取消调用方的 guard。
it('Resume_LegacyReconfirmOwnsAdmission_016', async () => {
  const { createLegacyClaudeAdapter } = await import('@/session/adapters/legacyClaudeAdapter')
  const legacy = useSessionStore(); let release!: () => void; let oldCurrent = true; let starts = 0
  const barrier = new Promise<void>(resolve => { release = resolve })
  const adapter = createLegacyClaudeAdapter({ store: { ...legacy, async loadHistoryFor() { await barrier; return { ok: true as const, sessions: [{ sessionId: 'saved', name: 'Saved', projectPath: '/repo', lastActiveAt: 1 }] } } }, projectPaths: () => ['/repo'],
    runtime: { async startTab() { starts++ }, stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn() } })
  const input = { runtime: 'legacy-claude' as const, cli: 'claude' as const, projectKey: '/repo', projectPath: '/repo', adapterSessionId: 'saved', nativeSessionId: 'saved' }
  const first = adapter.resumeSession(input, () => oldCurrent).catch(error => error)
  oldCurrent = false
  const fresh = adapter.resumeSession(input, () => true)
  release(); const opened = await fresh; expect((await first).message).toBe('RESTORE_CANCELLED')
  expect(opened.nativeSessionId).toBe('saved'); expect(legacy.tabs.size).toBe(1); expect(starts).toBe(1)
})

// 已证明缺失后，移除前的来源替换撤销证据；恢复原来源的完整单页可重新证明缺失。
it('Resume_RevalidatesAbsenceProof_017', async () => {
  const { useNativeHistoryStore } = await import('@/stores/nativeHistory')
  let root = 'original'; let present = true
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', async invoke(command: string, q: any) {
    if (command === 'native_get_scope') return { scopeId: 'scope-' + root, instanceId: 'instance', cli: 'codex', sourceRootKey: root, identityEpoch: '1', profileId: 'cx', profileRevision: '7', target: { kind: 'profile', profileId: 'cx', expectedProfileRevision: '7', projectId: 'project' }, basis: 'configured-profile' }
    return { source: q.source, resourceKind: 'history', requestEpoch: q.requestEpoch, observedAt: '1', state: 'ready', reason: null,
      items: present ? [{ type: 'session', sessionKey: JSON.stringify(['local', 'codex', root, 'saved']), nativeSessionId: 'saved', title: 'Saved', cwd: '/repo', updatedAt: null, truncated: false }] : [], hasMore: false }
  } } })
  try {
    const history = useNativeHistoryStore(); const tabs = useNativeTabsStore(); const catalog = useUnifiedSessionsStore()
    catalog.configureAdapters([createNativeCliAdapter({ history, tabs, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })])
    await history.load(entry().context); await catalog.refresh(); const chosen = catalog.sessions[0]; present = false
    await expect(catalog.resumeCatalogSession(chosen)).rejects.toThrow('SESSION_NOT_FOUND')
    root = 'replacement'
    await expect(catalog.removeMissingRecord(chosen.id)).rejects.toThrow('SOURCE_CHANGED')
    expect(catalog.sessions.some(row => row.id === chosen.id)).toBe(true)
    root = 'original'; await catalog.removeMissingRecord(chosen.id)
    expect(catalog.sessions).toEqual([]); expect(tabs.tabs.size).toBe(0)
  } finally { delete (window as any).__CC_DESK_DOCUMENT__ }
})

// 待定恢复在统一目录发布禁用态；相同精确来源复用一次准入，失败后清除禁用态。
it('Resume_PendingDeduplicates_023', async () => {
  const catalog = useUnifiedSessionsStore()
  const target = { id: 'history-pending', runtime: 'native-cli' as const, cli: 'codex' as const,
    projectKey: '/repo', projectPath: '/repo', adapterSessionId: 'history', nativeSessionId: 'history',
    title: 'History', processState: 'stopped' as const, attentionState: 'none' as const,
    archived: false, opened: false, resumable: true, lastActivityAt: 1 }
  let fail!: (error: Error) => void
  let calls = 0
  catalog.configureAdapters([{ runtime: 'native-cli', listSessions: async () => [target],
    resumeSession: async () => { calls++; return new Promise<typeof target>((_resolve, reject) => { fail = reject }) },
  } as unknown as import('@/types/unifiedSession').SessionAdapter])
  await catalog.refresh()
  const first = catalog.resumeCatalogSession(target).catch(() => undefined)
  const second = catalog.resumeCatalogSession(target).catch(() => undefined)
  expect(catalog.projectGroups[0].sessions[0].resumePending).toBe(true)
  expect(calls).toBe(1)
  await catalog.refresh()
  expect(catalog.projectGroups[0].sessions[0].resumePending).toBe(true)
  fail(new Error('SESSION_NOT_FOUND'))
  await Promise.all([first, second])
  expect(catalog.projectGroups[0].sessions[0].resumePending).toBe(false)
})

// 归档恢复待定期间复用精确来源的一次 metadata 写入，失败后允许显式重试。
it('RestoreArchive_PendingCoalesces_024', async () => {
  const catalog = useUnifiedSessionsStore()
  const target = { id: 'archive-pending', runtime: 'native-cli' as const, cli: 'codex' as const,
    projectKey: '/repo', projectPath: '/repo', adapterSessionId: 'archive', nativeSessionId: 'archive',
    title: 'Archive', processState: 'stopped' as const, attentionState: 'none' as const,
    archived: true, opened: false, resumable: true, lastActivityAt: 1 }
  let fail!: (error: Error) => void
  let calls = 0
  const admission = new Promise<void>((_resolve, reject) => { fail = reject })
  catalog.configureAdapters([{ runtime: 'native-cli', listSessions: async () => [target],
    restoreArchivedSession: async () => { calls++; await admission },
  } as unknown as import('@/types/unifiedSession').SessionAdapter])
  await catalog.refresh()
  const first = catalog.restoreArchivedSession(target.id).catch(() => undefined)
  const second = catalog.restoreArchivedSession(target.id).catch(() => undefined)
  expect(catalog.isResumePending(target)).toBe(true)
  await flushPromises()
  expect(calls).toBe(1)
  fail(new Error('COMMIT_STATE_UNKNOWN'))
  await Promise.all([first, second])
  expect(calls).toBe(1)
  expect(catalog.isResumePending(target)).toBe(false)
  expect(catalog.sessions[0].archived).toBe(true)
})
