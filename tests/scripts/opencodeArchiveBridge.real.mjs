// 显式验收隔离构建的 helper。每次都调用官方 SDK 创建新假库，不接受用户数据库。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { DatabaseSync, backup } from 'node:sqlite'
const bridge = fs.realpathSync(process.argv[2]), modules = fs.realpathSync(process.argv[3])
const fixture = spawnSync(process.execPath, ['tests/scripts/opencodeRecoverySdk.real.mjs', modules], { encoding: 'utf8', windowsHide: true, timeout: 40000 })
assert.equal(fixture.status, 0, fixture.stdout + fixture.stderr)
const root = JSON.parse(fixture.stdout).root, filename = path.join(root, 'fixture.db'), db = new DatabaseSync(filename)
const target = db.prepare("SELECT id,directory FROM session_v2 WHERE title='late target'").get(), other = db.prepare("SELECT id FROM session_v2 WHERE title='unrelated'").get().id
const env = {}
for (const key of ['SystemRoot', 'WINDIR']) if (process.env[key]) env[key] = process.env[key]
for (const key of ['HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'TEMP', 'TMP']) env[key] = path.join(root, key)
const request = { databasePath: filename, nativeSessionId: target.id, expectedCwd: target.directory, archived: true }
function invoke(value) {
  const result = spawnSync(process.execPath, ['--permission', '--allow-fs-read=' + bridge, ...[value.databasePath, value.databasePath + '-wal', value.databasePath + '-shm'].flatMap(file => ['--allow-fs-read=' + file, '--allow-fs-write=' + file]), bridge], { env, cwd: root, input: JSON.stringify(value), encoding: 'utf8', windowsHide: true, timeout: 12000, maxBuffer: 1048576 })
  assert(!result.error, result.error?.message); assert(result.stdout.trim(), result.stderr)
  const lines = result.stdout.trim().split('\n').map(line => JSON.parse(line)); assert.deepEqual(lines.shift(), { state: 'ready' }); assert.equal(lines.length, 1); return lines[0]
}
const checks = []
try {
  const snapshots = new Map(['migration', 'job', 'schedule', 'blob', 'blob_ref', 'session_inbox', 'session_message', 'session_part', 'credential'].map(table => [table, db.prepare(`SELECT * FROM ${table}`).all()]))
  assert.deepEqual(invoke(request), { state: 'archived', nativeSessionId: target.id, archived: true })
  assert(db.prepare('SELECT time_archived FROM session_v2 WHERE id=?').get(target.id).time_archived)
  checks.push('bundled native archive works under Node filesystem permission, no child/addon permission')
  const eventCount = db.prepare('SELECT count(*) n FROM event WHERE aggregate_id=?').get(target.id).n
  assert.equal(invoke(request).archived, true); assert.equal(db.prepare('SELECT count(*) n FROM event WHERE aggregate_id=?').get(target.id).n, eventCount)
  checks.push('repeat desired state writes no extra native event')
  db.prepare('UPDATE session_v2 SET title=? WHERE id=?').run('concurrent unrelated marker', other)
  const unrelated = db.prepare('SELECT * FROM session_v2 WHERE id=?').get(other)
  assert.equal(invoke({ ...request, archived: false }).archived, false)
  assert.deepEqual(db.prepare('SELECT * FROM session_v2 WHERE id=?').get(other), unrelated)
  for (const [table, rows] of snapshots) assert.deepEqual(db.prepare(`SELECT * FROM ${table}`).all(), rows, table)
  checks.push('native restore retains other connection write, suspended claim and due sentinel; no migration')
  assert.equal(invoke({ ...request, expectedCwd: root }).code, 'ARCHIVE_SOURCE_UNAVAILABLE')
  assert.equal(db.prepare('SELECT time_archived FROM session_v2 WHERE id=?').get(target.id).time_archived, null)
  checks.push('wrong project is rejected before native mutation')
  db.exec('BEGIN IMMEDIATE')
  try { assert.equal(invoke(request).code, 'ARCHIVE_SOURCE_UNAVAILABLE') } finally { db.exec('ROLLBACK') }
  assert.equal(db.prepare('SELECT time_archived FROM session_v2 WHERE id=?').get(target.id).time_archived, null)
  assert.equal(invoke(request).archived, true)
  checks.push('competing writer is rejected without replay; explicit retry after release succeeds')
  const altered = path.join(root, 'unknown-schema.db'); await backup(db, altered); const changed = new DatabaseSync(altered); changed.exec('CREATE TABLE unverified_future_schema(x)'); changed.close()
  assert.equal(invoke({ ...request, databasePath: altered, archived: false }).code, 'ARCHIVE_SCHEMA_UNSUPPORTED')
  const verify = new DatabaseSync(altered, { readOnly: true }); assert(verify.prepare('SELECT time_archived FROM session_v2 WHERE id=?').get(target.id).time_archived); verify.close()
  checks.push('unknown schema rejected, not migrated or modified')
  const trapped = path.join(root, 'trigger.db'); await backup(db, trapped); const trap = new DatabaseSync(trapped)
  trap.exec(`CREATE TRIGGER sqliteXarchive AFTER UPDATE OF time_archived ON session_v2 BEGIN UPDATE session_v2 SET title='trigger changed unrelated' WHERE id='${other}'; END`); trap.close()
  assert.equal(invoke({ ...request, databasePath: trapped, archived: false }).code, 'ARCHIVE_SCHEMA_UNSUPPORTED')
  const trapRead = new DatabaseSync(trapped, { readOnly: true }); assert.equal(trapRead.prepare('SELECT title FROM session_v2 WHERE id=?').get(other).title, 'concurrent unrelated marker'); assert(trapRead.prepare('SELECT time_archived FROM session_v2 WHERE id=?').get(target.id).time_archived); trapRead.close()
  checks.push('sqliteX-prefixed trigger rejected; target and unrelated row remain unchanged')
  const oversized = path.join(root, 'oversized.db'); await backup(db, oversized); const large = new DatabaseSync(oversized); large.prepare('UPDATE session_v2 SET title=? WHERE id=?').run('x'.repeat(1048577), target.id); large.close()
  assert.equal(invoke({ ...request, databasePath: oversized, archived: false }).code, 'ARCHIVE_SOURCE_UNAVAILABLE')
  checks.push('oversized target fields rejected before native decoding')
  const nulFile = path.join(root, 'nul-numeric.db'); await backup(db, nulFile); const nulDb = new DatabaseSync(nulFile); nulDb.prepare('UPDATE session_v2 SET tokens_input=? WHERE id=?').run('\0' + 'x'.repeat(1048577), target.id); nulDb.close()
  const nulResult = invoke({ ...request, databasePath: nulFile, archived: false })
  const sequenceFile = path.join(root, 'sequence.db'); await backup(db, sequenceFile); const sequenceDb = new DatabaseSync(sequenceFile); sequenceDb.prepare('UPDATE event_sequence SET owner_id=? WHERE aggregate_id=?').run('\0' + 'x'.repeat(1048577), target.id); sequenceDb.close()
  assert.equal(invoke({ ...request, databasePath: sequenceFile, archived: false }).code, 'ARCHIVE_SOURCE_UNAVAILABLE')
  const viewFile = path.join(root, 'migration-view.db'); await backup(db, viewFile); const viewDb = new DatabaseSync(viewFile); viewDb.exec("ALTER TABLE migration RENAME TO preserved_migration; CREATE VIEW migration AS SELECT json_extract('not-json', '$') id"); viewDb.close()
  const viewResult = invoke({ ...request, databasePath: viewFile, archived: false })
  assert.deepEqual({ nul: nulResult.code, view: viewResult.code }, { nul: 'ARCHIVE_SOURCE_UNAVAILABLE', view: 'ARCHIVE_SCHEMA_UNSUPPORTED' })
  checks.push('all session/sequence columns use actual byte budget including NUL; migration view is not evaluated')
  const missing = path.join(root, 'must-not-create.db'); assert.equal(invoke({ ...request, databasePath: missing }).code, 'ARCHIVE_SOURCE_UNAVAILABLE'); assert(!fs.existsSync(missing))
  checks.push('absent source is never created')
  console.log(JSON.stringify({ root, checks, bridgeBytes: fs.statSync(bridge).size }))
} finally { db.close() }