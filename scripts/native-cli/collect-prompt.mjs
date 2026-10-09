#!/usr/bin/env node

import {
  readFileSync,
  realpathSync,
  renameSync,
  writeFileSync,
} from 'node:fs'
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path'
import { pathToFileURL } from 'node:url'

const MAX_INPUT_BYTES = 8 * 1024 * 1024
const SUPPORTED_CLIS = new Set(['claude', 'codex'])

function isPlainObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function invalid(reason) {
  return { valid: false, reason }
}

/**
 * Validate one public UserPromptSubmit envelope against a synthetic fixture.
 *
 * This deliberately performs exact comparisons. It never trims, normalizes,
 * strips paste-looking wrappers, or infers a session/turn from nearby files.
 */
export function validatePromptEvent(event, fixture, cli) {
  if (!SUPPORTED_CLIS.has(cli)) return invalid('UNSUPPORTED_CLI')
  if (!isPlainObject(event) || !isPlainObject(fixture)) {
    return invalid('INVALID_ENVELOPE')
  }
  if (event.hook_event_name !== 'UserPromptSubmit') {
    return invalid('EVENT_MISMATCH')
  }
  if (typeof fixture.sessionId !== 'string' || event.session_id !== fixture.sessionId) {
    return invalid('SESSION_MISMATCH')
  }
  if (
    Object.hasOwn(fixture, 'turnId')
    && (typeof fixture.turnId !== 'string' || event.turn_id !== fixture.turnId)
  ) {
    return invalid('TURN_MISMATCH')
  }
  if (
    Object.hasOwn(fixture, 'expectedCwd')
    && (typeof fixture.expectedCwd !== 'string' || event.cwd !== fixture.expectedCwd)
  ) {
    return invalid('CWD_MISMATCH')
  }
  if (
    typeof fixture.expectedPrompt !== 'string'
    || typeof fixture.nonce !== 'string'
    || !fixture.expectedPrompt.includes(fixture.nonce)
  ) {
    return invalid('INVALID_FIXTURE')
  }
  if (event.prompt !== fixture.expectedPrompt) {
    return invalid('CONTENT_MISMATCH')
  }
  return { valid: true }
}

function fail(message) {
  throw new Error(message)
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

function resolveFixturePath(root, fixturePath) {
  const target = realpathSync(fixturePath)
  if (!inside(root, target)) fail('fixture path outside test root')
  return target
}

function resolveReportPath(root, reportPath) {
  const parent = realpathSync(dirname(reportPath))
  if (!inside(root, parent)) fail('report path outside test root')
  return resolve(reportPath)
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

function parseJson(text, label) {
  try {
    return JSON.parse(text)
  } catch {
    fail(`invalid ${label} JSON`)
  }
}

function writeReport(reportPath, report) {
  const temporaryPath = `${reportPath}.${process.pid}.${Date.now()}.tmp`
  writeFileSync(temporaryPath, `${JSON.stringify(report, null, 2)}\n`, {
    encoding: 'utf8',
    flag: 'wx',
  })
  renameSync(temporaryPath, reportPath)
}

async function main() {
  if (process.env.CC_DESK_SYNTHETIC_TEST_MODE !== '1') {
    fail('synthetic test mode required')
  }
  const rootValue = process.env.CC_DESK_TEST_ROOT
  if (!rootValue || !isAbsolute(rootValue)) {
    fail('missing absolute CC_DESK_TEST_ROOT')
  }

  const root = realpathSync(rootValue)
  const options = parseArgs(process.argv.slice(2))
  const fixturePath = resolveFixturePath(root, options.fixture)
  const reportPath = resolveReportPath(root, options.report)
  const fixture = parseJson(readFileSync(fixturePath, 'utf8'), 'fixture')
  const rawEnvelope = parseJson(await readStdinLimited(), 'hook envelope')

  if (!isPlainObject(fixture) || typeof fixture.transformId !== 'string') {
    fail('invalid fixture contract')
  }

  writeReport(reportPath, {
    schemaVersion: 1,
    cli: options.cli,
    transformId: fixture.transformId,
    validation: validatePromptEvent(rawEnvelope, fixture, options.cli),
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
