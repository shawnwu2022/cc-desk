// 显式、隔离的官方 OpenCode v2 合同实验，不是产品或真实用户源验收。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import net from 'node:net'
import { spawn, execFileSync } from 'node:child_process'
import { randomUUID, createHash } from 'node:crypto'
import { once } from 'node:events'
const program = process.argv[2]
assert(program && path.isAbsolute(program), 'Explicit installed executable required')
const root = fs.mkdtempSync(path.resolve('.audit/opencode-recovery-'))
const env = {}
for (const key of ['PATH', 'SystemRoot', 'WINDIR', 'ComSpec', 'PATHEXT']) if (process.env[key]) env[key] = process.env[key]
for (const key of ['HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'TEMP', 'TMP']) { env[key] = path.join(root, key); fs.mkdirSync(env[key], { recursive: true }) }
Object.assign(env, { OPENCODE_DISABLE_AUTOUPDATE: 'true', OPENCODE_DISABLE_MODELS_FETCH: 'true', OPENCODE_DISABLE_DEFAULT_PLUGINS: 'true', OPENCODE_DISABLE_EXTERNAL_SKILLS: 'true', OPENCODE_DISABLE_PROJECT_CONFIG: 'true', OPENCODE_PURE: 'true', OPENCODE_DB: 'contract.db', OPENCODE_PASSWORD: randomUUID() })
const project = path.join(root, 'project'); fs.mkdirSync(project)
const listener = net.createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening'); const port = listener.address().port; await new Promise(resolve => listener.close(resolve))
const report = { root, version: execFileSync(program, ['--version'], { env, cwd: project, windowsHide: true, encoding: 'utf8', timeout: 15000 }).trim(), sha256: createHash('sha256').update(fs.readFileSync(program)).digest('hex'), checks: [] }
const child = spawn(program, ['serve', '--hostname', '127.0.0.1', '--port', String(port)], { env, cwd: project, windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] })
let spawnFailure; child.stderr.resume()
const stopped = new Promise(resolve => { child.once('exit', resolve); child.once('error', error => { spawnFailure = error; resolve() }) })
const base = `http://127.0.0.1:${port}`
async function request(method, route, body, expected = 200) {
  const response = await fetch(base + route, { method, redirect: 'error', headers: { authorization: 'Basic ' + Buffer.from(`opencode:${env.OPENCODE_PASSWORD}`).toString('base64'), 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(5000) })
  assert.equal(response.status, expected, `${method} ${route}: ${response.status}`)
  if (expected === 204) return
  const text = await response.text(); assert(text.length < 1048576); return JSON.parse(text)
}
const session = id => `/api/session/${id}`
const get = async id => (await request('GET', session(id))).data
try {
  let ready = false
  for (let n = 0; n < 100; n++) { try { const r = await fetch(base + '/health', { signal: AbortSignal.timeout(300) }); if (r.ok) { ready = true; break } } catch {} if (spawnFailure || child.exitCode !== null || child.signalCode !== null) break; await new Promise(resolve => setTimeout(resolve, 100)) }
  assert(ready, 'Isolated server did not become healthy')
  const create = async input => (await request('POST', '/api/session', input)).data
  const main = await create({ location: { directory: project }, title: 'synthetic recovery root' })
  const other = await create({ location: { directory: project }, title: 'unrelated marker' })
  const descendant = await create({ parentID: main.id, title: 'synthetic child' })
  const otherBefore = await get(other.id), childBefore = await get(descendant.id)
  await request('PATCH', session(main.id), { archived: true }, 204)
  const archived = await get(main.id); assert(archived.time.archived)
  assert.deepEqual(await get(other.id), otherBefore); assert.deepEqual(await get(descendant.id), childBefore)
  report.checks.push('archive retains root and leaves child/unrelated records exactly unchanged')
  const listed = async archived => (await request('GET', '/api/session?directory=' + encodeURIComponent(project) + '&archived=' + archived)).data.map(row => row.id)
  assert(!(await listed(false)).includes(main.id)); assert((await listed(true)).includes(main.id))
  report.checks.push('native nonarchived list omits target; archived list retains it')
  await request('PATCH', session(main.id), { archived: true }, 204)
  assert.deepEqual(await get(main.id), archived)
  report.checks.push('repeat archive is a native no-op')
  await request('PATCH', session(descendant.id), { title: 'late child update' }, 204)
  await request('PATCH', session(main.id), { title: 'late root update' }, 204)
  await request('PATCH', session(main.id), { archived: false }, 204)
  const restored = await get(main.id); assert(!restored.time.archived); assert.equal(restored.title, 'late root update')
  assert.equal((await get(descendant.id)).title, 'late child update'); assert.deepEqual(await get(other.id), otherBefore)
  assert((await listed(false)).includes(main.id)); assert(!(await listed(true)).includes(main.id))
  report.checks.push('unarchive preserves late root/child writes and unrelated record, returns native list visibility')
  await request('PATCH', session(main.id), { archived: false }, 204)
  assert.deepEqual(await get(main.id), restored)
  report.checks.push('repeat unarchive is a native no-op')
  await request('DELETE', session(main.id), undefined, 204)
  await request('GET', session(main.id), undefined, 404); await request('GET', session(descendant.id), undefined, 404)
  assert.deepEqual(await get(other.id), otherBefore)
  report.checks.push('native DELETE removes this synthetic root and child, unrelated record unchanged; no idle claim')
} catch (error) { report.error = error.message; process.exitCode = 1 }
finally {
  if (!spawnFailure && child.exitCode === null && child.signalCode === null) child.kill()
  await stopped
  report.serverReaped = child.exitCode !== null || child.signalCode !== null
  fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2))
  // CLI stderr 直接排空，避免保存本任务临时认证。即使失败也不输出它。
  console.log(JSON.stringify(report, null, 2))
}