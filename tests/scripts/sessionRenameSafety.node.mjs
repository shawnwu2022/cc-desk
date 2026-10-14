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
