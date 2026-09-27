#!/usr/bin/env node

import { createHash } from 'node:crypto'
import {
  readFileSync,
  realpathSync,
  statSync,
} from 'node:fs'
import {
  isAbsolute,
  relative,
  resolve,
} from 'node:path'
import { pathToFileURL } from 'node:url'

const SHA256 = /^[0-9a-f]{64}$/i
const COMMIT_SHA = /^[0-9a-f]{40}$/i
const CASE_ID = /^NATIVE-(0[1-9]|[1-5][0-9]|6[0-4])$/
const OWNER = /^W[0-9]$/
const TOKEN = /^[A-Za-z0-9._-]{1,128}$/
const SUBCASE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/
const LAYERS = new Set(['A', 'B', 'C', 'D'])
const REQUIRED_STATUSES = new Set(['PASS', 'FAIL', 'BLOCKED', 'NOT_RUN'])
const MAX_JSON_BYTES = 8 * 1024 * 1024
const MAX_EVIDENCE_FILE_BYTES = 256 * 1024 * 1024

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function fail(reason, detail = undefined) {
  return {
    status: 'FAIL',
    reason,
    ...(detail === undefined ? {} : { detail }),
  }
}

function text(value, maximum = 4096) {
  return typeof value === 'string'
    && value.length > 0
    && value.length <= maximum
    && !value.includes('\0')
}

function sha256Bytes(value) {
  return createHash('sha256').update(value).digest('hex')
}

function unique(values) {
  return new Set(values).size === values.length
}

function expectedCaseIds() {
  return Array.from(
    { length: 64 },
    (_, index) => `NATIVE-${String(index + 1).padStart(2, '0')}`,
  )
}

function canonicalArtifacts(candidate) {
  if (!Array.isArray(candidate?.artifacts)) return null
  const values = candidate.artifacts.map(value => ({
    name: value?.name,
    path: value?.path,
    sha256: typeof value?.sha256 === 'string'
      ? value.sha256.toLowerCase()
      : value?.sha256,
  }))
  values.sort((left, right) => String(left.name).localeCompare(String(right.name)))
  return values
}

export function acceptanceCandidateId(candidate) {
  const identity = {
    schemaVersion: candidate?.schemaVersion,
    deskCommit: typeof candidate?.deskCommit === 'string'
      ? candidate.deskCommit.toLowerCase()
      : candidate?.deskCommit,
    artifacts: canonicalArtifacts(candidate),
  }
  return sha256Bytes(Buffer.from(JSON.stringify(identity), 'utf8'))
}

function validateCatalog(catalog) {
  if (!isObject(catalog) || catalog.schemaVersion !== 1 || catalog.specVersion !== 2) {
    return fail('INVALID_ACCEPTANCE_CATALOG')
  }
  if (!Array.isArray(catalog.cases) || catalog.cases.length !== 64) {
    return fail('ACCEPTANCE_CATALOG_CASE_COUNT')
  }

  const ids = []
  const byId = new Map()
  for (const entry of catalog.cases) {
    if (
      !isObject(entry)
      || !CASE_ID.test(entry.caseId ?? '')
      || !OWNER.test(entry.owner ?? '')
      || !Array.isArray(entry.requiredSubcaseIds)
      || entry.requiredSubcaseIds.some(value => !SUBCASE.test(value ?? ''))
      || !unique(entry.requiredSubcaseIds)
      || (entry.allowNA !== undefined && typeof entry.allowNA !== 'boolean')
    ) {
      return fail('INVALID_ACCEPTANCE_CATALOG_CASE')
    }
    if (byId.has(entry.caseId)) return fail('DUPLICATE_CATALOG_CASE')
    const normalized = {
      caseId: entry.caseId,
      owner: entry.owner,
      requiredSubcaseIds: [...entry.requiredSubcaseIds],
      allowNA: entry.allowNA !== false,
    }
    ids.push(entry.caseId)
    byId.set(entry.caseId, normalized)
  }

  const expected = expectedCaseIds()
  if (ids.slice().sort().join('\n') !== expected.join('\n')) {
    return fail('ACCEPTANCE_CATALOG_CASE_SET')
  }

  return {
    status: 'PASS',
    specVersion: catalog.specVersion,
    byId,
  }
}

function safeRelativePath(value) {
  if (
    !text(value, 4096)
    || isAbsolute(value)
    || value.split(/[\\/]+/).some(part => part === '..')
  ) {
    return false
  }
  return true
}

function containedFile(root, path, missingReason) {
  if (!text(root, 32768) || !safeRelativePath(path)) {
    return { valid: false, reason: 'INVALID_EVIDENCE_PATH' }
  }

  let realRoot
  let realFile
  let size
  try {
    realRoot = realpathSync(resolve(root))
    realFile = realpathSync(resolve(root, path))
    size = statSync(realFile)
  } catch {
    return { valid: false, reason: missingReason }
  }

  const rel = relative(realRoot, realFile)
  if (
    rel === ''
    || rel === '..'
    || rel.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`)
    || isAbsolute(rel)
    || !size.isFile()
    || size.size <= 0
    || size.size > MAX_EVIDENCE_FILE_BYTES
  ) {
    return { valid: false, reason: 'EVIDENCE_PATH_ESCAPE' }
  }

  return { valid: true, path: realFile }
}

function verifyFileRef(root, reference, options = {}) {
  if (
    !isObject(reference)
    || !safeRelativePath(reference.path)
    || !SHA256.test(reference.sha256 ?? '')
  ) {
    return fail(options.invalidReason ?? 'INVALID_EVIDENCE_REFERENCE')
  }

  const file = containedFile(
    root,
    reference.path,
    options.missingReason ?? 'EVIDENCE_FILE_MISSING',
  )
  if (!file.valid) return fail(file.reason)

  let bytes
  try {
    bytes = readFileSync(file.path)
  } catch {
    return fail(options.missingReason ?? 'EVIDENCE_FILE_MISSING')
  }

  if (sha256Bytes(bytes) !== reference.sha256.toLowerCase()) {
    return fail(options.hashReason ?? 'EVIDENCE_FILE_HASH_MISMATCH')
  }
  return { status: 'PASS' }
}

function validateCandidate(candidate, artifactRoot) {
  if (
    !isObject(candidate)
    || candidate.schemaVersion !== 1
    || !SHA256.test(candidate.candidateId ?? '')
    || !COMMIT_SHA.test(candidate.deskCommit ?? '')
    || !Array.isArray(candidate.artifacts)
    || candidate.artifacts.length === 0
    || candidate.artifacts.length > 32
  ) {
    return fail('INVALID_CANDIDATE_MANIFEST')
  }

  if (candidate.candidateId.toLowerCase() !== acceptanceCandidateId(candidate)) {
    return fail('CANDIDATE_ID_MISMATCH')
  }

  const names = new Set()
  const paths = new Set()
  const artifacts = new Map()
  for (const artifact of candidate.artifacts) {
    if (
      !isObject(artifact)
      || !TOKEN.test(artifact.name ?? '')
      || !safeRelativePath(artifact.path)
      || !SHA256.test(artifact.sha256 ?? '')
    ) {
      return fail('INVALID_CANDIDATE_ARTIFACT')
    }
    if (names.has(artifact.name) || paths.has(artifact.path)) {
      return fail('DUPLICATE_CANDIDATE_ARTIFACT')
    }
    names.add(artifact.name)
    paths.add(artifact.path)

    const checked = verifyFileRef(artifactRoot, artifact, {
      invalidReason: 'INVALID_CANDIDATE_ARTIFACT',
      missingReason: 'CANDIDATE_ARTIFACT_MISSING',
      hashReason: 'CANDIDATE_ARTIFACT_HASH_MISMATCH',
    })
    if (checked.status !== 'PASS') return checked

    artifacts.set(artifact.name, {
      name: artifact.name,
      path: artifact.path,
      sha256: artifact.sha256.toLowerCase(),
    })
  }

  return {
    status: 'PASS',
    candidateId: candidate.candidateId.toLowerCase(),
    deskCommit: candidate.deskCommit.toLowerCase(),
    artifacts,
  }
}

function evidenceKey(targetId, caseId, subcaseId, layer) {
  return JSON.stringify([targetId, caseId, subcaseId, layer])
}

function validateNaBasis(decision, evidenceRoot) {
  if (!text(decision.reason, 1024) || !Array.isArray(decision.basis) || decision.basis.length === 0) {
    return fail('NA_BASIS_EVIDENCE_REQUIRED')
  }
  if (!unique(decision.basis.map(item => item?.path))) {
    return fail('DUPLICATE_NA_BASIS_EVIDENCE')
  }
  for (const reference of decision.basis) {
    const checked = verifyFileRef(evidenceRoot, reference)
    if (checked.status !== 'PASS') return checked
  }
  return { status: 'PASS' }
}

function validatePlan(plan, catalogState, candidateState, evidenceRoot) {
  if (
    !isObject(plan)
    || plan.schemaVersion !== 1
    || !SHA256.test(plan.candidateId ?? '')
    || plan.candidateId.toLowerCase() !== candidateState.candidateId
    || !Array.isArray(plan.targets)
    || plan.targets.length === 0
    || plan.targets.length > 64
  ) {
    return fail('INVALID_TARGET_PLAN')
  }

  const targetIds = new Set()
  const targets = new Map()
  for (const target of plan.targets) {
    if (
      !isObject(target)
      || !TOKEN.test(target.targetId ?? '')
      || !TOKEN.test(target.artifactName ?? '')
      || !candidateState.artifacts.has(target.artifactName)
      || !isObject(target.cli)
      || !['claude', 'codex'].includes(target.cli.kind)
      || !text(target.cli.version, 256)
      || !SHA256.test(target.cli.binarySha256 ?? '')
      || !Array.isArray(target.decisions)
    ) {
      return fail('INVALID_TARGET_PLAN')
    }
    if (targetIds.has(target.targetId)) return fail('DUPLICATE_TARGET_ID')
    targetIds.add(target.targetId)

    const decisions = new Map()
    for (const decision of target.decisions) {
      if (
        !isObject(decision)
        || !CASE_ID.test(decision.caseId ?? '')
        || !catalogState.byId.has(decision.caseId)
        || !['REQUIRED', 'N_A'].includes(decision.status)
      ) {
        return fail('INVALID_CASE_DECISION')
      }
      if (decisions.has(decision.caseId)) return fail('DUPLICATE_CASE_DECISION')

      const catalogCase = catalogState.byId.get(decision.caseId)
      if (decision.status === 'REQUIRED') {
        if (
          !Array.isArray(decision.layers)
          || decision.layers.length === 0
          || decision.layers.some(layer => !LAYERS.has(layer))
          || !unique(decision.layers)
          || decision.reason !== undefined
          || decision.basis !== undefined
        ) {
          return fail('INVALID_REQUIRED_CASE_DECISION')
        }
        decisions.set(decision.caseId, {
          status: 'REQUIRED',
          layers: [...decision.layers],
        })
      } else {
        if (!catalogCase.allowNA) return fail('NA_NOT_ALLOWED')
        const basis = validateNaBasis(decision, evidenceRoot)
        if (basis.status !== 'PASS') return basis
        if (decision.layers !== undefined) return fail('INVALID_NA_CASE_DECISION')
        decisions.set(decision.caseId, {
          status: 'N_A',
          reason: decision.reason,
        })
      }
    }

    if (decisions.size !== catalogState.byId.size) {
      return fail('TARGET_CASE_PLAN_INCOMPLETE')
    }
    for (const id of catalogState.byId.keys()) {
      if (!decisions.has(id)) return fail('TARGET_CASE_PLAN_INCOMPLETE')
    }

    targets.set(target.targetId, {
      targetId: target.targetId,
      artifact: candidateState.artifacts.get(target.artifactName),
      cli: {
        kind: target.cli.kind,
        version: target.cli.version,
        binarySha256: target.cli.binarySha256.toLowerCase(),
      },
      decisions,
    })
  }

  return { status: 'PASS', targets }
}

function requiredKeys(catalogState, planState) {
  const required = new Map()
  for (const target of planState.targets.values()) {
    for (const [id, decision] of target.decisions) {
      if (decision.status !== 'REQUIRED') continue
      const catalogCase = catalogState.byId.get(id)
      const subcases = catalogCase.requiredSubcaseIds.length > 0
        ? catalogCase.requiredSubcaseIds
        : [null]
      for (const subcaseId of subcases) {
        for (const layer of decision.layers) {
          required.set(
            evidenceKey(target.targetId, id, subcaseId, layer),
            {
              target,
              caseId: id,
              subcaseId,
              layer,
            },
          )
        }
      }
    }
  }
  return required
}

function validatePassRecord(record, requirement, candidateState, catalogState, evidenceRoot) {
  if (
    record.schemaVersion !== 2
    || record.specVersion !== catalogState.specVersion
    || !text(record.runId, 256)
    || record.targetId !== requirement.target.targetId
    || record.candidateId?.toLowerCase() !== candidateState.candidateId
    || record.caseId !== requirement.caseId
    || record.subcaseId !== requirement.subcaseId
    || record.evidenceLayer !== requirement.layer
    || !SHA256.test(record.cliBinarySha256 ?? '')
    || !SHA256.test(record.deskPackageSha256 ?? '')
    || !Array.isArray(record.evidence)
    || record.evidence.length === 0
    || record.evidence.length > 64
  ) {
    return fail('INVALID_PASS_EVIDENCE')
  }

  if (
    record.cliBinarySha256.toLowerCase()
    !== requirement.target.cli.binarySha256
  ) {
    return fail('CLI_BINARY_HASH_MISMATCH')
  }
  if (
    record.deskPackageSha256.toLowerCase()
    !== requirement.target.artifact.sha256
  ) {
    return fail('DESK_PACKAGE_HASH_MISMATCH')
  }

  const evidencePaths = record.evidence.map(item => item?.path)
  if (!unique(evidencePaths)) return fail('DUPLICATE_EVIDENCE_FILE')
  for (const reference of record.evidence) {
    const checked = verifyFileRef(evidenceRoot, reference)
    if (checked.status !== 'PASS') return checked
  }
  return { status: 'PASS' }
}

function validateRecords(records, required, candidateState, catalogState, evidenceRoot) {
  if (!Array.isArray(records) || records.length > 100000) {
    return fail('INVALID_EVIDENCE_RECORDS')
  }

  const seen = new Set()
  for (const record of records) {
    if (
      !isObject(record)
      || !TOKEN.test(record.targetId ?? '')
      || typeof record.caseId !== 'string'
      || (record.subcaseId !== null && !SUBCASE.test(record.subcaseId ?? ''))
      || !LAYERS.has(record.evidenceLayer)
    ) {
      return fail('INVALID_EVIDENCE_RECORD')
    }

    const key = evidenceKey(
      record.targetId,
      record.caseId,
      record.subcaseId,
      record.evidenceLayer,
    )
    if (!required.has(key)) return fail('UNPLANNED_EVIDENCE_RECORD')
    if (seen.has(key)) return fail('DUPLICATE_EVIDENCE_RECORD')
    seen.add(key)

    if (!REQUIRED_STATUSES.has(record.status)) {
      return fail('INVALID_EVIDENCE_STATUS')
    }
    if (record.status !== 'PASS') {
      return fail(`REQUIRED_EVIDENCE_${record.status}`)
    }

    const checked = validatePassRecord(
      record,
      required.get(key),
      candidateState,
      catalogState,
      evidenceRoot,
    )
    if (checked.status !== 'PASS') return checked
  }

  if (seen.size !== required.size) return fail('REQUIRED_EVIDENCE_MISSING')
  for (const key of required.keys()) {
    if (!seen.has(key)) return fail('REQUIRED_EVIDENCE_MISSING')
  }

  return { status: 'PASS' }
}

export function verifyAcceptance(input) {
  try {
    const catalogState = validateCatalog(input?.catalog)
    if (catalogState.status !== 'PASS') return catalogState

    const candidateState = validateCandidate(
      input?.candidate,
      input?.artifactRoot,
    )
    if (candidateState.status !== 'PASS') return candidateState

    const planState = validatePlan(
      input?.plan,
      catalogState,
      candidateState,
      input?.evidenceRoot,
    )
    if (planState.status !== 'PASS') return planState

    const required = requiredKeys(catalogState, planState)
    if (required.size === 0) return fail('NO_REQUIRED_EVIDENCE')

    const recordsState = validateRecords(
      input?.records,
      required,
      candidateState,
      catalogState,
      input?.evidenceRoot,
    )
    if (recordsState.status !== 'PASS') return recordsState

    return {
      status: 'PASS',
      candidateId: candidateState.candidateId,
      targetIds: [...planState.targets.keys()].sort(),
    }
  } catch {
    return fail('ACCEPTANCE_GATE_INTERNAL_ERROR')
  }
}

function loadJson(path) {
  let stat
  try {
    stat = statSync(path)
  } catch {
    throw new Error('INPUT_FILE_UNAVAILABLE')
  }
  if (!stat.isFile() || stat.size <= 0 || stat.size > MAX_JSON_BYTES) {
    throw new Error('INPUT_FILE_SIZE_INVALID')
  }
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    throw new Error('INVALID_INPUT_JSON')
  }
}

function parseArgs(argv) {
  const values = new Map()
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index]
    if (![
      '--catalog',
      '--candidate',
      '--plan',
      '--records',
      '--artifact-root',
      '--evidence-root',
    ].includes(option)) {
      throw new Error('UNKNOWN_OPTION')
    }
    if (values.has(option) || index + 1 >= argv.length) {
      throw new Error('INVALID_OPTION')
    }
    values.set(option, argv[++index])
  }

  for (const option of [
    '--catalog',
    '--candidate',
    '--plan',
    '--records',
    '--artifact-root',
    '--evidence-root',
  ]) {
    if (!values.has(option)) throw new Error('REQUIRED_OPTION_MISSING')
  }
  return Object.fromEntries(values)
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  const recordsFile = loadJson(resolve(args['--records']))
  const records = Array.isArray(recordsFile)
    ? recordsFile
    : recordsFile?.schemaVersion === 1 && Array.isArray(recordsFile.records)
      ? recordsFile.records
      : null

  const result = verifyAcceptance({
    catalog: loadJson(resolve(args['--catalog'])),
    candidate: loadJson(resolve(args['--candidate'])),
    plan: loadJson(resolve(args['--plan'])),
    records,
    artifactRoot: resolve(args['--artifact-root']),
    evidenceRoot: resolve(args['--evidence-root']),
  })
  process.stdout.write(`${JSON.stringify(result)}\n`)
  process.exitCode = result.status === 'PASS' ? 0 : 1
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  main().catch(error => {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'ACCEPTANCE_GATE_FAILED'
    process.stderr.write(`[acceptance-gate-error] ${reason}\n`)
    process.exitCode = 1
  })
}
