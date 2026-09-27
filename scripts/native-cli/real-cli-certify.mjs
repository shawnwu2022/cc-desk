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

export function runD20ProductCertification(cli, config, options = {}) {
  if (!['claude', 'codex'].includes(cli)) {
    return {
      status: 'FAIL',
      reason: 'CLI_KIND_REQUIRED',
    }
  }
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

export function aggregateD20CertificationResults(results) {
  const value = isObject(results) ? results : {}
  const normalized = {
    claude: isObject(value.claude)
      ? value.claude
      : missingConfig('claude'),
    codex: isObject(value.codex)
      ? value.codex
      : missingConfig('codex'),
  }

  const failedClis = Object.entries(normalized)
    .filter(([, result]) => result.status === 'FAIL')
    .map(([cli]) => cli)

  if (failedClis.length > 0) {
    const configOnly = failedClis.every(
      cli => normalized[cli].reason === 'CLI_CONFIG_KIND_MISMATCH',
    )
    return {
      status: 'FAIL',
      reason: configOnly
        ? 'REAL_CLI_CERTIFICATION_CONFIG_INVALID'
        : 'REAL_CLI_CERTIFICATION_FAILED',
      failedClis,
      results: normalized,
    }
  }

  const blockedClis = Object.entries(normalized)
    .filter(([, result]) => result.status !== 'PASS')
    .map(([cli]) => cli)

  if (blockedClis.length > 0) {
    return {
      status: 'BLOCKED',
      reason: 'REAL_CLI_EVIDENCE_INCOMPLETE',
      blockedClis,
      results: normalized,
    }
  }

  return {
    status: 'PASS',
    results: normalized,
  }
}

export function runD20Certification(config, options = {}) {
  const input = isObject(config) ? config : {}
  return aggregateD20CertificationResults({
    claude: runD20ProductCertification('claude', input.claude, options),
    codex: runD20ProductCertification('codex', input.codex, options),
  })
}
