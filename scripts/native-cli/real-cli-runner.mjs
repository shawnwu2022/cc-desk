import { createHash } from 'node:crypto'
import { isAbsolute, join, relative, resolve } from 'node:path'

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

function blocked(cli, reason) {
  return { status: 'BLOCKED', cli, reason, runs: [] }
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
