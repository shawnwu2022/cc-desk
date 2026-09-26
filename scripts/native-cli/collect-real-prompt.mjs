#!/usr/bin/env node

import {
  closeSync,
  existsSync,
  fsyncSync,
  openSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from 'node:fs'
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path'
import { pathToFileURL } from 'node:url'
import { verifyPromptTransform } from './real-cli-evidence.mjs'

const MAX_INPUT_BYTES = 8 * 1024 * 1024
const SUPPORTED_CLIS = new Set(['claude', 'codex'])
const SUPPORTED_LANES = new Set(['cc-desk', 'system-terminal'])
const SUPPORTED_OBSERVER_STATES = new Set(['off', 'on'])

function fail(message) {
  throw new Error(message)
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function nonEmpty(value) {
  return typeof value === 'string' && value.length > 0
}

function parseArgs(argv) {
  const parsed = { cli: null, fixture: null, report: null }
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index]
    const nextValue = () => {
      if (index + 1 >= argv.length) fail(`missing value for ${option}`)
      index += 1
      return argv[index]
    }

    switch (option) {
      case '--cli':
        if (parsed.cli !== null) fail('duplicate --cli')
        parsed.cli = nextValue()
        break
      case '--fixture':
        if (parsed.fixture !== null) fail('duplicate --fixture')
        parsed.fixture = nextValue()
        break
      case '--report':
        if (parsed.report !== null) fail('duplicate --report')
        parsed.report = nextValue()
        break
      default:
        fail(`unknown option: ${option}`)
    }
  }

  if (!SUPPORTED_CLIS.has(parsed.cli)) fail('unsupported --cli')
  if (parsed.fixture === null) fail('missing --fixture')
  if (parsed.report === null) fail('missing --report')
  if (!isAbsolute(parsed.fixture)) fail('fixture path must be absolute')
  if (!isAbsolute(parsed.report)) fail('report path must be absolute')
  return parsed
}

function inside(root, target) {
  const fromRoot = relative(root, target)
  return fromRoot === '' || (
    fromRoot !== '..'
    && !fromRoot.startsWith(`..${sep}`)
    && !isAbsolute(fromRoot)
  )
}

function resolveFixturePath(root, path) {
  const target = realpathSync(path)
  if (!inside(root, target)) fail('fixture path outside test root')
  return target
}

function resolveReportPath(root, path) {
  const parent = realpathSync(dirname(path))
  if (!inside(root, parent)) fail('report path outside test root')
  const target = resolve(path)
  if (existsSync(target)) fail('report already exists')
  return target
}

function canonicalBase64(value) {
  if (!nonEmpty(value)) return null
  try {
    const bytes = Buffer.from(value, 'base64')
    return bytes.toString('base64') === value ? bytes : null
  } catch {
    return null
  }
}

function validateFixture(root, fixture, cli) {
  if (!isObject(fixture) || fixture.schemaVersion !== 1) {
    fail('invalid real CLI fixture')
  }
  if (fixture.cli !== cli) fail('fixture CLI mismatch')
  if (!nonEmpty(fixture.runId)) fail('missing fixture runId')
  if (!SUPPORTED_LANES.has(fixture.lane)) fail('invalid fixture lane')
  if (!SUPPORTED_OBSERVER_STATES.has(fixture.observer)) {
    fail('invalid fixture observer state')
  }
  if (!nonEmpty(fixture.nonce)) fail('missing fixture nonce')
  if (!isAbsolute(fixture.expectedCwd ?? '')) fail('fixture cwd must be absolute')

  let expectedCwd
  try {
    expectedCwd = realpathSync(fixture.expectedCwd)
  } catch {
    fail('fixture cwd unavailable')
  }
  if (!inside(root, expectedCwd)) fail('fixture cwd outside test root')

  const payload = canonicalBase64(fixture.hostPayloadBase64)
  if (!payload) fail('invalid fixture host payload')
  const hostPayload = payload.toString('utf8')
  if (!hostPayload.includes(fixture.nonce)) fail('fixture nonce missing from host payload')

  const allowedTransform = cli === 'codex'
    ? fixture.transformId === 'codex-user-prompt-submit-v1-exact'
    : (
      fixture.transformId === 'claude-user-prompt-submit-v1-exact'
      || fixture.transformId === 'claude-user-prompt-submit-pasted-content-v1'
    )
  if (!allowedTransform) fail('unsupported fixture transform')

  return {
    ...fixture,
    expectedCwd,
    hostPayload,
  }
}

function sameExistingPath(left, right) {
  try {
    return realpathSync(left) === realpathSync(right)
  } catch {
    return false
  }
}

function invalid(reason) {
  return { valid: false, reason }
}

function validateRealEvent(event, fixture) {
  if (!isObject(event)) return invalid('INVALID_ENVELOPE')
  if (event.hook_event_name !== 'UserPromptSubmit') {
    return invalid('EVENT_MISMATCH')
  }
  if (!nonEmpty(event.session_id)) return invalid('SESSION_ID_REQUIRED')
  if (!nonEmpty(event.cwd) || !sameExistingPath(event.cwd, fixture.expectedCwd)) {
    return invalid('CWD_MISMATCH')
  }
  if (fixture.cli === 'codex' && !nonEmpty(event.turn_id)) {
    return invalid('CODEX_TURN_ID_REQUIRED')
  }
  if (typeof event.prompt !== 'string') return invalid('CONTENT_MISMATCH')

  return verifyPromptTransform(
    fixture.transformId,
    fixture.hostPayload,
    event.prompt,
  )
}

async function readStdinLimited() {
  const chunks = []
  let total = 0
  for await (const chunk of process.stdin) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)
    total += bytes.length
    if (total > MAX_INPUT_BYTES) fail('hook envelope exceeds 8 MiB')
    chunks.push(bytes)
  }
  return Buffer.concat(chunks, total).toString('utf8')
}

function parseJson(value, label) {
  try {
    return JSON.parse(value)
  } catch {
    fail(`invalid ${label} JSON`)
  }
}

function writeReportOnce(path, report) {
  let fd
  try {
    fd = openSync(path, 'wx')
    writeFileSync(fd, `${JSON.stringify(report, null, 2)}\n`, 'utf8')
    fsyncSync(fd)
  } catch (error) {
    if (error && typeof error === 'object' && error.code === 'EEXIST') {
      fail('report already exists')
    }
    throw error
  } finally {
    if (fd !== undefined) closeSync(fd)
  }
}

async function main() {
  if (process.env.CC_DESK_REAL_CLI_TEST_MODE !== '1') {
    fail('real CLI test mode required')
  }

  const rootValue = process.env.CC_DESK_TEST_ROOT
  if (!rootValue || !isAbsolute(rootValue)) {
    fail('missing absolute CC_DESK_TEST_ROOT')
  }
  const root = realpathSync(rootValue)
  const options = parseArgs(process.argv.slice(2))
  const fixturePath = resolveFixturePath(root, options.fixture)
  const reportPath = resolveReportPath(root, options.report)
  const fixture = validateFixture(
    root,
    parseJson(readFileSync(fixturePath, 'utf8'), 'fixture'),
    options.cli,
  )

  const rawEnvelope = parseJson(await readStdinLimited(), 'hook envelope')
  const validation = validateRealEvent(rawEnvelope, fixture)

  writeReportOnce(reportPath, {
    schemaVersion: 1,
    kind: 'real-user-prompt-submit',
    cli: options.cli,
    runId: fixture.runId,
    lane: fixture.lane,
    observer: fixture.observer,
    transformId: fixture.transformId,
    validation,
    sessionId: nonEmpty(rawEnvelope?.session_id) ? rawEnvelope.session_id : null,
    turnId: nonEmpty(rawEnvelope?.turn_id) ? rawEnvelope.turn_id : null,
    cwd: nonEmpty(rawEnvelope?.cwd) ? rawEnvelope.cwd : null,
    rawEnvelope,
  })
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  main().catch(error => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 2
  })
}
