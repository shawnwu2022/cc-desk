import {
  executeD20Matrix,
  prepareD20Matrix,
} from './real-cli-runner.mjs'

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function missingConfig(cli) {
  return {
    status: 'BLOCKED',
    cli,
    reason: 'REAL_CLI_CONFIG_REQUIRED',
    recordPaths: [],
  }
}

function configMismatch() {
  return {
    status: 'FAIL',
    reason: 'CLI_CONFIG_KIND_MISMATCH',
  }
}

function executeProduct(cli, config, options) {
  if (!isObject(config)) return missingConfig(cli)
  if (config.cli !== cli) return configMismatch()

  const plan = prepareD20Matrix(config)
  if (plan.status !== 'READY') {
    return {
      status: 'BLOCKED',
      cli,
      reason: plan.reason ?? 'REAL_CLI_PLAN_NOT_READY',
      recordPaths: [],
    }
  }

  return executeD20Matrix(plan, options)
}

export function runD20Certification(config, options = {}) {
  const input = isObject(config) ? config : {}
  const results = {
    claude: executeProduct('claude', input.claude, options),
    codex: executeProduct('codex', input.codex, options),
  }

  const failedClis = Object.entries(results)
    .filter(([, result]) => result.status === 'FAIL')
    .map(([cli]) => cli)

  if (failedClis.length > 0) {
    const configOnly = failedClis.every(
      cli => results[cli].reason === 'CLI_CONFIG_KIND_MISMATCH',
    )
    return {
      status: 'FAIL',
      reason: configOnly
        ? 'REAL_CLI_CERTIFICATION_CONFIG_INVALID'
        : 'REAL_CLI_CERTIFICATION_FAILED',
      failedClis,
      results,
    }
  }

  const blockedClis = Object.entries(results)
    .filter(([, result]) => result.status !== 'PASS')
    .map(([cli]) => cli)

  if (blockedClis.length > 0) {
    return {
      status: 'BLOCKED',
      reason: 'REAL_CLI_EVIDENCE_INCOMPLETE',
      blockedClis,
      results,
    }
  }

  return {
    status: 'PASS',
    results,
  }
}
