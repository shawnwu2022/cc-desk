import { createHash } from 'node:crypto'
import {
  lstatSync,
  readFileSync,
  realpathSync,
  statSync,
} from 'node:fs'
import {
  isAbsolute,
  relative,
  resolve,
  sep,
} from 'node:path'

const SHA256 = /^[0-9a-f]{64}$/
const COMMIT = /^[0-9a-f]{40}$/
const CASE_ID = /^NATIVE-(?:0[1-9]|[1-5][0-9]|6[0-4])$/
const LAYERS = new Set(['A', 'B', 'C', 'D'])
const MAX_EVIDENCE_BYTES = 64 * 1024 * 1024
const MAX_TEXT_BYTES = 16 * 1024 * 1024

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function fail(reason, detail = null) {
  return {
    status: 'FAIL',
    reason,
    ...(detail === null ? {} : { detail }),
  }
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical)
  if (isObject(value)) {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map(key => [key, canonical(value[key])]),
    )
  }
  return value
}

export function sha256Json(value) {
  return createHash('sha256')
    .update(JSON.stringify(canonical(value)), 'utf8')
    .digest('hex')
}

export function candidateIdFor(candidate) {
  if (!isObject(candidate)) return null
  const files = Array.isArray(candidate.files)
    ? [...candidate.files]
      .map(file => ({
        path: file?.path,
        platform: file?.platform,
        arch: file?.arch,
        sha256: file?.sha256,
      }))
      .sort((a, b) => String(a.path).localeCompare(String(b.path)))
    : []
  return sha256Json({
    schemaVersion: candidate.schemaVersion,
    sourceCommit: candidate.sourceCommit,
    files,
  })
}

function safeRelativePath(value) {
  return (
    typeof value === 'string'
    && value.length > 0
    && !value.includes('\0')
    && !isAbsolute(value)
    && !/^[A-Za-z]:[\\/]/.test(value)
    && !value.split(/[\\/]+/).includes('..')
  )
}

function catalogMap(catalog) {
  if (!isObject(catalog) || catalog.schemaVersion !== 2) {
    throw new Error('CATALOG_SCHEMA_INVALID')
  }
  if (!Array.isArray(catalog.cases) || catalog.cases.length !== 64 || catalog.caseCount !== 64) {
    throw new Error('CATALOG_CASE_COUNT_INVALID')
  }

  const map = new Map()
  for (const item of catalog.cases) {
    if (
      !isObject(item)
      || !CASE_ID.test(item.caseId ?? '')
      || typeof item.owner !== 'string'
      || item.owner.length === 0
      || item.requiredByDefault !== true
      || !Array.isArray(item.subcaseIds)
      || item.subcaseIds.length === 0
      || item.subcaseIds.some(
        value => typeof value !== 'string' || value.length === 0 || value.includes('\0'),
      )
      || new Set(item.subcaseIds).size !== item.subcaseIds.length
    ) {
      throw new Error('CATALOG_CASE_INVALID')
    }
    if (map.has(item.caseId)) throw new Error('CATALOG_CASE_DUPLICATE')
    map.set(item.caseId, item)
  }

  for (let index = 1; index <= 64; index += 1) {
    const id = `NATIVE-${String(index).padStart(2, '0')}`
    if (!map.has(id)) throw new Error('CATALOG_CASE_MISSING')
  }
  return map
}

function validateCandidate(candidate) {
  if (
    !isObject(candidate)
    || candidate.schemaVersion !== 1
    || !COMMIT.test(candidate.sourceCommit ?? '')
    || !Array.isArray(candidate.files)
    || candidate.files.length === 0
  ) {
    return fail('CANDIDATE_INVALID')
  }

  const paths = new Set()
  for (const file of candidate.files) {
    if (
      !isObject(file)
      || !safeRelativePath(file.path)
      || typeof file.platform !== 'string'
      || file.platform.length === 0
      || typeof file.arch !== 'string'
      || file.arch.length === 0
      || !SHA256.test(file.sha256 ?? '')
      || paths.has(file.path)
    ) {
      return fail('CANDIDATE_FILE_INVALID')
    }
    paths.add(file.path)
  }

  const computed = candidateIdFor(candidate)
  if (!SHA256.test(candidate.candidateId ?? '') || candidate.candidateId !== computed) {
    return fail('CANDIDATE_ID_MISMATCH')
  }

  return { status: 'PASS' }
}

function targetIdentityValid(target) {
  return (
    isObject(target)
    && typeof target.targetId === 'string'
    && target.targetId.length > 0
    && typeof target.os === 'string'
    && target.os.length > 0
    && typeof target.osBuild === 'string'
    && target.osBuild.length > 0
    && typeof target.arch === 'string'
    && target.arch.length > 0
    && target.executionDomain === 'local'
    && isObject(target.cli)
    && ['claude', 'codex'].includes(target.cli.kind)
    && typeof target.cli.version === 'string'
    && target.cli.version.length > 0
    && SHA256.test(target.cli.binarySha256 ?? '')
    && safeRelativePath(target.candidateFilePath)
  )
}

function validatePlan(plan, catalog, candidate) {
  if (
    !isObject(plan)
    || plan.schemaVersion !== 1
    || plan.claim !== 'installed-native-release'
    || plan.candidateId !== candidate.candidateId
    || !Array.isArray(plan.targets)
    || plan.targets.length === 0
  ) {
    return fail('TARGET_PLAN_INVALID')
  }

  const caseIds = [...catalog.keys()].sort()
  const targetIds = new Set()
  for (const target of plan.targets) {
    if (!targetIdentityValid(target) || targetIds.has(target.targetId)) {
      return fail('TARGET_IDENTITY_INVALID')
    }
    targetIds.add(target.targetId)

    const candidateFile = candidate.files.find(file => file.path === target.candidateFilePath)
    if (
      !candidateFile
      || candidateFile.arch !== target.arch
      || candidateFile.platform !== target.os
      || !SHA256.test(target.packageSha256 ?? '')
      || candidateFile.sha256 !== target.packageSha256
    ) {
      return fail('TARGET_PACKAGE_IDENTITY_MISMATCH', target.targetId)
    }

    if (
      !Array.isArray(target.requiredCaseIds)
      || target.requiredCaseIds.length !== 64
      || new Set(target.requiredCaseIds).size !== 64
      || [...target.requiredCaseIds].sort().join('\n') !== caseIds.join('\n')
    ) {
      return fail('REQUIRED_CASE_SET_INCOMPLETE', target.targetId)
    }
  }

  return { status: 'PASS' }
}

function recordKey(record) {
  return `${record.targetId}\0${record.caseId}\0${record.subcaseId}`
}

function validateRecordShape(record) {
  return (
    isObject(record)
    && record.schemaVersion === 2
    && record.specVersion === 2
    && typeof record.targetId === 'string'
    && CASE_ID.test(record.caseId ?? '')
    && typeof record.subcaseId === 'string'
    && record.subcaseId.length > 0
    && typeof record.runId === 'string'
    && record.runId.length > 0
    && ['PASS', 'N_A', 'FAIL', 'BLOCKED', 'NOT_RUN'].includes(record.status)
    && LAYERS.has(record.evidenceLayer)
    && SHA256.test(record.cliBinarySha256 ?? '')
    && SHA256.test(record.deskPackageSha256 ?? '')
    && Array.isArray(record.evidence)
  )
}

function evidencePath(root, relativePath) {
  if (!safeRelativePath(relativePath)) throw new Error('EVIDENCE_PATH_INVALID')
  const rootReal = realpathSync(root)
  const full = resolve(rootReal, relativePath)
  const info = lstatSync(full)
  if (!info.isFile() || info.isSymbolicLink() || info.size > MAX_EVIDENCE_BYTES) {
    throw new Error('EVIDENCE_FILE_INVALID')
  }
  const real = realpathSync(full)
  const rel = relative(rootReal, real)
  if (rel === '' || rel.startsWith(`..${sep}`) || rel === '..' || isAbsolute(rel)) {
    throw new Error('EVIDENCE_PATH_ESCAPE')
  }
  return real
}

function hashFile(path) {
  const bytes = readFileSync(path)
  return createHash('sha256').update(bytes).digest('hex')
}

function verifyEvidenceFiles(record, evidenceRoot) {
  if (record.evidence.length === 0) return fail('EVIDENCE_REQUIRED', recordKey(record))
  const seen = new Set()
  for (const entry of record.evidence) {
    if (
      !isObject(entry)
      || !safeRelativePath(entry.path)
      || !SHA256.test(entry.sha256 ?? '')
      || seen.has(entry.path)
    ) {
      return fail('EVIDENCE_REFERENCE_INVALID', recordKey(record))
    }
    seen.add(entry.path)
    let file
    try {
      file = evidencePath(evidenceRoot, entry.path)
    } catch {
      return fail('EVIDENCE_FILE_INVALID', recordKey(record))
    }
    if (hashFile(file) !== entry.sha256) {
      return fail('EVIDENCE_HASH_MISMATCH', recordKey(record))
    }
  }
  return { status: 'PASS' }
}

function verifyRecord(record, target, catalogCase, evidenceRoot) {
  if (!validateRecordShape(record)) return fail('EVIDENCE_RECORD_INVALID')

  if (
    record.cliBinarySha256 !== target.cli.binarySha256
    || record.deskPackageSha256 !== target.packageSha256
    || record.evidenceLayer !== 'D'
  ) {
    return fail('EVIDENCE_IDENTITY_MISMATCH', recordKey(record))
  }

  if (!catalogCase.subcaseIds.includes(record.subcaseId)) {
    return fail('EVIDENCE_SUBCASE_UNKNOWN', recordKey(record))
  }

  if (record.status === 'FAIL' || record.status === 'BLOCKED' || record.status === 'NOT_RUN') {
    return fail('REQUIRED_EVIDENCE_NOT_PASSING', recordKey(record))
  }
  if (record.status === 'N_A') {
    if (
      typeof record.nonApplicabilityReason !== 'string'
      || record.nonApplicabilityReason.length < 8
    ) {
      return fail('N_A_BASIS_REQUIRED', recordKey(record))
    }
  } else if (record.nonApplicabilityReason !== null && record.nonApplicabilityReason !== undefined) {
    return fail('PASS_CANNOT_HAVE_N_A_REASON', recordKey(record))
  }

  return verifyEvidenceFiles(record, evidenceRoot)
}

export function validateAcceptanceInputs({ catalog, plan, candidate }) {
  let catalogCases
  try {
    catalogCases = catalogMap(catalog)
  } catch (error) {
    return fail(error instanceof Error ? error.message : 'CATALOG_INVALID')
  }

  const candidateResult = validateCandidate(candidate)
  if (candidateResult.status !== 'PASS') return candidateResult
  const planResult = validatePlan(plan, catalogCases, candidate)
  if (planResult.status !== 'PASS') return planResult
  return { status: 'PASS', catalogCases }
}

export function verifyAcceptance({
  catalog,
  plan,
  candidate,
  records,
  evidenceRoot,
}) {
  const inputs = validateAcceptanceInputs({ catalog, plan, candidate })
  if (inputs.status !== 'PASS') return inputs
  if (!Array.isArray(records)) return fail('EVIDENCE_RECORDS_INVALID')
  if (typeof evidenceRoot !== 'string' || evidenceRoot.length === 0) {
    return fail('EVIDENCE_ROOT_REQUIRED')
  }

  const targetMap = new Map(plan.targets.map(target => [target.targetId, target]))
  const expected = new Set()
  for (const target of plan.targets) {
    for (const caseId of target.requiredCaseIds) {
      const catalogCase = inputs.catalogCases.get(caseId)
      for (const subcaseId of catalogCase.subcaseIds) {
        expected.add(`${target.targetId}\0${caseId}\0${subcaseId}`)
      }
    }
  }

  const recordsByKey = new Map()
  for (const record of records) {
    if (!validateRecordShape(record)) return fail('EVIDENCE_RECORD_INVALID')
    const target = targetMap.get(record.targetId)
    const catalogCase = inputs.catalogCases.get(record.caseId)
    if (!target || !catalogCase) return fail('UNPLANNED_EVIDENCE_RECORD', recordKey(record))
    const key = recordKey(record)
    if (!expected.has(key)) return fail('UNPLANNED_EVIDENCE_RECORD', key)
    if (recordsByKey.has(key)) return fail('DUPLICATE_EVIDENCE_RECORD', key)
    recordsByKey.set(key, record)
  }

  for (const key of expected) {
    const record = recordsByKey.get(key)
    if (!record) return fail('REQUIRED_EVIDENCE_MISSING', key)
    const target = targetMap.get(record.targetId)
    const catalogCase = inputs.catalogCases.get(record.caseId)
    const result = verifyRecord(record, target, catalogCase, evidenceRoot)
    if (result.status !== 'PASS') return result
  }

  if (recordsByKey.size !== expected.size) return fail('EVIDENCE_SET_MISMATCH')

  return {
    status: 'PASS',
    candidateId: candidate.candidateId,
    targetIds: plan.targets.map(target => target.targetId).sort(),
    recordCount: records.length,
  }
}

export function readJsonFile(path) {
  const size = statSync(path).size
  if (size <= 0 || size > MAX_TEXT_BYTES) throw new Error('JSON_FILE_SIZE_INVALID')
  return JSON.parse(readFileSync(path, 'utf8'))
}
