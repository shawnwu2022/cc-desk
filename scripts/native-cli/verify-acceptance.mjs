import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { relative, resolve, sep } from 'node:path'
import { pathToFileURL } from 'node:url'
import { verifyCandidateFiles } from './candidate-manifest.mjs'

const SHA256 = /^[0-9a-f]{64}$/
const SOURCE_SHA = /^[0-9a-f]{40,64}$/
const CASE_ID = /^NATIVE-(?:0[1-9]|[1-5][0-9]|6[0-4])$/
const LAYERS = ['A', 'B', 'C', 'D']
const TERMINAL_FAILURES = new Set(['FAIL', 'BLOCKED', 'NOT_RUN'])

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function canonical(value) {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']'
  if (value && typeof value === 'object') {
    return '{' + Object.keys(value).sort().map(key => JSON.stringify(key) + ':' + canonical(value[key])).join(',') + '}'
  }
  return JSON.stringify(value)
}

export function computeTargetPlanId(plan) {
  return sha256(Buffer.from(canonical({
    schemaVersion: 1,
    candidateId: plan.candidateId,
    targets: plan.targets,
  })))
}

function requirementKey(targetId, caseId, subcaseId) {
  return [targetId, caseId, subcaseId].join('\0')
}

function validatePlan(plan, candidate) {
  if (!plan || plan.schemaVersion !== 1 || plan.candidateId !== candidate.candidateId
    || !Array.isArray(plan.targets) || plan.targets.length === 0) {
    fail('ACCEPTANCE_PLAN_INVALID')
  }
  if (!SHA256.test(String(plan.planId ?? '')) || computeTargetPlanId(plan) !== plan.planId) {
    fail('ACCEPTANCE_PLAN_ID_MISMATCH')
  }

  const targetIds = new Set()
  const requirements = new Map()
  for (const target of plan.targets) {
    if (!target || typeof target.targetId !== 'string' || !target.targetId
      || typeof target.platform !== 'string' || !target.platform
      || !SHA256.test(String(target.identitySha256 ?? ''))
      || !target.cli || !['claude', 'codex'].includes(target.cli.kind)
      || typeof target.cli.version !== 'string' || !target.cli.version
      || !SHA256.test(String(target.cli.binarySha256 ?? ''))
      || !Array.isArray(target.requirements) || target.requirements.length === 0) {
      fail('ACCEPTANCE_PLAN_INVALID')
    }
    if (targetIds.has(target.targetId)) fail('ACCEPTANCE_TARGET_DUPLICATE')
    targetIds.add(target.targetId)

    const candidatePlatformFiles = candidate.files.filter(file =>
      file.platform === target.platform && !file.path.endsWith('.sig'))
    if (candidatePlatformFiles.length === 0) fail('ACCEPTANCE_TARGET_PLATFORM_MISSING')

    for (const requirement of target.requirements) {
      if (!requirement || !CASE_ID.test(String(requirement.caseId ?? ''))
        || typeof requirement.subcaseId !== 'string' || !requirement.subcaseId
        || !Array.isArray(requirement.requiredLayers) || requirement.requiredLayers.length === 0
        || requirement.requiredLayers.some(layer => !LAYERS.includes(layer))) {
        fail('ACCEPTANCE_REQUIREMENT_INVALID')
      }
      const uniqueLayers = new Set(requirement.requiredLayers)
      if (uniqueLayers.size !== requirement.requiredLayers.length) {
        fail('ACCEPTANCE_REQUIREMENT_DUPLICATE_LAYER')
      }
      const key = requirementKey(target.targetId, requirement.caseId, requirement.subcaseId)
      if (requirements.has(key)) fail('ACCEPTANCE_REQUIREMENT_DUPLICATE')
      requirements.set(key, { target, requirement })
    }
  }
  return requirements
}

function walkJson(root) {
  const output = []
  function visit(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const full = resolve(dir, entry.name)
      if (entry.isDirectory()) visit(full)
      else if (entry.isFile() && entry.name.endsWith('.json')) output.push(full)
    }
  }
  visit(resolve(root))
  return output
}

function loadRecords(recordsRoot) {
  const records = []
  for (const file of walkJson(recordsRoot)) {
    let value
    try {
      value = JSON.parse(readFileSync(file, 'utf8'))
    } catch {
      fail('ACCEPTANCE_RECORD_JSON_INVALID')
    }
    if (Array.isArray(value)) records.push(...value)
    else records.push(value)
  }
  return records
}

function confined(root, supplied) {
  const base = resolve(root)
  const full = resolve(base, supplied)
  const rel = relative(base, full).split(sep).join('/')
  if (!rel || rel === '..' || rel.startsWith('../')) fail('ACCEPTANCE_EVIDENCE_PATH_INVALID')
  return { full, rel }
}

function verifyEvidenceFiles(record, evidenceRoot) {
  if (!Array.isArray(record.evidence) || record.evidence.length === 0) {
    fail('ACCEPTANCE_EVIDENCE_REQUIRED')
  }
  const paths = new Set()
  for (const evidence of record.evidence) {
    if (!evidence || typeof evidence.path !== 'string' || !evidence.path
      || !SHA256.test(String(evidence.sha256 ?? ''))) {
      fail('ACCEPTANCE_EVIDENCE_INVALID')
    }
    const { full, rel } = confined(evidenceRoot, evidence.path)
    if (paths.has(rel)) fail('ACCEPTANCE_EVIDENCE_DUPLICATE')
    paths.add(rel)
    let bytes
    try {
      if (!statSync(full).isFile()) fail('ACCEPTANCE_EVIDENCE_MISSING')
      bytes = readFileSync(full)
    } catch {
      fail('ACCEPTANCE_EVIDENCE_MISSING')
    }
    if (sha256(bytes) !== evidence.sha256) fail('ACCEPTANCE_EVIDENCE_HASH_MISMATCH')
  }
}

function candidatePackageHashes(candidate, platform) {
  return new Set(candidate.files
    .filter(file => file.platform === platform && !file.path.endsWith('.sig'))
    .map(file => file.sha256))
}

function validateRecord(record, plan, candidate, requirementEntry, evidenceRoot) {
  const { target, requirement } = requirementEntry
  if (!record || record.schemaVersion !== 2
    || record.targetId !== target.targetId
    || record.caseId !== requirement.caseId
    || record.subcaseId !== requirement.subcaseId
    || typeof record.runId !== 'string' || !record.runId
    || record.candidateId !== candidate.candidateId
    || record.sourceSha !== candidate.sourceSha
    || record.targetIdentitySha256 !== target.identitySha256
    || !LAYERS.includes(record.evidenceLayer)
    || !['PASS', 'FAIL', 'BLOCKED', 'NOT_RUN', 'N_A'].includes(record.status)) {
    fail('ACCEPTANCE_RECORD_INVALID')
  }

  if (record.cliKind !== target.cli.kind
    || record.cliVersion !== target.cli.version
    || record.cliBinarySha256 !== target.cli.binarySha256) {
    fail('ACCEPTANCE_CLI_IDENTITY_MISMATCH')
  }

  if (TERMINAL_FAILURES.has(record.status)) fail('ACCEPTANCE_NONPASS_RESULT')
  verifyEvidenceFiles(record, evidenceRoot)

  if (record.status === 'N_A') {
    if (typeof record.nonApplicabilityReason !== 'string'
      || record.nonApplicabilityReason.trim().length < 8
      || record.nonApplicabilityEvidenceSha256 == null
      || !SHA256.test(String(record.nonApplicabilityEvidenceSha256))) {
      fail('ACCEPTANCE_NA_UNJUSTIFIED')
    }
    if (!record.evidence.some(item => item.sha256 === record.nonApplicabilityEvidenceSha256)) {
      fail('ACCEPTANCE_NA_EVIDENCE_MISMATCH')
    }
    return
  }

  if (record.status !== 'PASS') fail('ACCEPTANCE_RECORD_INVALID')
  if (record.nonApplicabilityReason != null || record.nonApplicabilityEvidenceSha256 != null) {
    fail('ACCEPTANCE_RECORD_INVALID')
  }

  if (record.evidenceLayer === 'D') {
    if (!SHA256.test(String(record.deskPackageSha256 ?? ''))
      || !candidatePackageHashes(candidate, target.platform).has(record.deskPackageSha256)) {
      fail('ACCEPTANCE_PACKAGE_HASH_MISMATCH')
    }
  }
}

export function verifyAcceptance({ candidate, candidateRoot, plan, records, evidenceRoot }) {
  verifyCandidateFiles(candidate, candidateRoot)
  const requirements = validatePlan(plan, candidate)

  const recordsByRequirement = new Map()
  const unique = new Set()
  for (const record of records) {
    const key = requirementKey(record?.targetId, record?.caseId, record?.subcaseId)
    const requirementEntry = requirements.get(key)
    if (!requirementEntry) fail('ACCEPTANCE_UNPLANNED_RECORD')
    const duplicateKey = [key, record.evidenceLayer, record.status].join('\0')
    if (unique.has(duplicateKey)) fail('ACCEPTANCE_DUPLICATE_RESULT')
    unique.add(duplicateKey)
    validateRecord(record, plan, candidate, requirementEntry, evidenceRoot)
    const bucket = recordsByRequirement.get(key) ?? []
    bucket.push(record)
    recordsByRequirement.set(key, bucket)
  }

  for (const [key, { requirement }] of requirements) {
    const bucket = recordsByRequirement.get(key) ?? []
    if (bucket.length === 0) fail('ACCEPTANCE_REQUIRED_RESULT_MISSING')
    const na = bucket.filter(record => record.status === 'N_A')
    if (na.length > 1) fail('ACCEPTANCE_DUPLICATE_RESULT')
    if (na.length === 1) {
      if (bucket.length !== 1) fail('ACCEPTANCE_NA_CONFLICT')
      continue
    }
    for (const layer of requirement.requiredLayers) {
      const matches = bucket.filter(record => record.status === 'PASS' && record.evidenceLayer === layer)
      if (matches.length !== 1) {
        fail(matches.length === 0 ? 'ACCEPTANCE_REQUIRED_LAYER_MISSING' : 'ACCEPTANCE_DUPLICATE_RESULT')
      }
    }
    if (bucket.some(record => !requirement.requiredLayers.includes(record.evidenceLayer))) {
      fail('ACCEPTANCE_UNPLANNED_LAYER')
    }
  }

  return {
    schemaVersion: 1,
    status: 'PASS',
    candidateId: candidate.candidateId,
    planId: plan.planId,
    targetCount: plan.targets.length,
    requirementCount: requirements.size,
    recordCount: records.length,
  }
}

function main() {
  const [candidatePath, candidateRoot, planPath, recordsRoot, evidenceRoot] = process.argv.slice(2)
  if (!candidatePath || !candidateRoot || !planPath || !recordsRoot || !evidenceRoot) {
    fail('usage: verify-acceptance.mjs <candidate.json> <candidate-root> <plan.json> <records-dir> <evidence-dir>')
  }
  const candidate = JSON.parse(readFileSync(resolve(candidatePath), 'utf8'))
  const plan = JSON.parse(readFileSync(resolve(planPath), 'utf8'))
  const records = loadRecords(recordsRoot)
  const result = verifyAcceptance({ candidate, candidateRoot, plan, records, evidenceRoot })
  process.stdout.write(JSON.stringify(result) + '\n')
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code ?? error?.message ?? 'ACCEPTANCE_GATE_FAILED') + '\n')
    process.exit(1)
  }
}
