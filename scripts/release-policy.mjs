import { coverageArtifactName, validateNativeCoverage } from './windows-native-validation.mjs'

export const REQUIRED_CI_JOBS = Object.freeze(['Frontend checks', 'Rust checks',
  'Disposable roundtrip compile-only policy (no native acceptance)'])

export function sourceChecksPassed(context) {
  const c = context
  if (!c || !['push', 'workflow_dispatch'].includes(c.event) || c.ref !== 'refs/heads/main') return false
  if (!/^[a-f0-9]{40}$/.test(c.sha ?? '') || c.main?.protected !== true || c.main.sha !== c.sha) return false
  if (!Array.isArray(c.versions) || c.versions.length !== 3 || !c.versions.every(v => v === c.versions[0])) return false
  if (!/^\d+\.\d+\.\d+$/.test(c.versions[0]) || c.tag !== `v${c.versions[0]}`) return false
  const ci = c.ci
  if (!Number.isSafeInteger(ci?.id) || ci.id <= 0 || !Number.isSafeInteger(ci.run_attempt) || ci.run_attempt <= 0) return false
  if (!ci || ci.path !== '.github/workflows/ci.yml' || ci.head_sha !== c.sha || ci.head_branch !== 'main' || ci.event !== 'push') return false
  if (ci.status !== 'completed' || ci.conclusion !== 'success') return false
  if (!Array.isArray(c.jobs) || !c.jobs.length || !c.jobs.every(j => j.status === 'completed' && j.conclusion === 'success')) return false
  return REQUIRED_CI_JOBS.every(name => c.jobs.filter(j => j.name === name).length === 1)
}

export function requiredChecksPassed(context) {
  return sourceChecksPassed(context) && context.tagExists === false && context.releaseExists === false
}

export function resolveCoverageArtifact(artifacts, ci, sha, now = Date.now()) {
  if (!Array.isArray(artifacts)) throw new Error('invalid coverage artifact response')
  const expected = coverageArtifactName(sha, String(ci.id), ci.run_attempt)
  const matches = artifacts.filter(artifact => artifact?.name === expected)
  if (matches.length !== 1) throw new Error('missing or duplicate coverage artifact')
  const artifact = matches[0]
  if (!Number.isSafeInteger(artifact.id) || artifact.id <= 0 || artifact.expired !== false
    || !Number.isFinite(Date.parse(artifact.expires_at)) || Date.parse(artifact.expires_at) <= now
    || artifact.workflow_run?.id !== ci.id || artifact.workflow_run?.head_sha !== sha
    || artifact.workflow_run?.head_branch !== 'main') throw new Error('coverage artifact source/run binding failed')
  return artifact
}

export function validateCoverageBinding(binding, artifact, ci, sha) {
  if (!binding || binding.sourceSha !== sha || binding.runId !== String(ci.id)
    || binding.runAttempt !== ci.run_attempt || binding.artifactId !== artifact.id
    || binding.artifactName !== artifact.name
    || artifact.name !== coverageArtifactName(sha, String(ci.id), ci.run_attempt)) {
    throw new Error('coverage binding differs from current CI artifact')
  }
}

export function coverageChecksPassed(context) {
  if (!sourceChecksPassed(context)) return false
  try {
    const { ci, sha, coverage } = context
    const artifact = resolveCoverageArtifact([coverage?.artifact], ci, sha)
    validateCoverageBinding(coverage?.binding, artifact, ci, sha)
    validateNativeCoverage(coverage.report, { sourceSha: sha, runId: String(ci.id), runAttempt: ci.run_attempt })
    return true
  } catch { return false }
}

export function mayPublish(context) {
  return requiredChecksPassed(context) && coverageChecksPassed(context)
}

export function validateArtifacts(artifacts, sha, runId) {
  const names = ['windows', 'macos', 'linux'].map(platform => `cc-desk-candidate-${sha}-${platform}`)
  if (!Array.isArray(artifacts) || artifacts.length !== 3) throw new Error('expected exactly three platform artifacts')
  for (const name of names) {
    const matches = artifacts.filter(a => a.name === name)
    if (matches.length !== 1) throw new Error(`missing or duplicate artifact: ${name}`)
    const artifact = matches[0]
    if (artifact.expired !== false || artifact.workflow_run?.id !== runId || artifact.workflow_run?.head_sha !== sha || artifact.workflow_run?.head_branch !== 'main') {
      throw new Error(`artifact source/run binding failed: ${name}`)
    }
  }
}
