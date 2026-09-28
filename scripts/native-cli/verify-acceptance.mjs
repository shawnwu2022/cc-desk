#!/usr/bin/env node
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { candidateIdFor } from './candidate-manifest.mjs'

const SHA256 = /^[0-9a-f]{64}$/
const COMMIT = /^[0-9a-f]{40}$/
const CASE_ID = /^NATIVE-(?:0[1-9]|[1-5][0-9]|6[0-4])$/
const LAYERS = new Set(['A', 'B', 'C', 'D'])
const CERTIFIED_STATUSES = new Set(['PASS', 'N_A'])

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

function object(value, code) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail(code)
  return value
}

function text(value, code) {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0')) fail(code)
  return value
}

function array(value, code) {
  if (!Array.isArray(value)) fail(code)
  return value
}

function verifyCandidate(value) {
  const candidate = object(value, 'INVALID_CANDIDATE')
  if (candidate.schemaVersion !== 1) fail('INVALID_CANDIDATE_SCHEMA')
  const candidateId = text(candidate.candidateId, 'INVALID_CANDIDATE_ID')
  if (!COMMIT.test(String(candidate.commitSha))) fail('INVALID_CANDIDATE_COMMIT')
  const files = array(candidate.files, 'INVALID_CANDIDATE_FILES')
  if (files.length === 0) fail('EMPTY_CANDIDATE_FILES')
  const paths = new Set()
  const hashes = new Set()
  for (const raw of files) {
    const file = object(raw, 'INVALID_CANDIDATE_FILE')
    const path = text(file.path, 'INVALID_CANDIDATE_PATH')
    if (path.startsWith('/') || path.includes('..') || paths.has(path)) fail('INVALID_CANDIDATE_PATH')
    paths.add(path)
    text(file.kind, 'INVALID_CANDIDATE_KIND')
    const sha = String(file.sha256 ?? '').toLowerCase()
    if (!SHA256.test(sha)) fail('INVALID_CANDIDATE_HASH')
    hashes.add(sha)
    if (!Number.isSafeInteger(file.size) || file.size <= 0) fail('INVALID_CANDIDATE_SIZE')
  }
  const expectedId = candidateIdFor(candidate.commitSha, files)
  if (candidateId !== expectedId) fail('INVALID_CANDIDATE_IDENTITY')
  return { candidateId, hashes }
}

function key(targetId, caseId, subcaseId) {
  return JSON.stringify([targetId, caseId, subcaseId ?? null])
}

function verifyEvidenceItems(value) {
  const items = array(value, 'EVIDENCE_REQUIRED')
  if (items.length === 0) fail('EVIDENCE_REQUIRED')
  const seen = new Set()
  for (const raw of items) {
    const item = object(raw, 'INVALID_EVIDENCE')
    const kind = text(item.kind, 'INVALID_EVIDENCE_KIND')
    const path = text(item.path, 'INVALID_EVIDENCE_PATH')
    const sha = String(item.sha256 ?? '').toLowerCase()
    if (!SHA256.test(sha)) fail('INVALID_EVIDENCE_HASH')
    const identity = JSON.stringify([kind, path, sha])
    if (seen.has(identity)) fail('DUPLICATE_EVIDENCE')
    seen.add(identity)
  }
}

export function verifyAcceptance(manifest, expectedCandidate = null) {
  const root = object(manifest, 'INVALID_ACCEPTANCE_MANIFEST')
  if (root.schemaVersion !== 1) fail('UNSUPPORTED_ACCEPTANCE_SCHEMA')
  const candidate = verifyCandidate(root.candidate)
  if (expectedCandidate !== null) {
    const expected = verifyCandidate(expectedCandidate)
    if (expected.candidateId !== candidate.candidateId) fail('CANDIDATE_REFERENCE_MISMATCH')
  }

  const requirements = new Map()
  const targets = array(root.targets, 'INVALID_TARGETS')
  if (targets.length === 0) fail('TARGETS_REQUIRED')
  const targetIds = new Set()

  for (const rawTarget of targets) {
    const target = object(rawTarget, 'INVALID_TARGET')
    const targetId = text(target.targetId, 'INVALID_TARGET_ID')
    if (targetIds.has(targetId)) fail('DUPLICATE_TARGET')
    targetIds.add(targetId)
    const required = array(target.required, 'INVALID_REQUIRED_CASES')
    if (required.length === 0) fail('REQUIRED_CASES_EMPTY')

    for (const rawReq of required) {
      const req = object(rawReq, 'INVALID_REQUIRED_CASE')
      const caseId = text(req.caseId, 'INVALID_CASE_ID')
      if (!CASE_ID.test(caseId)) fail('INVALID_CASE_ID')
      const subcases = req.subcaseIds === undefined ? [null] : array(req.subcaseIds, 'INVALID_SUBCASES')
      if (subcases.length === 0) fail('INVALID_SUBCASES')
      const local = new Set()
      for (const rawSubcase of subcases) {
        const subcaseId = rawSubcase === null ? null : text(rawSubcase, 'INVALID_SUBCASE_ID')
        if (local.has(subcaseId)) fail('DUPLICATE_REQUIREMENT')
        local.add(subcaseId)
        const requirementKey = key(targetId, caseId, subcaseId)
        if (requirements.has(requirementKey)) fail('DUPLICATE_REQUIREMENT')
        requirements.set(requirementKey, false)
      }
    }
  }

  const records = array(root.records, 'INVALID_RECORDS')
  const recordKeys = new Set()
  for (const rawRecord of records) {
    const record = object(rawRecord, 'INVALID_RECORD')
    const targetId = text(record.targetId, 'INVALID_TARGET_ID')
    const caseId = text(record.caseId, 'INVALID_CASE_ID')
    const subcaseId = record.subcaseId === null || record.subcaseId === undefined
      ? null
      : text(record.subcaseId, 'INVALID_SUBCASE_ID')
    const recordKey = key(targetId, caseId, subcaseId)
    if (!requirements.has(recordKey)) fail('UNDECLARED_ACCEPTANCE_RECORD')
    if (recordKeys.has(recordKey)) fail('DUPLICATE_ACCEPTANCE_RECORD')
    recordKeys.add(recordKey)

    if (record.candidateId !== candidate.candidateId) fail('CANDIDATE_ID_MISMATCH')
    if (!CERTIFIED_STATUSES.has(record.status)) fail('UNCERTIFIED_STATUS')
    if (!LAYERS.has(record.evidenceLayer)) fail('INVALID_EVIDENCE_LAYER')
    verifyEvidenceItems(record.evidence)

    if (record.status === 'N_A') {
      text(record.nonApplicabilityReason, 'N_A_REASON_REQUIRED')
    } else if (record.nonApplicabilityReason !== null && record.nonApplicabilityReason !== undefined) {
      fail('PASS_HAS_N_A_REASON')
    }

    if (record.evidenceLayer === 'D') {
      const packageSha256 = String(record.packageSha256 ?? '').toLowerCase()
      if (!SHA256.test(packageSha256) || !candidate.hashes.has(packageSha256)) {
        fail('CANDIDATE_PACKAGE_HASH_MISMATCH')
      }
    }
    requirements.set(recordKey, true)
  }

  const missing = [...requirements.entries()].filter(([, covered]) => !covered)
  if (missing.length !== 0) fail('MISSING_REQUIRED_ACCEPTANCE')

  return Object.freeze({
    ok: true,
    schemaVersion: 1,
    candidateId: candidate.candidateId,
    targetCount: targets.length,
    recordCount: records.length,
  })
}

function main() {
  const manifestPath = process.argv[2]
  const candidatePath = process.argv[3]
  if (!manifestPath) fail('ACCEPTANCE_MANIFEST_PATH_REQUIRED')
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  const expectedCandidate = candidatePath
    ? JSON.parse(fs.readFileSync(candidatePath, 'utf8'))
    : null
  const result = verifyAcceptance(manifest, expectedCandidate)
  process.stdout.write(JSON.stringify(result) + '\n')
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code || 'ACCEPTANCE_GATE_FAILED') + '\n')
    process.exit(1)
  }
}
