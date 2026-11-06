// 官方 remove 函数体在注入服务和真实合成 SQLite 上的算法实验，不是完整 CLI/HTTP 验收。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { DatabaseSync } from 'node:sqlite'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'
import { upstreamRemoveSource } from './v027-stage9-opencode-remove-source.mjs'

const audit = path.dirname(fileURLToPath(import.meta.url))
const repository = path.dirname(audit)
assert.equal(fs.realpathSync(audit), path.join(fs.realpathSync(repository), '.audit'))
const fixture = fs.mkdtempSync(path.join(audit, 'v027-stage9-opencode-'))
assert.equal(path.dirname(fs.realpathSync(fixture)), fs.realpathSync(audit))
const expectProtection = process.argv.includes('--expect-protection')
const ROOT = 'ses_synthetic_root', CHILD = 'ses_synthetic_child'
console.log(JSON.stringify({ sourceCommit: '9bbb8ce18c80f099e0e448775f59d965ead40a21', sourceFile: 'packages/core/src/session.ts', sourceLine: 391, generatorSha256: createHash('sha256').update(upstreamRemoveSource).digest('hex'), mode: expectProtection ? 'falsifiable-protection-hypothesis' : 'observations', nativeCliOrServiceStarted: false, fixture: path.basename(fixture) }))

function database(name) {
  const filename = path.join(fixture, name + '.sqlite')
  const db = new DatabaseSync(filename)
  db.exec('PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE session_v2(id TEXT PRIMARY KEY, parent_id TEXT, time_archived INTEGER, time_suspended INTEGER, directory TEXT NOT NULL);')
  const add = (id, parent = null, archived = null) => db.prepare('INSERT INTO session_v2 VALUES(?,?,?,NULL,?)').run(id, parent, archived, '/synthetic/project')
  add(ROOT)
  return { db, filename, add, has: id => !!db.prepare('SELECT id FROM session_v2 WHERE id=?').get(id) }
}
const task = action => ({ *[Symbol.iterator]() { return yield action } })
async function run(iterator) {
  let step = iterator.next()
  while (!step.done) {
    try { step = iterator.next(await step.value()) }
    catch (error) { step = iterator.throw(error) }
  }
  return step.value
}
function nativeRemoval(database, hooks = {}) {
  const { db } = database, localActive = new Set(), foreignActive = new Set(), deleted = [], calls = []
  const Effect = { forEach: (items, fn) => task(async () => { for (const item of items) await run(fn(item)) }) }
  const result = { get: id => task(() => {
    const row = db.prepare('SELECT * FROM session_v2 WHERE id=?').get(id)
    assert(row, 'Synthetic session missing')
    hooks.afterGet?.(id)
    return row
  }) }
  const execution = {
    interrupt: id => task(() => { calls.push(['interrupt-current-process', id]); return localActive.delete(id) }),
    awaitIdle: id => task(() => { assert(!localActive.has(id)); calls.push(['await-idle-current-process', id]) }),
  }
  const jobs = { cancelSession: id => task(() => { calls.push(['cancel-session-jobs', id]) }) }
  const transport = { close: id => task(() => { calls.push(['close-current-transport', id]) }) }
  const store = { list: ({ parentID }) => task(() => {
    const rows = db.prepare('SELECT id FROM session_v2 WHERE parent_id=?').all(parentID)
    hooks.afterChildren?.(parentID)
    return rows
  }) }
  const environments = { clear: id => task(() => { calls.push(['clear-environment', id]) }) }
  const SessionEvent = { Deleted: 'synthetic-session-deleted' }
  const bus = {
    publish: (event, { sessionID }) => task(() => {
      assert.equal(event, SessionEvent.Deleted)
      // 只模拟官方 Deleted projector 的无 busy 谓词 SQL；blob/事件总线未作为实测声称。
      db.prepare('DELETE FROM session_v2 WHERE id=?').run(sessionID)
      deleted.push(sessionID)
    }),
    remove: id => task(() => { calls.push(['remove-stream', id]) }),
  }
  result.remove = Function('Effect', 'result', 'execution', 'jobs', 'transport', 'store', 'environments', 'bus', 'SessionEvent', `return (${upstreamRemoveSource})`)(Effect, result, execution, jobs, transport, store, environments, bus, SessionEvent)
  return { remove: id => run(result.remove(id)), localActive, foreignActive, deleted, calls }
}

test('OpenCodeRemoval_ForeignClaimAfterFinalReadIsNotAnIdleLease_001', async () => {
  const data = database('foreign-claim'), foreign = new DatabaseSync(data.filename)
  let claimed = false
  const native = nativeRemoval(data, { afterGet(id) {
    if (id !== ROOT) return
    foreign.prepare('UPDATE session_v2 SET time_suspended=? WHERE id=? AND time_suspended IS NULL').run(123, id)
    claimed = foreign.prepare('SELECT time_suspended FROM session_v2 WHERE id=?').get(id).time_suspended === 123
    native.foreignActive.add(id)
  } })
  try {
    assert.equal(data.db.prepare('SELECT time_suspended FROM session_v2 WHERE id=?').get(ROOT).time_suspended, null)
    await native.remove(ROOT)
    const observation = { foreignClaimCommitted: claimed, foreignExecutionModelRetained: native.foreignActive.has(ROOT), rootPresent: data.has(ROOT) }
    console.log(JSON.stringify({ case: 'foreign-claim', ...observation }))
    assert(claimed)
    assert(native.foreignActive.has(ROOT))
    assert.equal(data.has(ROOT), expectProtection, 'Native remove does not reject a committed foreign execution claim')
  } finally { foreign.close(); data.db.close() }
})

test('OpenCodeRemoval_ChildCreatedAfterEnumerationCanRemainOrphaned_002', async () => {
  const data = database('late-child'), foreign = new DatabaseSync(data.filename)
  const native = nativeRemoval(data, { afterChildren(id) {
    if (id === ROOT) foreign.prepare('INSERT INTO session_v2 VALUES(?,?,NULL,NULL,?)').run(CHILD, ROOT, '/synthetic/project')
  } })
  try {
    assert.equal(data.db.prepare("PRAGMA foreign_key_list('session_v2')").all().length, 0)
    await native.remove(ROOT)
    console.log(JSON.stringify({ case: 'late-child', rootPresent: data.has(ROOT), childPresent: data.has(CHILD) }))
    assert(!data.has(ROOT))
    assert.equal(data.has(CHILD), !expectProtection, 'Native parent disappearance is not complete descendant cleanup')
  } finally { foreign.close(); data.db.close() }
})

test('OpenCodeRemoval_ArchivedDescendantsAreIncluded_003', async () => {
  const data = database('archived-child')
  data.add(CHILD, ROOT, 123); data.add('ses_synthetic_other')
  const native = nativeRemoval(data)
  try {
    await native.remove(ROOT)
    assert.deepEqual(native.deleted, [CHILD, ROOT])
    assert(data.has('ses_synthetic_other'))
    console.log(JSON.stringify({ case: 'archived-child', removedArchivedChild: true, unrelatedRootRetained: true }))
  } finally { data.db.close() }
})

test('OpenCodeRemoval_MatchingMetadataDoesNotSelectTheHeldDatabase_004', async () => {
  const chosen = database('chosen'), served = database('served')
  const native = nativeRemoval(served)
  try {
    assert.deepEqual({ ...chosen.db.prepare('SELECT * FROM session_v2 WHERE id=?').get(ROOT) }, { ...served.db.prepare('SELECT * FROM session_v2 WHERE id=?').get(ROOT) })
    assert.notEqual(fs.statSync(chosen.filename).ino, fs.statSync(served.filename).ino)
    await native.remove(ROOT)
    console.log(JSON.stringify({ case: 'wrong-source', chosenRootPresent: chosen.has(ROOT), servedRootPresent: served.has(ROOT), identicalMetadata: true }))
    assert(!served.has(ROOT))
    assert.equal(chosen.has(ROOT), !expectProtection, 'Matching session identity does not bind the service database')
  } finally { chosen.db.close(); served.db.close() }
})

test('OpenCodeRemoval_LocalExecutionStartingAfterReadIsInterrupted_005', async () => {
  const data = database('local-late-start')
  let started = false
  const native = nativeRemoval(data, { afterGet(id) {
    native.localActive.add(id); started = true
  } })
  try {
    assert.equal(native.localActive.size, 0)
    await native.remove(ROOT)
    console.log(JSON.stringify({ case: 'local-late-start', executionModelStarted: started, executionModelInterrupted: !native.localActive.has(ROOT), rootPresent: data.has(ROOT) }))
    assert(started)
    assert.equal(native.localActive.has(ROOT), expectProtection, 'Native remove interrupts late local execution instead of refusing it')
    assert.equal(data.has(ROOT), expectProtection)
  } finally { data.db.close() }
})
