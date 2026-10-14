import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

// 执行 adapter 与 metadata writer 的真实函数体；已有 CLI 草稿由独立 writer 模型持有。
const metadataSource = readFileSync('src/session/sessionMetadata.ts', 'utf8')
function metadataFunction(name, parameters) {
  const body = metadataSource.match(new RegExp('export async function ' + name + '\\([^]*?\\): Promise<void> \\{([\\s\\S]*?)\\n\\}'))?.[1]
  assert.ok(body)
  return Function('matchesSessionMetadata', 'return async function(' + parameters + '){' + body + '}')(() => true)
}
const saveSessionDisplayName = metadataFunction('saveSessionDisplayName', 'metadata,identity,title,owns,onIssued')
const clearSessionDisplayName = metadataFunction('clearSessionDisplayName', 'metadata,identity,owns,onIssued')

function harness(kind, failWrite = false, failMetadata = false) {
  const source = readFileSync('src/session/adapters/' + (kind === 'native' ? 'nativeCliAdapter.ts' : 'legacyClaudeAdapter.ts'), 'utf8')
  const body = source.match(/  async function renameSession\([^]*?\): Promise<SessionRenameResult> \{([\s\S]*?)\n  \}/)?.[1]
  assert.ok(body)
  const id = (kind === 'native' ? 'native-tab:' : 'legacy-tab:') + 'tab'
  const identity = { id, runtime: kind === 'native' ? 'native-cli' : 'legacy-claude',
    cli: 'claude', projectPath: '/repo', adapterSessionId: 'tab', lastActivityAt: 1 }
  let draft = 'unfinished private draft'
  const writes = []
  const metadata = { sessionRecords: new Map([[id, { ...identity, title: 'Old override' }]]),
    async upsertSessionRecord(key, record, before) {
      before?.()
      if (failMetadata) throw Error('REVISION_CONFLICT')
      this.sessionRecords.set(key, record)
    } }
  const tab = { tabId: 'tab', cli: 'claude', status: 'running', title: 'Original', name: 'Original' }
  const deps = { metadata, tabs: { rename(_id, title) { tab.title = title } },
    runtime: { async renameTab(_id, title) {
      if (failWrite) throw Error('PTY_PAUSED')
      const text = '/rename ' + title + '\r'; writes.push(text); draft += text
    } } }
  const store = { updateTabName(_id, title) { tab.name = title } }
  const rename = Function('deps', 'store', 'captureOwnership', 'requireTab', 'tabDisplayIdentity',
    'saveSessionDisplayName', 'clearSessionDisplayName', 'ACTIVE_PREFIX',
    'return async function(id,title,canContinue=()=>true,onIssued,signal){' + body + '}')(
    deps, store, () => () => true, () => tab, () => identity,
    saveSessionDisplayName, clearSessionDisplayName, kind === 'native' ? 'native-tab:' : 'legacy-tab:')
  return { writes, metadata, id, tab, draft: () => draft, rename: () => rename(id, 'New name') }
}
for (const kind of ['native', 'legacy']) {
  test('Rename_DraftNeverSubmitted_001 ' + kind, async () => {
    const h = harness(kind)
    assert.equal(await h.rename(), 'display-name-saved')
    assert.deepEqual(h.writes, [])
    assert.equal(h.draft(), 'unfinished private draft')
    assert.equal(h.metadata.sessionRecords.get(h.id).title, 'New name')
  })
  test('Rename_PausedPtyDoesNotOwnMetadata_002 ' + kind, async () => {
    const h = harness(kind, true)
    assert.equal(await h.rename(), 'display-name-saved')
    assert.deepEqual(h.writes, [])
    assert.equal(h.metadata.sessionRecords.get(h.id).title, 'New name')
  })
  test('Rename_MetadataConflictKeepsDraftAndOldName_003 ' + kind, async () => {
    const h = harness(kind, false, true)
    await assert.rejects(h.rename(), /REVISION_CONFLICT/)
    assert.deepEqual(h.writes, [])
    assert.equal(h.draft(), 'unfinished private draft')
    assert.equal(h.metadata.sessionRecords.get(h.id).title, 'Old override')
  })
}

function catalogRenameHarness(owns = () => true) {
  const source = readFileSync('src/stores/unifiedSessions.ts', 'utf8')
  const sessions = { value: [{ id: 'history', title: 'Old title', opened: false, renameState: 'idle' }] }
  const renameOwners = new Map(), pendingRenames = new Map()
  const captureRenameOwner = (_row, state) => ({ controller: new AbortController(), state, owns })
  const ownsRename = (owner, row) => !!row && owner.owns()
  const beginBody = source.match(/  function beginRename\(id: string\) \{([\s\S]*?)\n  \}/)?.[1]
  const cancelBody = source.match(/  function cancelRename\(id: string\) \{([\s\S]*?)\n  \}/)?.[1]
  const renameBody = source.match(/  function renameSession\([^]*?\): Promise<[^]*?> \{([\s\S]*?)\n  \}/)?.[1]
  assert.ok(beginBody && cancelBody && renameBody)
  const cancel = Function('pendingRenames', 'renameOwners', 'sessions', 'return function(id){' + cancelBody + '}')(pendingRenames, renameOwners, sessions)
  const begin = Function('sessions', 'activeSessionId', 'pendingRenames', 'isPreparingSession', 'renameOwners', 'ownsRename', 'captureRenameOwner',
    'return function(id){' + beginBody + '}')(sessions, { value: 'live' }, pendingRenames, () => false, renameOwners, ownsRename, captureRenameOwner)
  const adapter = { async renameSession() { throw Error('REVISION_CONFLICT') } }
  const rename = Function('requireSession', 'adapterForRuntime', 'renameOwners', 'pendingRenames', 'activeSessionId', 'ownsRename', 'sessions', 'captureRenameOwner', 'enqueue', 'refresh', 'cancelRename',
    'return function(id,title,requireEditor=false){' + renameBody + '}')(
    id => sessions.value.find(row => row.id === id), () => adapter, renameOwners, pendingRenames, { value: 'live' }, ownsRename, sessions, captureRenameOwner,
    async (_id, operation, publish) => { const result = await operation(); await publish(result); return result }, async () => {}, cancel)
  return { sessions, begin, rename, renameOwners }
}

test('Rename_MissingHistoryNeverAdmitsEditor_004', () => {
  const h = catalogRenameHarness(() => false)
  h.begin('history')
  assert.equal(h.sessions.value[0].renameState, 'idle')
  assert.equal(h.renameOwners.size, 0)
})

test('Rename_OwnedFailedSaveRestoresEditor_005', async () => {
  const h = catalogRenameHarness()
  h.begin('history')
  await assert.rejects(h.rename('history', 'Recoverable draft', true), /REVISION_CONFLICT/)
  assert.equal(h.sessions.value[0].renameState, 'editing')
  assert.equal(h.renameOwners.size, 1)
})

test('Rename_HistoryDraftSurvivesQueuedSaveAndFailure_006', () => {
  const source = readFileSync('src/components/sessions/SessionItem.vue', 'utf8')
  const body = source.match(/function commitRename\(\) \{([\s\S]*?)\n\}/)?.[1]
  const watchBody = source.match(/watch\(\(\) => props.session.renameState, \(state, previous\) => \{([\s\S]*?)\n\}, \{ immediate: true \}\)/)?.[1]
  assert.ok(body && watchBody)
  const localRename = { value: true }, renameValue = { value: 'Recoverable draft' }, renameInvalid = { value: false }
  const emitted = []
  Function('isRenaming', 'isSaving', 'renameValue', 'renameInvalid', 'localRename', 'emit', 'props', 'nextTick', 'row', body)(
    { value: true }, { value: false }, renameValue, renameInvalid, localRename, (...args) => emitted.push(args), { session: { id: 'history' } }, () => {}, {})
  const watchState = Function('state', 'previous', 'localRename', 'renameValue', 'props', 'isSaving', 'focusRename', 'renameInvalid', watchBody)
  for (const [state, previous] of [['saving', 'editing'], ['editing', 'saving']]) {
    watchState(state, previous, localRename, renameValue, { session: { title: 'Old title' } }, { value: state === 'saving' }, () => {}, renameInvalid)
    assert.equal(renameValue.value, 'Recoverable draft')
    assert.equal(localRename.value, true)
  }
  assert.deepEqual(emitted, [['rename-commit', 'history', 'Recoverable draft']])
})
