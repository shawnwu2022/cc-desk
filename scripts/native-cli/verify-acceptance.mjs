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

export function computeAcceptanceCatalogId(catalog) {
  return sha256(Buffer.from(canonical({
    schemaVersion: catalog.schemaVersion,
    catalogVersion: catalog.catalogVersion,
    policy: catalog.policy,
    cases: catalog.cases,
  })))
}

export function computeTargetIdentitySha256(identity) {
  return sha256(Buffer.from(canonical(identity)))
}

export function computeTargetPlanId(plan) {
  return sha256(Buffer.from(canonical({
    schemaVersion: 1,
    candidateId: plan.candidateId,
    catalogId: plan.catalogId,
    targets: plan.targets,
  })))
}

function requirementKey(targetId, caseId, subcaseId) {
  return [targetId, caseId, subcaseId].join('\0')
}

function validateCatalog(catalog) {
  if (!catalog || catalog.schemaVersion !== 1
    || !Number.isSafeInteger(catalog.catalogVersion) || catalog.catalogVersion <= 0
    || !SHA256.test(String(catalog.catalogId ?? ''))
    || computeAcceptanceCatalogId(catalog) !== catalog.catalogId
    || catalog.policy?.allRequiredCasesMustAppearPerTarget !== true
    || catalog.policy?.naRequiresEvidence !== true
    || catalog.policy?.planMayAddLayersButNotRemoveMinimumLayers !== true
    || !Array.isArray(catalog.cases)) {
    fail('ACCEPTANCE_CATALOG_INVALID')
  }

  const expectedIds = Array.from({ length: 64 }, (_, index) =>
    `NATIVE-${String(index + 1).padStart(2, '0')}`)
  if (catalog.cases.length !== expectedIds.length) fail('ACCEPTANCE_CATALOG_INCOMPLETE')

  const cases = new Map()
  for (const entry of catalog.cases) {
    if (!entry || !CASE_ID.test(String(entry.caseId ?? ''))
      || typeof entry.owner !== 'string' || !/^W[0-9]$/.test(entry.owner)
      || entry.required !== true || entry.allowNa !== true
      || entry.subcasePolicy !== 'explicit'
      || !Array.isArray(entry.minimumLayers) || entry.minimumLayers.length === 0
      || entry.minimumLayers.some(layer => !LAYERS.includes(layer))
      || new Set(entry.minimumLayers).size !== entry.minimumLayers.length) {
      fail('ACCEPTANCE_CATALOG_INVALID')
    }
    if (cases.has(entry.caseId)) fail('ACCEPTANCE_CATALOG_DUPLICATE')
    cases.set(entry.caseId, entry)
  }
  if (expectedIds.some(id => !cases.has(id))) fail('ACCEPTANCE_CATALOG_INCOMPLETE')
  return cases
}

function validatePlan(plan, candidate, catalog) {
  const catalogCases = validateCatalog(catalog)
  if (!plan || plan.schemaVersion !== 1 || plan.candidateId !== candidate.candidateId
    || plan.catalogId !== catalog.catalogId
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
      || !target.identity || typeof target.identity !== 'object' || Array.isArray(target.identity)
      || !Array.isArray(target.requirements) || target.requirements.length === 0) {
      fail('ACCEPTANCE_PLAN_INVALID')
    }

    const identity = target.identity
    const nonempty = value => typeof value === 'string' && value.length > 0 && !value.includes('\0')
    if (identity.platform !== target.platform
      || !identity.os || !nonempty(identity.os.name) || !nonempty(identity.os.build) || !nonempty(identity.os.arch)
      || !nonempty(identity.executionDomain)
      || !identity.cli || !['claude', 'codex'].includes(identity.cli.kind)
      || !nonempty(identity.cli.version) || !SHA256.test(String(identity.cli.binarySha256 ?? ''))
      || !identity.desk || identity.desk.candidateId !== candidate.candidateId
      || identity.desk.sourceSha !== candidate.sourceSha
      || !SHA256.test(String(identity.desk.packageSha256 ?? ''))
      || !identity.runtime || !nonempty(identity.runtime.webView)
      || !nonempty(identity.runtime.xtermVersion) || !nonempty(identity.runtime.renderer)
      || !identity.launcher || !['native', 'shell', 'shim'].includes(identity.launcher.kind)
      || (identity.launcher.kind !== 'native' && !nonempty(identity.launcher.shellVersion))
      || !nonempty(identity.inputPolicyVersion)
      || !nonempty(identity.terminalProtocolVersion)
      || !nonempty(identity.fixtureConfigVersion)
      || (target.platform === 'windows-x86_64'
        ? !nonempty(identity.runtime.conptyVersion)
        : identity.runtime.conptyVersion !== null)
      || computeTargetIdentitySha256(identity) !== target.identitySha256) {
      fail('ACCEPTANCE_TARGET_IDENTITY_INVALID')
    }

    const packageHashes = candidatePackageHashes(candidate, target.platform)
    if (!packageHashes.has(identity.desk.packageSha256)) {
      fail('ACCEPTANCE_TARGET_PACKAGE_MISMATCH')
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
      const catalogCase = catalogCases.get(requirement.caseId)
      if (!catalogCase) fail('ACCEPTANCE_REQUIREMENT_NOT_IN_CATALOG')
      const key = requirementKey(target.targetId, requirement.caseId, requirement.subcaseId)
      if (requirements.has(key)) fail('ACCEPTANCE_REQUIREMENT_DUPLICATE')
      requirements.set(key, { target, requirement, catalogCase })
    }

    for (const [caseId, catalogCase] of catalogCases) {
      const caseRequirements = target.requirements.filter(item => item.caseId === caseId)
      if (caseRequirements.length === 0) fail('ACCEPTANCE_REQUIRED_CASE_MISSING')
      const plannedLayers = new Set(caseRequirements.flatMap(item => item.requiredLayers))
      for (const layer of catalogCase.minimumLayers) {
        if (!plannedLayers.has(layer)) fail('ACCEPTANCE_MINIMUM_LAYER_MISSING')
      }
    }
  }
  return { requirements, catalogCases }
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
  const { target, requirement, catalogCase } = requirementEntry
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

  if (record.cliKind !== target.identity.cli.kind
    || record.cliVersion !== target.identity.cli.version
    || record.cliBinarySha256 !== target.identity.cli.binarySha256) {
    fail('ACCEPTANCE_CLI_IDENTITY_MISMATCH')
  }

  if (TERMINAL_FAILURES.has(record.status)) fail('ACCEPTANCE_NONPASS_RESULT')
  verifyEvidenceFiles(record, evidenceRoot)

  if (record.status === 'N_A') {
    if (catalogCase.allowNa !== true) fail('ACCEPTANCE_NA_FORBIDDEN')
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

  if (record.evidenceLayer !== 'A') {
    const verification = record.verification
    if (!verification || typeof verification !== 'object' || Array.isArray(verification)
      || typeof verification.kind !== 'string'
      || !/^[a-z0-9][a-z0-9._-]{2,63}$/.test(verification.kind)
      || !Number.isSafeInteger(verification.schemaVersion) || verification.schemaVersion <= 0
      || !SHA256.test(String(verification.resultEvidenceSha256 ?? ''))) {
      fail('ACCEPTANCE_VERIFICATION_REQUIRED')
    }
    if (!record.evidence.some(item => item.sha256 === verification.resultEvidenceSha256)) {
      fail('ACCEPTANCE_VERIFICATION_EVIDENCE_MISMATCH')
    }
  }

  if (record.evidenceLayer === 'D') {
    if (!SHA256.test(String(record.deskPackageSha256 ?? ''))
      || record.deskPackageSha256 !== target.identity.desk.packageSha256
      || !candidatePackageHashes(candidate, target.platform).has(record.deskPackageSha256)) {
      fail('ACCEPTANCE_PACKAGE_HASH_MISMATCH')
    }
  }
}

export function verifyAcceptance({ candidate, candidateRoot, catalog, plan, records, evidenceRoot }) {
  verifyCandidateFiles(candidate, candidateRoot)
  const { requirements } = validatePlan(plan, candidate, catalog)

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
    catalogId: catalog.catalogId,
    planId: plan.planId,
    targetCount: plan.targets.length,
    requirementCount: requirements.size,
    recordCount: records.length,
  }
}

function main() {
  const [candidatePath, candidateRoot, catalogPath, planPath, recordsRoot, evidenceRoot] = process.argv.slice(2)
  if (!candidatePath || !candidateRoot || !catalogPath || !planPath || !recordsRoot || !evidenceRoot) {
    fail('usage: verify-acceptance.mjs <candidate.json> <candidate-root> <catalog.json> <plan.json> <records-dir> <evidence-dir>')
  }
  const candidate = JSON.parse(readFileSync(resolve(candidatePath), 'utf8'))
  const catalog = JSON.parse(readFileSync(resolve(catalogPath), 'utf8'))
  const plan = JSON.parse(readFileSync(resolve(planPath), 'utf8'))
  const records = loadRecords(recordsRoot)
  const result = verifyAcceptance({ candidate, candidateRoot, catalog, plan, records, evidenceRoot })
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
