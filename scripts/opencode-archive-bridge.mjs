// 专用原生归档 helper：调用官方 Session，不启动 host、模型或任务，不迁移数据库。
// 产品接线前调用者必须固定来源 db/WAL/SHM 的物理身份；此脚本自身不声称替代 OS 能力边界。
import { Effect, Layer } from 'effect'
import { SqliteClient } from '@effect/sql-sqlite-node'
import { SqlClient } from 'effect/sql'
import { Database } from '@opencode/core/database/database'
import { EffectDrizzleSqlite } from '@opencode/core/database/drizzle'
import { LayerNode } from '@opencode/util/effect/layer-node'
import { Session } from '@opencode/core/session/session'
import { SessionStore } from '@opencode/core/session/store'
import { SessionProjector } from '@opencode/core/session/projector'
import { SessionInbox } from '@opencode/core/session/inbox'
import { Bus } from '@opencode/core/bus'
import { Instance } from '@opencode/core/instance/service'
import { SessionExecution } from '@opencode/core/session/execution'
import { FSUtil } from '@opencode/util/fs-util'
import { createHash } from 'node:crypto'
import { isAbsolute, normalize } from 'node:path'
import { lstatSync } from 'node:fs'
const schema = '260fc8de269b8b1640ff3f558614fb75e5fdb6bc078a80538c8e668b2b2ec393'
const migration = 'cbf46641978f0b06ac422a845f3826a2cd033827603146ad9c154f63d37f9d9b'
const fail = code => { throw new Error(code) }
const spelling = value => process.platform === 'win32' ? normalize(value).toLowerCase() : normalize(value)
const denied = new Proxy({}, { get() { return fail('HOST_SERVICE_FORBIDDEN') } })
process.stdout.write(JSON.stringify({ state: 'ready' }) + '\n')
let issued = false
try {
  let input = ''
  for await (const chunk of process.stdin) { input += chunk; if (Buffer.byteLength(input) > 32768) fail('INVALID_REQUEST') }
  const request = JSON.parse(input)
  if (!request || Object.keys(request).sort().join() !== 'archived,databasePath,expectedCwd,nativeSessionId'
    || typeof request.archived !== 'boolean'
    || typeof request.nativeSessionId !== 'string' || !/^ses_[A-Za-z0-9_-]{1,124}$/.test(request.nativeSessionId)
    || !['databasePath', 'expectedCwd'].every(key => typeof request[key] === 'string' && isAbsolute(request[key]) && request[key].length <= 4096 && !/\p{Cc}/u.test(request[key]))) fail('INVALID_REQUEST')
  // 不创建来源。父进程持有能力才可消除本检查与 SQLite 打开间的路径竞争。
  const stat = lstatSync(request.databasePath)
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 1073741824) fail('ARCHIVE_SOURCE_UNAVAILABLE')
  const operation = Effect.gen(function* () {
    const sql = yield* SqlClient.SqlClient
    const db = yield* EffectDrizzleSqlite.makeWithDefaults()
    const dependencies = LayerNode.compile(LayerNode.group([SessionStore.node, Bus.node, SessionProjector.node]), { replacements: [Database.node.replace(Layer.succeed(Database.Service, { db })), Bus.node.replace(Bus.configured({ persist: true }))] })
    const context = yield* Layer.build(dependencies)
    const mutate = Effect.gen(function* () {
      // 结构/迁移清单及身份检查与原生写入共用一个 IMMEDIATE 事务，拒绝未知触发器/版本。
      const rows = yield* sql.unsafe("SELECT substr(type,1,32) type,substr(name,1,256) name,substr(tbl_name,1,256) tbl_name,CASE WHEN length(CAST(sql AS BLOB))<=65536 THEN sql ELSE null END sql FROM sqlite_schema WHERE lower(substr(name,1,7)) != 'sqlite_' ORDER BY type,name LIMIT 257")
      if (createHash('sha256').update(JSON.stringify(rows)).digest('hex') !== schema) fail('ARCHIVE_SCHEMA_UNSUPPORTED')
      // 先确认 migration 是官方表，再执行表查询，不能在结构核验前运行替换视图。
      const migrations = (yield* sql.unsafe('SELECT CASE WHEN length(CAST(id AS BLOB))<=256 THEN id ELSE null END id FROM migration ORDER BY id LIMIT 513')).map(row => row.id)
      if (createHash('sha256').update(JSON.stringify(migrations)).digest('hex') !== migration) fail('ARCHIVE_SCHEMA_UNSUPPORTED')
      const boundedRow = Effect.fn(function* (table, key, required) {
        // 非 STRICT 数值列也能存 TEXT/BLOB；限制所有已验证列的真实字节，不能用 length(TEXT)。
        const fields = (yield* sql.unsafe('PRAGMA table_info(' + table + ')')).map(row => row.name)
        const sizes = fields.map(field => 'coalesce(length(CAST("' + field.replaceAll('"', '""') + '" AS BLOB)),0)').join('+')
        const size = yield* sql.unsafe('SELECT ' + sizes + ' bytes FROM ' + table + ' WHERE ' + key + '=? LIMIT 2', [request.nativeSessionId])
        if (size.length > 1 || (required && size.length !== 1) || size.some(row => row.bytes > 1048576)) fail('ARCHIVE_SOURCE_UNAVAILABLE')
      })
      yield* boundedRow('session_v2', 'id', true)
      yield* boundedRow('event_sequence', 'aggregate_id', false)
      const sessions = yield* Session.make()
      const target = sessions.forSession(request.nativeSessionId)
      const before = yield* target.get()
      if (spelling(before.location.directory) !== spelling(request.expectedCwd)) fail('ARCHIVE_SOURCE_UNAVAILABLE')
      issued = true
      yield* request.archived ? target.archive() : target.unarchive()
      const after = yield* target.get()
      if (!!after.time.archived !== request.archived || spelling(after.location.directory) !== spelling(request.expectedCwd)) fail('ARCHIVE_OUTCOME_UNKNOWN')
      return { state: 'archived', nativeSessionId: request.nativeSessionId, archived: request.archived }
    }).pipe(Effect.provide(context), Effect.provideService(Database.Service, { db }), Effect.provideService(Instance.Service, denied), Effect.provideService(SessionInbox.Service, denied), Effect.provideService(SessionExecution.Service, denied), Effect.provideService(FSUtil.Service, denied))
    return yield* sql.withTransaction(mutate)
  })
  // 不启用 WAL、不使用 Database.layer，因此不执行自动迁移、chmod 或该层的显式 checkpoint；SQLite 仍可自动 checkpoint。
  const result = await Effect.runPromise(Effect.scoped(operation.pipe(Effect.provide(SqliteClient.layer({ filename: request.databasePath, disableWAL: true, busyTimeout: 0 })))))
  process.stdout.write(JSON.stringify(result) + '\n')
} catch (error) {
  const known = ['INVALID_REQUEST', 'ARCHIVE_SCHEMA_UNSUPPORTED', 'ARCHIVE_SOURCE_UNAVAILABLE']
  const code = issued ? 'ARCHIVE_OUTCOME_UNKNOWN' : error instanceof Error && known.includes(error.message) ? error.message : 'ARCHIVE_SOURCE_UNAVAILABLE'
  process.stdout.write(JSON.stringify({ state: 'failed', code }) + '\n')
  process.exitCode = 1
}