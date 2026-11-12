// 显式离线元数据实验；必须由 Rust 的 owned Job 入口运行，不认证、不调用模型。
import fs from 'node:fs'
import path from 'node:path'
import { randomUUID, createHash } from 'node:crypto'
import { spawn } from 'node:child_process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const self = fileURLToPath(import.meta.url)
const limit = 1024 * 1024
const check = (ok, code) => { if (!ok) throw Error(code) }
const hash = p => createHash('sha256').update(fs.readFileSync(p)).digest('hex')
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))
const observations = []
function observe(stage, value) {
  check(observations.length < 64, 'OBSERVATION_LIMIT')
  observations.push({ stage, kind: Array.isArray(value) ? 'array' : value === null ? 'null' : typeof value,
    count: Array.isArray(value) ? value.length : null,
    fields: value && !Array.isArray(value) && typeof value === 'object' ? Object.keys(value).filter(key => /^[a-zA-Z_]+$/.test(key)).slice(0, 32) : [],
    arrays: value && !Array.isArray(value) && typeof value === 'object' ? Object.entries(value).filter(([key, item]) => /^[a-zA-Z_]+$/.test(key) && Array.isArray(item)).map(([key, item]) => [key, item.length]).slice(0, 16) : [] })
  return value
}
export function codexFixtureRecords(spec, id, now) {
  return [{ timestamp: now, type: 'session_meta', payload: { session_id: id, id, timestamp: now, cwd: spec.project, originator: 'cc_desk_acceptance', cli_version: spec.version, source: 'cli' } },
    { timestamp: now, type: 'event_msg', payload: { type: 'user_message', message: 'CC Desk synthetic fixture. No model call.', images: [], local_images: [], text_elements: [] } }]
}
export function isolatedEnvironment(root, host = {}) {
  const env = {}
  for (const key of ['SystemRoot', 'WINDIR']) if (host[key]) env[key] = host[key]
  if (host.SystemRoot) env.PATH = `${host.SystemRoot}\\System32`
  for (const [key, relative] of Object.entries({ HOME: 'home', USERPROFILE: 'home', APPDATA: 'roaming', LOCALAPPDATA: 'local', TEMP: 'temporary', TMP: 'temporary', CODEX_HOME: 'codex-home', CODEX_SQLITE_HOME: 'sqlite', CLAUDE_CONFIG_DIR: 'claude-home', XDG_CONFIG_HOME: 'xdg/config', XDG_DATA_HOME: 'xdg/data', XDG_CACHE_HOME: 'xdg/cache', XDG_STATE_HOME: 'xdg/state', OPENCODE_CONFIG_DIR: 'opencode-config' })) {
    env[key] = path.join(root, relative)
    fs.mkdirSync(env[key], { recursive: true })
  }
  Object.assign(env, { DISABLE_AUTOUPDATER: '1', DISABLE_UPDATES: '1', DISABLE_TELEMETRY: '1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1', OPENCODE_DISABLE_AUTOUPDATE: 'true', OPENCODE_DISABLE_MODELS_FETCH: 'true', OPENCODE_DISABLE_DEFAULT_PLUGINS: 'true', OPENCODE_DISABLE_EXTERNAL_SKILLS: 'true', OPENCODE_DISABLE_PROJECT_CONFIG: 'true', OPENCODE_EXPERIMENTAL_DISABLE_FILEWATCHER: 'true', OPENCODE_PURE: 'true', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: path.join(root, 'empty-gitconfig'), GIT_CEILING_DIRECTORIES: root, HTTP_PROXY: 'http://127.0.0.1:9', HTTPS_PROXY: 'http://127.0.0.1:9', ALL_PROXY: 'http://127.0.0.1:9', NO_PROXY: '127.0.0.1,localhost' })
  // Windows 会补齐缺失的身份变量；显式合成值防止补入真实账户信息。
  env.HOMEDRIVE = path.parse(env.HOME).root.replace(/[\\/]$/, '')
  env.HOMEPATH = env.HOME.slice(env.HOMEDRIVE.length)
  env.SYSTEMDRIVE = path.win32.parse(host.SystemRoot || 'C:\\Windows').root.replace(/[\\/]$/, '')
  env.USERDOMAIN = 'metadata-fixture'
  env.USERNAME = 'metadata-fixture'
  return env
}
export function requireMethod(method) {
  check(['initialize', 'initialized', 'thread/read', 'thread/list', 'thread/name/set'].includes(method), 'METHOD_FORBIDDEN')
}
export function rpcResult(frame, requestId) {
  if (frame.method) { check(!('id' in frame), 'SERVER_REQUEST_FORBIDDEN'); return undefined }
  check(frame.id === requestId && !frame.error && frame.result, 'RPC_RESPONSE_INVALID')
  return frame.result
}
export function requireSession(rows, id, title, cwd) {
  check(Array.isArray(rows), 'LIST_INVALID')
  const matches = rows.filter(row => row.id === id)
  check(matches.length === 1, 'SESSION_NOT_UNIQUE')
  check(matches[0].name === title, 'TITLE_MISMATCH')
  check(matches[0].cwd === cwd, 'PROJECT_MISMATCH')
}
export function metadataResult(cli, checks) {
  check(['claude', 'codex', 'opencode'].includes(cli), 'CLI_REQUIRED')
  check(['read', 'rename', 'reopen', 'unique-list'].every(c => checks.includes(c)), 'METADATA_INCOMPLETE')
  return { cli, status: 'METADATA_PASS', checks, nativeLifecycleAccepted: false, guiAccepted: false, modelCalled: false, installerRoundtripAccepted: false }
}
export async function readBoundedJson(response, maximum = limit) {
  if (!response.body) return null
  const reader = response.body.getReader(), chunks = []
  let size = 0
  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      size += value.length
      if (size > maximum) { await reader.cancel(); throw Error('HTTP_OUTPUT_LIMIT') }
      chunks.push(Buffer.from(value))
    }
  } finally { reader.releaseLock() }
  return size ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : null
}
function processChild(program, args, cwd, env) {
  const child = spawn(program, args, { cwd, env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] })
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8')
  let stdout = '', stderr = '', error = null, closed = false, result
  child.on('error', () => { error = 'CHILD_SPAWN_FAILED' })
  child.stdout.on('data', bytes => { if (Buffer.byteLength(stdout) + Buffer.byteLength(bytes) > limit) error = 'CHILD_OUTPUT_LIMIT'; else stdout += bytes.toString('utf8') })
  child.stderr.on('data', bytes => { if (Buffer.byteLength(stderr) + Buffer.byteLength(bytes) > limit) error = 'CHILD_OUTPUT_LIMIT'; else stderr += bytes.toString('utf8') })
  child.stdin.on('error', () => { error ??= 'CHILD_INPUT_FAILED' })
  child.once('close', (code, signal) => { closed = true; result = { code, signal } })
  const state = () => { check(!error, error); return { stdout, stderr, closed, result } }
  async function wait(predicate, code, ms = 6000) {
    const until = Date.now() + ms
    while (Date.now() < until) { const s = state(); if (predicate(s)) return s; check(!s.closed, 'CHILD_EXITED_EARLY'); await sleep(10) }
    throw Error(code)
  }
  return { child, state, wait, async close(terminate = false) {
    if (terminate && !closed) child.kill()
    else child.stdin.end()
    const s = await wait(s => s.closed, 'CHILD_EXIT_PENDING')
    check(terminate || s.result.code === 0, /^[A-Z_]+\s*$/.test(s.stderr) ? s.stderr.trim() : 'CHILD_EXIT_FAILED')
    return s
  } }
}
async function version(spec, env) {
  const process = processChild(spec.binary, ['--version'], spec.project, env)
  const { stdout, stderr } = await process.close()
  const patterns = { claude: /^(\d+\.\d+\.\d+) \(Claude Code\)$/, codex: /^codex-cli (\d+\.\d+\.\d+)$/, opencode: /^(?:opencode v?)?(\d+\.\d+\.\d+)$/ }
  check(!stderr && patterns[spec.cli]?.test(stdout.trim()), 'VERSION_OUTPUT_INVALID')
  return patterns[spec.cli].exec(stdout.trim())[1]
}
async function claude(spec, env) {
  const id = randomUUID(), title = 'CC Desk synthetic metadata title'
  const directory = path.join(env.CLAUDE_CONFIG_DIR, 'projects', 'fixture')
  fs.mkdirSync(directory, { recursive: true })
  const original = { type: 'user', sessionId: id, cwd: spec.project, uuid: randomUUID(), timestamp: new Date().toISOString(), message: { role: 'user', content: 'CC Desk synthetic fixture. No model call.' } }
  fs.writeFileSync(path.join(directory, `${id}.jsonl`), `${JSON.stringify(original)}\n`, { flag: 'wx' })
  // SDK 必须在已清空认证并设置隔离根的新进程内导入；不在父 Node 的用户环境导入。
  const input = { sdk: spec.sdk, id, title, project: spec.project }
  const invoke = async mode => {
    const child = processChild(process.execPath, [self, '--claude-sdk', mode], spec.project, env)
    child.child.stdin.write(JSON.stringify(input))
    const result = await child.close()
    check(!result.stderr, 'SDK_STDERR')
    return JSON.parse(result.stdout)
  }
  await invoke('rename')
  requireSession(await invoke('read'), id, title, spec.project)
  return metadataResult('claude', ['read', 'rename', 'reopen', 'unique-list'])
}
async function sdkMain(mode) {
  check(process.env.CC_DESK_NATIVE_METADATA_LIVE_GO === 'isolated-metadata', 'OWNED_ENTRY_REQUIRED')
  check(['rename', 'read'].includes(mode), 'SDK_MODE_INVALID')
  let input = ''
  for await (const bytes of process.stdin) { input += bytes; check(input.length < 32768, 'SDK_INPUT_LIMIT') }
  const request = JSON.parse(input)
  const root = process.env.CC_DESK_NATIVE_METADATA_ROOT
  check(typeof root === 'string' && path.isAbsolute(root), 'HELPER_FIXTURE_REQUIRED')
  const spec = JSON.parse(fs.readFileSync(path.join(root, 'metadata-spec.json'), 'utf8'))
  check(spec.cli === 'claude' && spec.root === root && spec.project === path.join(root, 'project') && request.project === spec.project && request.sdk === spec.sdk, 'HELPER_SOURCE_MISMATCH')
  const expected = isolatedEnvironment(root, process.env)
  check(Object.entries(expected).every(([key, value]) => process.env[key] === value), 'HELPER_ENVIRONMENT_MISMATCH')
  check(Object.keys(process.env).every(key => key in expected || ['CC_DESK_NATIVE_METADATA_LIVE_GO', 'CC_DESK_NATIVE_METADATA_ROOT'].includes(key)), 'HELPER_ENVIRONMENT_MISMATCH')
  const sdk = await import(pathToFileURL(request.sdk).href)
  const before = await sdk.getSessionInfo(request.id)
  check(before?.sessionId === request.id && before.cwd === request.project, 'SDK_SOURCE_MISMATCH')
  if (mode === 'rename') await sdk.renameSession(request.id, request.title)
  const info = await sdk.getSessionInfo(request.id)
  check(info?.customTitle === request.title, 'SDK_READBACK_MISMATCH')
  const rows = await sdk.listSessions()
  const result = rows.map(r => ({ id: r.sessionId, name: r.customTitle, cwd: r.cwd }))
  requireSession(result, request.id, request.title, request.project)
  process.stdout.write(JSON.stringify(result))
}
async function codex(spec, env) {
  const id = randomUUID(), title = 'CC Desk synthetic metadata title', now = new Date().toISOString()
  const directory = path.join(env.CODEX_HOME, 'sessions', now.slice(0, 10).replaceAll('-', '/'))
  fs.mkdirSync(directory, { recursive: true })
  const rollout = codexFixtureRecords(spec, id, now).map(row => JSON.stringify(row)).join('\n')
  fs.writeFileSync(path.join(directory, `rollout-${now.slice(0, 19).replaceAll(':', '-')}-${id}.jsonl`), `${rollout}\n`, { flag: 'wx' })
  for (const generation of [1, 2]) {
    const child = processChild(spec.binary, ['app-server', '--strict-config'], spec.project, env)
    let sequence = 0, offset = 0
    async function request(method, params, notification = false) {
      requireMethod(method)
      const requestId = ++sequence
      child.child.stdin.write(JSON.stringify({ ...(notification ? {} : { id: requestId }), method, ...(params ? { params } : {}) }) + '\n')
      if (notification) return
      let found
      await child.wait(s => {
        while (s.stdout.indexOf('\n', offset) >= 0) {
          const end = s.stdout.indexOf('\n', offset)
          const frame = JSON.parse(s.stdout.slice(offset, end)); offset = end + 1
          const result = rpcResult(frame, requestId)
          if (result === undefined) continue
          found = observe(method, result); return true
        }
        return false
      }, 'RPC_TIMEOUT')
      return found
    }
    try {
      const initialized = await request('initialize', { clientInfo: { name: 'cc_desk', version: '0.0.0' }, capabilities: { explicitGatewayOauth: true, experimentalApi: false } })
      check(fs.realpathSync(initialized.codexHome) === fs.realpathSync(env.CODEX_HOME), 'CODEX_HOME_MISMATCH')
      await request('initialized', undefined, true)
      const before = await request('thread/read', { threadId: id, includeTurns: false })
      check(before.thread.id === id && before.thread.cwd === spec.project, 'CODEX_SOURCE_MISMATCH')
      if (generation === 1) await request('thread/name/set', { threadId: id, name: title })
      const read = await request('thread/read', { threadId: id, includeTurns: false })
      requireSession([read.thread], id, title, spec.project)
      const list = await request('thread/list', { limit: 100, archived: false })
      check(list.nextCursor == null, 'LIST_INCOMPLETE')
      requireSession(list.data, id, title, spec.project)
    } finally { await child.close() }
  }
  return metadataResult('codex', ['read', 'rename', 'reopen', 'unique-list'])
}
export function opencodeContract(version) {
  const major = Number(version.split('.')[0])
  check(major === 1 || major === 2, 'API_VERSION_UNSUPPORTED')
  const unwrap = value => major === 2 ? value.data : value
  const session = value => {
    const s = unwrap(value)
    return { id: s.id, name: s.title, cwd: s.location?.directory ?? s.directory }
  }
  return { patchReturnsEntity: major === 1, route: major === 2 ? '/api/session' : '/session', health: major === 2 ? '/health' : '/global/health',
    create: directory => major === 2 ? { location: { directory } } : {}, session,
    list: value => unwrap(value).map(row => session(major === 2 ? { data: row } : row)) }
}
async function opencode(spec, env) {
  const password = randomUUID()
  Object.assign(env, { OPENCODE_DB: path.join(spec.root, 'opencode-fixture.db'), OPENCODE_SERVER_USERNAME: 'opencode', OPENCODE_SERVER_PASSWORD: password, OPENCODE_PASSWORD: password })
  const contract = opencodeContract(spec.version), title = 'CC Desk synthetic metadata title'
  let id
  for (const generation of [1, 2]) {
    const child = processChild(spec.binary, ['serve', '--hostname', '127.0.0.1', '--port', '0'], spec.project, env)
    try {
      const s = await child.wait(s => /http:\/\/127\.0\.0\.1:(\d+)/.test(s.stdout), 'SERVER_START_TIMEOUT')
      const port = /http:\/\/127\.0\.0\.1:(\d+)/.exec(s.stdout)[1]
      check(Number(port) > 0 && Number(port) < 65536, 'SERVER_PORT_INVALID')
      const request = async (method, route, body, health = false) => {
        observe('http-dispatch', { method, port: 'dynamic' })
        const response = await fetch(`http://127.0.0.1:${port}${route}`, { method, headers: { authorization: 'Basic ' + Buffer.from(`opencode:${password}`).toString('base64'), 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(3000), redirect: 'error' })
        check(response.ok, 'HTTP_METADATA_FAILED')
        if (method === 'PATCH' && !contract.patchReturnsEntity) {
          check(response.status === 204, 'PATCH_STATUS_UNEXPECTED')
          await response.body?.cancel()
          return null
        }
        // v2 /health 为 HTML；仅确认状态并取消正文，业务接口仍严格解析 JSON。
        if (health && contract.health === '/health') { await response.body?.cancel(); return true }
        return observe(method + (route.startsWith(contract.route + '/') ? contract.route + '/:id' : route), await readBoundedJson(response))
      }
      const health = await request('GET', contract.health, undefined, true)
      check(health === true || health.healthy === true, 'SERVER_UNHEALTHY')
      if (generation === 1) {
        const created = contract.session(await request('POST', contract.route, contract.create(spec.project)))
        id = created.id; check(/^ses_[a-zA-Z0-9]+$/.test(id) && created.cwd === spec.project, 'SESSION_SOURCE_MISMATCH')
        const changed = await request('PATCH', `${contract.route}/${id}`, { title })
        if (contract.patchReturnsEntity) requireSession([contract.session(changed)], id, title, spec.project)
      }
      const session = contract.session(await request('GET', `${contract.route}/${id}`))
      requireSession([session], id, title, spec.project)
      const list = contract.list(await request('GET', contract.route))
      requireSession(list, id, title, spec.project)
    } finally { await child.close(true) }
  }
  return metadataResult('opencode', ['read', 'rename', 'reopen', 'unique-list', 'create'])
}
async function main(specFile) {
  check(process.env.CC_DESK_NATIVE_METADATA_LIVE_GO === 'isolated-metadata', 'OWNED_ENTRY_REQUIRED')
  const spec = JSON.parse(fs.readFileSync(specFile, 'utf8'))
  check(['claude', 'codex', 'opencode'].includes(spec.cli), 'CLI_REQUIRED')
  check(spec.project === path.join(spec.root, 'project') && path.resolve(specFile) === path.join(spec.root, 'metadata-spec.json'), 'FIXTURE_PATH_MISMATCH')
  check(hash(spec.binary) === spec.binarySha256, 'BINARY_CHANGED')
  const env = isolatedEnvironment(spec.root, process.env)
  env.CC_DESK_NATIVE_METADATA_LIVE_GO = 'isolated-metadata'
  env.CC_DESK_NATIVE_METADATA_ROOT = spec.root
  const reportPath = path.join(spec.root, 'reports', 'metadata-result.json')
  const fd = fs.openSync(reportPath, 'wx')
  try {
    spec.version = await version(spec, env)
    const result = await ({ claude, codex, opencode })[spec.cli](spec, env)
    check(hash(spec.binary) === spec.binarySha256, 'BINARY_CHANGED')
    fs.writeFileSync(fd, JSON.stringify({ ...result, version: spec.version, binarySha256: spec.binarySha256, processGroupAcceptance: 'pending-outer-owner' }, null, 2))
  } catch (error) {
    const reason = typeof error.message === 'string' && /^[A-Z_]+$/.test(error.message) ? error.message : 'METADATA_PROTOCOL_FAILED'
    fs.writeFileSync(fd, JSON.stringify({ cli: spec.cli, version: spec.version ?? null, status: 'FAILED', reason, errorType: ['TypeError', 'SyntaxError', 'Error'].includes(error.name) ? error.name : 'other', causeCode: /^[A-Z_]+$/.test(error.cause?.code ?? '') ? error.cause.code : null, observations, nativeLifecycleAccepted: false }))
    throw Error(reason)
  } finally { fs.closeSync(fd) }
}
if (process.argv[1] && path.resolve(process.argv[1]) === self) {
  const operation = process.argv[2] === '--claude-sdk' ? sdkMain(process.argv[3]) : main(process.argv[2])
  operation.catch(error => { process.stderr.write(`${/^[A-Z_]+$/.test(error.message) ? error.message : 'METADATA_DRIVER_FAILED'}\n`); process.exitCode = 1 })
}
