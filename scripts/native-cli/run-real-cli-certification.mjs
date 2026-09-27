#!/usr/bin/env node

import { readFileSync, statSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import {
  aggregateD20CertificationResults,
  runD20ProductCertification,
} from './real-cli-certify.mjs'

const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/
const ENV_REF = /^\$\{([A-Za-z_][A-Za-z0-9_]*)\}$/
const MAX_CONFIG_BYTES = 1024 * 1024
const MAX_TIMEOUT_MS = 15 * 60_000

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function fail(reason) {
  return { status: 'FAIL', reason }
}

function blocked(cli, reason) {
  return {
    status: 'BLOCKED',
    cli,
    reason,
    recordPaths: [],
  }
}

function missingConfig(cli) {
  return blocked(cli, 'REAL_CLI_CONFIG_REQUIRED')
}

export function resolveProductCommandConfig(cli, rawConfig, sourceEnv = {}) {
  if (!['claude', 'codex'].includes(cli)) return fail('CLI_KIND_REQUIRED')
  if (!isObject(rawConfig)) return missingConfig(cli)
  if (rawConfig.cli !== cli) return fail('CLI_CONFIG_KIND_MISMATCH')

  const accountSpec = rawConfig.testAccountEnv
  const testAccountEnv = {}
  if (accountSpec !== undefined) {
    if (!isObject(accountSpec)) {
      return fail('INVALID_TEST_ACCOUNT_ENV_REFERENCE')
    }

    for (const [targetName, reference] of Object.entries(accountSpec)) {
      if (!ENV_NAME.test(targetName) || typeof reference !== 'string') {
        return fail('INVALID_TEST_ACCOUNT_ENV_REFERENCE')
      }
      const match = ENV_REF.exec(reference)
      if (!match) {
        return fail('PLAINTEXT_TEST_ACCOUNT_ENV_FORBIDDEN')
      }
      const value = sourceEnv?.[match[1]]
      if (
        typeof value !== 'string'
        || value.length === 0
        || value.includes('\0')
      ) {
        return blocked(cli, 'TEST_ACCOUNT_ENV_UNAVAILABLE')
      }
      testAccountEnv[targetName] = value
    }
  }

  return {
    status: 'READY',
    config: {
      ...rawConfig,
      hostEnv: isObject(sourceEnv) ? sourceEnv : {},
      testAccountEnv,
    },
  }
}

export function runD20CommandConfig(rawConfig, sourceEnv = process.env, options = {}) {
  if (!isObject(rawConfig) || rawConfig.schemaVersion !== 1) {
    return {
      status: 'FAIL',
      reason: 'D20_COMMAND_CONFIG_INVALID',
      failedClis: ['claude', 'codex'],
      results: {
        claude: fail('D20_COMMAND_CONFIG_INVALID'),
        codex: fail('D20_COMMAND_CONFIG_INVALID'),
      },
    }
  }

  const results = {}
  for (const cli of ['claude', 'codex']) {
    const resolved = resolveProductCommandConfig(cli, rawConfig[cli], sourceEnv)
    results[cli] = resolved.status === 'READY'
      ? runD20ProductCertification(cli, resolved.config, options)
      : resolved
  }

  return aggregateD20CertificationResults(results)
}

function parseArgs(argv) {
  let configPath = null
  let timeoutMs

  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index]
    const take = () => {
      if (index + 1 >= argv.length) throw new Error('MISSING_OPTION_VALUE')
      index += 1
      return argv[index]
    }

    if (option === '--config') {
      if (configPath !== null) throw new Error('DUPLICATE_CONFIG')
      configPath = take()
    } else if (option === '--timeout-ms') {
      if (timeoutMs !== undefined) throw new Error('DUPLICATE_TIMEOUT')
      const parsed = Number(take())
      if (!Number.isInteger(parsed) || parsed <= 0 || parsed > MAX_TIMEOUT_MS) {
        throw new Error('INVALID_TIMEOUT')
      }
      timeoutMs = parsed
    } else {
      throw new Error('UNKNOWN_OPTION')
    }
  }

  if (configPath === null) throw new Error('CONFIG_REQUIRED')
  return {
    configPath: resolve(configPath),
    ...(timeoutMs === undefined ? {} : { timeoutMs }),
  }
}

function loadConfig(path) {
  let size
  try {
    size = statSync(path).size
  } catch {
    throw new Error('CONFIG_UNAVAILABLE')
  }
  if (size <= 0 || size > MAX_CONFIG_BYTES) {
    throw new Error('CONFIG_SIZE_INVALID')
  }

  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    throw new Error('INVALID_CONFIG_JSON')
  }
}

function exitCode(result) {
  if (result?.status === 'PASS') return 0
  if (result?.status === 'BLOCKED') return 2
  return 1
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  const config = loadConfig(args.configPath)
  const result = runD20CommandConfig(
    config,
    process.env,
    args.timeoutMs === undefined ? {} : { timeoutMs: args.timeoutMs },
  )
  process.stdout.write(`${JSON.stringify(result)}\n`)
  process.exitCode = exitCode(result)
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  main().catch(error => {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'D20_COMMAND_FAILED'
    process.stderr.write(`[d20-certify-error] ${reason}\n`)
    process.exitCode = 1
  })
}
