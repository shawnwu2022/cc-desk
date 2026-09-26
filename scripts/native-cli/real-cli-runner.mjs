import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { extname, isAbsolute, join, relative, resolve } from 'node:path'
import {
  certifyCliComparison,
  validateRealCliRun,
} from './real-cli-evidence.mjs'

const CELLS = Object.freeze([
  ['cc-desk', 'off'],
  ['cc-desk', 'on'],
  ['system-terminal', 'off'],
  ['system-terminal', 'on'],
])

const HOST_ENV_ALLOWLIST = Object.freeze([
  'PATH',
  'Path',
  'LANG',
  'LC_ALL',
  'TERM',
  'SHELL',
  'SystemRoot',
  'WINDIR',
  'ComSpec',
  'PATHEXT',
  'TEMP',
  'TMP',
  'TMPDIR',
])

const MAX_REPORT_BYTES = 16 * 1024 * 1024
const DEFAULT_TIMEOUT_MS = 120_000

function blocked(cli, reason) {
  return { status: 'BLOCKED', cli, reason, runs: [] }
}

function executionBlocked(cli, reason) {
  return { status: 'BLOCKED', cli, reason, recordPaths: [] }
}

function executionFailure(reason, failedRunId) {
  return {
    status: 'FAIL',
    reason,
    ...(failedRunId ? { failedRunId } : {}),
  }
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function nonEmpty(value) {
  return typeof value === 'string' && value.length > 0
}

function contained(root, candidate) {
  if (!nonEmpty(candidate) || !isAbsolute(candidate)) return false
  const rel = relative(root, resolve(candidate))
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel))
}

function realContained(root, candidate) {
  try {
    return contained(realpathSync(root), realpathSync(candidate))
  } catch {
    return false
  }
}

function safeHostEnvironment(hostEnv) {
  const source = isObject(hostEnv) ? hostEnv : {}
  const out = {}
  for (const key of HOST_ENV_ALLOWLIST) {
    if (typeof source[key] === 'string') out[key] = source[key]
  }
  return out
}

function explicitTestAccountEnvironment(value) {
  if (!isObject(value)) return {}
  const out = {}
  for (const [key, item] of Object.entries(value)) {
    if (
      /^[A-Za-z_][A-Za-z0-9_]*$/.test(key)
      && typeof item === 'string'
      && !item.includes('\0')
    ) {
      out[key] = item
    }
  }
  return out
}

function fixtureHash({ nonce, originalText, hostPayloadBase64, transformId }) {
  return createHash('sha256')
    .update(JSON.stringify({
      nonce,
      originalText,
      hostPayloadBase64,
      transformId,
    }), 'utf8')
    .digest('hex')
}

function validTransform(cli, transformId) {
  if (cli === 'codex') return transformId === 'codex-user-prompt-submit-v1-exact'
  if (cli === 'claude') {
    return transformId === 'claude-user-prompt-submit-v1-exact'
      || transformId === 'claude-user-prompt-submit-pasted-content-v1'
  }
  return false
}

function regularFile(path) {
  try {
    return statSync(path).isFile()
  } catch {
    return false
  }
}

function driverCommand(path) {
  const extension = extname(path).toLowerCase()
  if (['.js', '.cjs', '.mjs'].includes(extension)) {
    return { command: process.execPath, prefix: [path] }
  }
  return { command: path, prefix: [] }
}

function writeRunFixture(run) {
  mkdirSync(run.configRoot, { recursive: true, mode: 0o700 })
  mkdirSync(run.env.HOME, { recursive: true, mode: 0o700 })
  mkdirSync(run.projectRoot, { recursive: true, mode: 0o700 })
  if (existsSync(run.fixturePath) || existsSync(run.reportPath)) {
    return false
  }
  try {
    writeFileSync(
      run.fixturePath,
      JSON.stringify(run.fixture),
      { encoding: 'utf8', flag: 'wx', mode: 0o600 },
    )
    return true
  } catch {
    return false
  }
}

function readEvidence(path) {
  if (!existsSync(path)) return { error: 'REAL_CLI_EVIDENCE_MISSING' }
  let size
  try {
    size = statSync(path).size
  } catch {
    return { error: 'REAL_CLI_EVIDENCE_MISSING' }
  }
  if (size <= 0 || size > MAX_REPORT_BYTES) {
    return { error: 'REAL_CLI_EVIDENCE_SIZE_INVALID' }
  }
  try {
    return { record: JSON.parse(readFileSync(path, 'utf8')) }
  } catch {
    return { error: 'REAL_CLI_EVIDENCE_INVALID_JSON' }
  }
}

function recordMatchesCell(record, plan, run) {
  if (!isObject(record) || record.runId !== run.runId) return false
  if (record.status === 'BLOCKED') return record.cli === plan.cli
  return record.lane === run.lane && record.observer === run.observer
}

export function prepareD20Matrix(config) {
  const cli = config?.cli
  if (!['claude', 'codex'].includes(cli)) {
    return blocked(cli ?? null, 'CLI_KIND_REQUIRED')
  }

  // Authorization is checked first. A caller without a dedicated test account
  // must not probe binaries, drivers, HOME, or any native CLI configuration.
  if (config?.authorizedTestAccount !== true) {
    return blocked(cli, 'AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE')
  }

  if (!nonEmpty(config.testRoot) || !isAbsolute(config.testRoot)) {
    return blocked(cli, 'TEST_ROOT_REQUIRED')
  }
  const testRoot = resolve(config.testRoot)

  if (!contained(testRoot, config.binaryPath)) {
    return blocked(cli, 'REAL_CLI_BINARY_NOT_ISOLATED')
  }
  if (
    !isObject(config.drivers)
    || !contained(testRoot, config.drivers.ccDesk)
    || !contained(testRoot, config.drivers.systemTerminal)
  ) {
    return blocked(cli, 'REAL_CLI_DRIVER_NOT_ISOLATED')
  }
  if (
    !nonEmpty(config.nonce)
    || !nonEmpty(config.originalText)
    || !config.originalText.includes(config.nonce)
    || !validTransform(cli, config.transformId)
  ) {
    return blocked(cli, 'REAL_CLI_FIXTURE_REQUIRED')
  }

  const hostPayload = typeof config.hostPayloadText === 'string'
    ? config.hostPayloadText
    : config.originalText
  if (!hostPayload.includes(config.nonce)) {
    return blocked(cli, 'RUN_NONCE_REQUIRED')
  }
  const hostPayloadBase64 = Buffer.from(hostPayload, 'utf8').toString('base64')
  const fixtureSha256 = fixtureHash({
    nonce: config.nonce,
    originalText: config.originalText,
    hostPayloadBase64,
    transformId: config.transformId,
  })

  const hostEnv = safeHostEnvironment(config.hostEnv)
  const accountEnv = explicitTestAccountEnvironment(config.testAccountEnv)

  const runs = CELLS.map(([lane, observer]) => {
    const cell = `${lane}-${observer}`
    const runId = `${cli}-${cell}-${config.nonce}`
    const runRoot = join(testRoot, 'runs', cli, cell)
    const configRoot = join(runRoot, 'config')
    const homeRoot = join(runRoot, 'home')
    const projectRoot = join(runRoot, 'project')
    const reportPath = join(runRoot, 'evidence.json')
    const fixturePath = join(runRoot, 'fixture.json')

    const env = {
      ...hostEnv,
      ...accountEnv,
      HOME: homeRoot,
      USERPROFILE: homeRoot,
      XDG_CONFIG_HOME: join(configRoot, 'xdg'),
      CC_DESK_REAL_CLI_TEST_MODE: '1',
      CC_DESK_TEST_ROOT: testRoot,
      CC_DESK_D20_RUN_ID: runId,
    }
    if (cli === 'codex') {
      env.CODEX_HOME = configRoot
      delete env.CLAUDE_CONFIG_DIR
    } else {
      env.CLAUDE_CONFIG_DIR = configRoot
      delete env.CODEX_HOME
    }

    return {
      cli,
      lane,
      observer,
      runId,
      testRoot,
      runRoot,
      configRoot,
      projectRoot,
      reportPath,
      fixturePath,
      binaryPath: resolve(config.binaryPath),
      driverPath: resolve(
        lane === 'cc-desk'
          ? config.drivers.ccDesk
          : config.drivers.systemTerminal,
      ),
      env,
      fixture: {
        schemaVersion: 1,
        cli,
        runId,
        lane,
        observer,
        expectedCwd: projectRoot,
        nonce: config.nonce,
        originalText: config.originalText,
        hostPayloadBase64,
        transformId: config.transformId,
        fixtureSha256,
      },
    }
  })

  return {
    status: 'READY',
    cli,
    testRoot,
    runs,
  }
}

export function executeD20Matrix(plan, options = {}) {
  if (!isObject(plan)) {
    return executionFailure('INVALID_REAL_CLI_PLAN')
  }
  if (plan.status !== 'READY') {
    return executionBlocked(plan.cli ?? null, plan.reason ?? 'REAL_CLI_PLAN_NOT_READY')
  }
  if (
    !['claude', 'codex'].includes(plan.cli)
    || !nonEmpty(plan.testRoot)
    || !Array.isArray(plan.runs)
    || plan.runs.length !== 4
  ) {
    return executionFailure('INVALID_REAL_CLI_PLAN')
  }
  if (!existsSync(plan.testRoot)) {
    return executionBlocked(plan.cli, 'TEST_ROOT_UNAVAILABLE')
  }

  const first = plan.runs[0]
  if (!regularFile(first.binaryPath)) {
    return executionBlocked(plan.cli, 'REAL_CLI_BINARY_UNAVAILABLE')
  }
  if (!realContained(plan.testRoot, first.binaryPath)) {
    return executionBlocked(plan.cli, 'REAL_CLI_BINARY_NOT_ISOLATED')
  }

  for (const run of plan.runs) {
    if (!regularFile(run.driverPath)) {
      return executionBlocked(plan.cli, 'REAL_CLI_DRIVER_UNAVAILABLE')
    }
    if (!realContained(plan.testRoot, run.driverPath)) {
      return executionBlocked(plan.cli, 'REAL_CLI_DRIVER_NOT_ISOLATED')
    }
  }

  const timeoutMs = Number.isInteger(options.timeoutMs)
    && options.timeoutMs > 0
    && options.timeoutMs <= 15 * 60_000
    ? options.timeoutMs
    : DEFAULT_TIMEOUT_MS

  const records = []
  const recordPaths = []
  for (const run of plan.runs) {
    if (!writeRunFixture(run)) {
      return executionFailure('REAL_CLI_RUN_ROOT_NOT_FRESH', run.runId)
    }

    const driver = driverCommand(run.driverPath)
    const result = spawnSync(
      driver.command,
      [
        ...driver.prefix,
        '--cli', run.cli,
        '--binary', run.binaryPath,
        '--fixture', run.fixturePath,
        '--report', run.reportPath,
        '--lane', run.lane,
        '--observer', run.observer,
      ],
      {
        cwd: run.projectRoot,
        env: run.env,
        encoding: 'utf8',
        timeout: timeoutMs,
        maxBuffer: 1024 * 1024,
        windowsHide: true,
      },
    )

    if (result.stdout !== '') {
      return executionFailure('REAL_CLI_DRIVER_STDOUT_FORBIDDEN', run.runId)
    }
    if (result.error || result.signal || result.status !== 0) {
      return executionFailure('REAL_CLI_DRIVER_FAILED', run.runId)
    }

    const evidence = readEvidence(run.reportPath)
    if (evidence.error) {
      return executionFailure(evidence.error, run.runId)
    }
    const record = evidence.record
    if (!recordMatchesCell(record, plan, run)) {
      return executionFailure('REAL_CLI_RECORD_PROVENANCE_MISMATCH', run.runId)
    }
    const validation = validateRealCliRun(record)
    if (!validation.valid) {
      return executionFailure(
        `INVALID_REAL_CLI_EVIDENCE:${validation.reason}`,
        run.runId,
      )
    }
    records.push(record)
    recordPaths.push(run.reportPath)
  }

  const comparison = certifyCliComparison(records)
  return {
    ...comparison,
    recordPaths,
  }
}
