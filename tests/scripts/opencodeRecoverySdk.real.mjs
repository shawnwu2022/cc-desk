// 显式运行：node tests/scripts/opencodeRecoverySdk.real.mjs <isolated node_modules>
// 只证明固定 SDK 的受限宿主原型；不接产品，不接受已有用户 DB。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL, fileURLToPath } from 'node:url'
import { DatabaseSync } from 'node:sqlite'
import { createHash } from 'node:crypto'
const self = fileURLToPath(import.meta.url)
if (process.argv[2] !== '--worker') {
  const modules = fs.realpathSync(process.argv[2] ?? '')
  for (const name of ['sdk', 'core', 'server', 'schema', 'client', 'util', 'plugin']) assert.equal(JSON.parse(fs.readFileSync(path.join(modules, '@opencode', name, 'package.json'))).version, '2.2.6')
  const root = fs.mkdtempSync(path.resolve('.audit/opencode-sdk-recovery-')), env = {}
  for (const key of ['PATH', 'SystemRoot', 'WINDIR', 'ComSpec', 'PATHEXT']) if (process.env[key]) env[key] = process.env[key]
  for (const key of ['HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'TEMP', 'TMP']) { env[key] = path.join(root, key); fs.mkdirSync(env[key], { recursive: true }) }
  Object.assign(env, { OPENCODE_DISABLE_AUTOUPDATE: 'true', OPENCODE_DISABLE_MODELS_FETCH: 'true', OPENCODE_DISABLE_DEFAULT_PLUGINS: 'true', OPENCODE_DISABLE_EXTERNAL_SKILLS: 'true', OPENCODE_DISABLE_PROJECT_CONFIG: 'true', OPENCODE_PURE: 'true' })
  const result = spawnSync(process.execPath, [self, '--worker', modules, root], { env, cwd: root, windowsHide: true, timeout: 30000, encoding: 'utf8', maxBuffer: 1048576 })
  const report = { root, status: result.status, error: result.error?.message, stdout: result.stdout, stderr: result.stderr, sdkPackageSha256: createHash('sha256').update(fs.readFileSync(path.join(modules, '@opencode/sdk/package.json'))).digest('hex') }
  fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2)); console.log(JSON.stringify(report, null, 2)); process.exitCode = result.status ?? 1
} else {
  const modules = process.argv[3], root = fs.realpathSync(process.argv[4]); assert.equal(root, fs.realpathSync(process.cwd())); assert.equal(process.env.HOME, path.join(root, 'HOME'))
  const load = filename => import(pathToFileURL(path.join(modules, filename)).href)
  const { OpenCode } = await load('@opencode/sdk/dist/effect/index.js')
  const { SessionRestart } = await load('@opencode/core/dist/session/execution/restart.js')
  const { SessionExecution } = await load('@opencode/core/dist/session/execution.js')
  const { Schedule } = await load('@opencode/core/dist/schedule.js')
  const { Effect, Layer } = await load('effect/dist/index.js')
  const filename = path.join(root, 'fixture.db'), project = path.join(root, 'project'), config = path.join(root, 'empty-config'); fs.mkdirSync(project); fs.mkdirSync(config)
  const calls = { recovery: 0, schedule: 0, execution: 0, network: 0 }, checks = []
  const unexpected = kind => Effect.sync(() => { calls[kind]++; throw Error('Forbidden host operation: ' + kind) })
  const schedule = { run: unexpected('schedule'), tick: unexpected('schedule'), enabled: () => Effect.succeed(false) }
  for (const method of ['create', 'list', 'get', 'pause', 'resume', 'remove']) schedule[method] = () => unexpected('schedule')
  const execution = { active: Effect.succeed(new Set()), isActive: () => Effect.succeed(false) }
  for (const method of ['resume', 'wake', 'interrupt', 'awaitIdle']) execution[method] = () => unexpected('execution')
  const options = { database: { path: filename }, events: { persist: true }, config: { directory: config, project: false, claude: false, content: '{}' }, models: { fetch: false, snapshot: false }, fs: { filewatcher: false, fff: false }, fetch: () => { calls.network++; return Promise.reject(Error('Network forbidden')) } }
  const embed = { overrides: [SessionRestart.node.replace(Layer.succeed(SessionRestart.Service, { resumeSuspendedSessions: Effect.sync(() => { calls.recovery++ }) })), Schedule.node.replace(Layer.succeed(Schedule.Service, schedule)), SessionExecution.node.replace(Layer.succeed(SessionExecution.Service, execution))] }
  const run = operation => Effect.runPromise(Effect.scoped(Effect.gen(function* () { const sdk = yield* OpenCode.create(options, embed); return yield* operation(sdk) })))
  const ids = await run(sdk => Effect.gen(function* () {
    const main = yield* sdk.sessions.create({ location: { directory: project }, title: 'target' })
    const child = yield* sdk.sessions.create({ parentID: main.id, title: 'child' })
    const other = yield* sdk.sessions.create({ location: { directory: project }, title: 'unrelated' })
    return { main: main.id, child: child.id, other: other.id }
  }))
  // 只有本测试刚创建的真实 schema 被加入合成悬挂/到期哨兵，绝不把这些字段当生产空闲证明。
  const db = new DatabaseSync(filename)
  db.prepare('UPDATE session_v2 SET time_suspended=1,resume_attempts=2 WHERE id=?').run(ids.other)
  db.prepare('INSERT INTO schedule(id,session_id,action,"when",tz,next_due_at,time_created,time_updated) VALUES(?,?,?,?,?,?,?,?)').run('synthetic_due_sentinel', ids.other, '{"type":"synthetic-sentinel"}', '{"type":"synthetic-sentinel"}', 'UTC', 1, 1, 1)
  const tables = ['migration', 'job', 'schedule', 'blob', 'blob_ref', 'session_message', 'session_inbox', 'session_part', 'credential']
  const snapshots = new Map(tables.map(table => [table, db.prepare(`SELECT * FROM ${table}`).all()]))
  const unrelated = db.prepare('SELECT * FROM session_v2 WHERE id=?').get(ids.other)
  // 非法项目配置若被宿主读取就应失败；受限宿主必须不触碰它。
  fs.writeFileSync(path.join(project, 'opencode.json'), 'NOT VALID JSON')
  await run(sdk => Effect.gen(function* () {
    yield* sdk.sessions.update({ sessionID: ids.main, archived: true })
    const archived = yield* sdk.sessions.get({ sessionID: ids.main }); assert(archived.time.archived)
    assert(!(yield* sdk.sessions.get({ sessionID: ids.child })).time.archived)
    const before = db.prepare('SELECT * FROM session_v2 WHERE id=?').get(ids.main)
    yield* sdk.sessions.update({ sessionID: ids.main, archived: true })
    assert.deepEqual(db.prepare('SELECT * FROM session_v2 WHERE id=?').get(ids.main), before)
    checks.push('native archive keeps children and repeated state is a no-op')
    yield* sdk.sessions.update({ sessionID: ids.main, title: 'late target' })
    yield* sdk.sessions.update({ sessionID: ids.child, title: 'late child' })
    yield* sdk.sessions.update({ sessionID: ids.main, archived: false })
    const restored = yield* sdk.sessions.get({ sessionID: ids.main }); assert(!restored.time.archived); assert.equal(restored.title, 'late target')
    assert.equal((yield* sdk.sessions.get({ sessionID: ids.child })).title, 'late child')
    checks.push('restore preserves later target/child metadata')
  }))
  for (const [table, rows] of snapshots) assert.deepEqual(db.prepare(`SELECT * FROM ${table}`).all(), rows, table)
  assert.deepEqual(db.prepare('SELECT * FROM session_v2 WHERE id=?').get(ids.other), unrelated)
  assert.equal(db.prepare('SELECT count(*) n FROM event WHERE aggregate_id=?').get(ids.other).n, 1)
  checks.push('suspended unrelated session and due synthetic sentinel retain every field; no unrelated events')
  assert.deepEqual(calls, { recovery: 2, schedule: 0, execution: 0, network: 0 })
  checks.push('public overrides replace boot recovery, scheduling and execution; no outbound fetch')
  checks.push('malformed project config is ignored; no schema migration changes on second host')
  db.close(); console.log(JSON.stringify({ checks, calls }))
}