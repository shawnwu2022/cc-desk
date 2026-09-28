#!/usr/bin/env node
import fs from 'node:fs'
import { createHash } from 'node:crypto'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { candidateIdFor } from './candidate-manifest.mjs'

const SHA256 = /^[0-9a-f]{64}$/
const COMMIT = /^[0-9a-f]{40}$/
const CASE_ID = /^NATIVE-(?:0[1-9]|[1-5][0-9]|6[0-4])$/
const LAYERS = new Set(['A', 'B', 'C', 'D'])
const CERTIFIED_STATUSES = new Set(['PASS', 'N_A'])
const MAX_EVIDENCE_BYTES = 64 * 1024 * 1024

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
    const filePath = text(file.path, 'INVALID_CANDIDATE_PATH')
    if (filePath.startsWith('/') || filePath.includes('..') || paths.has(filePath)) {
      fail('INVALID_CANDIDATE_PATH')
    }
    paths.add(filePath)
    text(file.kind, 'INVALID_CANDIDATE_KIND')
    const sha = String(file.sha256 ?? '').toLowerCase()
    if (!SHA256.test(sha)) fail('INVALID_CANDIDATE_HASH')
    hashes.add(sha)
    if (!Number.isSafeInteger(file.size) || file.size <= 0) fail('INVALID_CANDIDATE_SIZE')
  }
  if (candidateId !== candidateIdFor(candidate.commitSha, files)) {
    fail('INVALID_CANDIDATE_IDENTITY')
  }
  return { candidateId, hashes }
}

function requirementKey(targetId, caseId, subcaseId) {
  return JSON.stringify([targetId, caseId, subcaseId ?? null])
}

function normalizeTargets(rawTargets) {
  const targets = array(rawTargets, 'INVALID_TARGETS')
  if (targets.length === 0) fail('TARGETS_REQUIRED')
  const targetIds = new Set()
  const normalized = []
  const requirements = new Map()

  for (const rawTarget of targets) {
    const target = object(rawTarget, 'INVALID_TARGET')
    const targetId = text(target.targetId, 'INVALID_TARGET_ID')
    if (targetIds.has(targetId)) fail('DUPLICATE_TARGET')
    targetIds.add(targetId)

    const required = array(target.required, 'INVALID_REQUIRED_CASES')
    if (required.length === 0) fail('REQUIRED_CASES_EMPTY')
    const normalizedRequired = []

    for (const rawReq of required) {
      const req = object(rawReq, 'INVALID_REQUIRED_CASE')
      const caseId = text(req.caseId, 'INVALID_CASE_ID')
      if (!CASE_ID.test(caseId)) fail('INVALID_CASE_ID')
      const layers = array(req.evidenceLayers, 'REQUIRED_EVIDENCE_LAYERS')
      if (layers.length === 0) fail('REQUIRED_EVIDENCE_LAYERS')
      const layerSet = new Set()
      for (const layer of layers) {
        if (!LAYERS.has(layer) || layerSet.has(layer)) fail('INVALID_REQUIRED_EVIDENCE_LAYER')
        layerSet.add(layer)
      }

      const subcases = req.subcaseIds === undefined
        ? [null]
        : array(req.subcaseIds, 'INVALID_SUBCASES')
      if (subcases.length === 0) fail('INVALID_SUBCASES')
      const local = new Set()
      const normalizedSubcases = []
      for (const rawSubcase of subcases) {
        const subcaseId = rawSubcase === null ? null : text(rawSubcase, 'INVALID_SUBCASE_ID')
        if (local.has(subcaseId)) fail('DUPLICATE_REQUIREMENT')
        local.add(subcaseId)
        normalizedSubcases.push(subcaseId)
        const key = requirementKey(targetId, caseId, subcaseId)
        if (requirements.has(key)) fail('DUPLICATE_REQUIREMENT')
        requirements.set(key, { layers: layerSet, covered: false })
      }

      normalizedRequired.push({
        caseId,
        evidenceLayers: [...layerSet].sort(),
        subcaseIds: normalizedSubcases,
      })
    }

    normalized.push({
      targetId,
      required: normalizedRequired.sort((a, b) => a.caseId.localeCompare(b.caseId)),
    })
  }

  normalized.sort((a, b) => a.targetId.localeCompare(b.targetId))
  return { normalized, requirements }
}

function verifyTargetPlan(value) {
  const plan = object(value, 'INVALID_TARGET_PLAN')
  if (plan.schemaVersion !== 1) fail('INVALID_TARGET_PLAN_SCHEMA')
  if (plan.status !== 'READY') fail('RELEASE_TARGETS_NOT_READY')
  return normalizeTargets(plan.targets)
}

function verifyEvidenceItems(value, evidenceRoot = null) {
  const items = array(value, 'EVIDENCE_REQUIRED')
  if (items.length === 0) fail('EVIDENCE_REQUIRED')
  const seen = new Set()
  for (const raw of items) {
    const item = object(raw, 'INVALID_EVIDENCE')
    const kind = text(item.kind, 'INVALID_EVIDENCE_KIND')
    const evidencePath = text(item.path, 'INVALID_EVIDENCE_PATH')
    if (
      path.isAbsolute(evidencePath)
      || evidencePath.split(/[\\/]/).includes('..')
      || evidencePath.includes('\0')
    ) {
      fail('INVALID_EVIDENCE_PATH')
    }
    const sha = String(item.sha256 ?? '').toLowerCase()
    if (!SHA256.test(sha)) fail('INVALID_EVIDENCE_HASH')
    const identity = JSON.stringify([kind, evidencePath, sha])
    if (seen.has(identity)) fail('DUPLICATE_EVIDENCE')
    seen.add(identity)

    if (evidenceRoot !== null) {
      const root = path.resolve(evidenceRoot)
      const file = path.resolve(root, evidencePath)
      const prefix = root.endsWith(path.sep) ? root : root + path.sep
      if (!file.startsWith(prefix)) fail('INVALID_EVIDENCE_PATH')
      let metadata
      try {
        metadata = fs.lstatSync(file)
      } catch {
        fail('EVIDENCE_FILE_MISSING')
      }
      if (!metadata.isFile() || metadata.isSymbolicLink()) fail('UNSAFE_EVIDENCE_FILE')
      const realRoot = fs.realpathSync(root)
      const realFile = fs.realpathSync(file)
      const realPrefix = realRoot.endsWith(path.sep) ? realRoot : realRoot + path.sep
      if (!realFile.startsWith(realPrefix)) fail('UNSAFE_EVIDENCE_FILE')
      if (metadata.size <= 0 || metadata.size > MAX_EVIDENCE_BYTES) fail('EVIDENCE_FILE_SIZE_INVALID')
      const actual = createHash('sha256').update(fs.readFileSync(realFile)).digest('hex')
      if (actual !== sha) fail('EVIDENCE_FILE_HASH_MISMATCH')
    }
  }
}

export function verifyAcceptance(manifest, expectedCandidate = null, expectedTargetPlan = null, options = {}) {
  const root = object(manifest, 'INVALID_ACCEPTANCE_MANIFEST')
  if (root.schemaVersion !== 1) fail('UNSUPPORTED_ACCEPTANCE_SCHEMA')
  const candidate = verifyCandidate(root.candidate)

  if (expectedCandidate !== null) {
    const expected = verifyCandidate(expectedCandidate)
    if (expected.candidateId !== candidate.candidateId) fail('CANDIDATE_REFERENCE_MISMATCH')
  }

  const declared = normalizeTargets(root.targets)
  const authoritative = expectedTargetPlan === null
    ? declared
    : verifyTargetPlan(expectedTargetPlan)
  if (JSON.stringify(declared.normalized) !== JSON.stringify(authoritative.normalized)) {
    fail('TARGET_PLAN_MISMATCH')
  }
  const requirements = authoritative.requirements

  const records = array(root.records, 'INVALID_RECORDS')
  const recordKeys = new Set()
  for (const rawRecord of records) {
    const record = object(rawRecord, 'INVALID_RECORD')
    const targetId = text(record.targetId, 'INVALID_TARGET_ID')
    const caseId = text(record.caseId, 'INVALID_CASE_ID')
    const subcaseId = record.subcaseId === null || record.subcaseId === undefined
      ? null
      : text(record.subcaseId, 'INVALID_SUBCASE_ID')
    const key = requirementKey(targetId, caseId, subcaseId)
    const requirement = requirements.get(key)
    if (!requirement) fail('UNDECLARED_ACCEPTANCE_RECORD')
    if (recordKeys.has(key)) fail('DUPLICATE_ACCEPTANCE_RECORD')
    recordKeys.add(key)

    if (record.candidateId !== candidate.candidateId) fail('CANDIDATE_ID_MISMATCH')
    if (!CERTIFIED_STATUSES.has(record.status)) fail('UNCERTIFIED_STATUS')
    if (!LAYERS.has(record.evidenceLayer)) fail('INVALID_EVIDENCE_LAYER')
    if (!requirement.layers.has(record.evidenceLayer)) fail('EVIDENCE_LAYER_TOO_WEAK')
    verifyEvidenceItems(record.evidence, options.evidenceRoot ?? null)

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
    requirement.covered = true
  }

  if ([...requirements.values()].some(requirement => !requirement.covered)) {
    fail('MISSING_REQUIRED_ACCEPTANCE')
  }

  return Object.freeze({
    ok: true,
    schemaVersion: 1,
    candidateId: candidate.candidateId,
    targetCount: declared.normalized.length,
    recordCount: records.length,
  })
}

function main() {
  const manifestPath = process.argv[2]
  const candidatePath = process.argv[3]
  const targetPlanPath = process.argv[4]
  const evidenceRoot = process.argv[5]
  if (!manifestPath) fail('ACCEPTANCE_MANIFEST_PATH_REQUIRED')
  if (!candidatePath) fail('ACCEPTANCE_CANDIDATE_PATH_REQUIRED')
  if (!targetPlanPath) fail('ACCEPTANCE_TARGET_PLAN_PATH_REQUIRED')
  if (!evidenceRoot) fail('ACCEPTANCE_EVIDENCE_ROOT_REQUIRED')
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  const expectedCandidate = JSON.parse(fs.readFileSync(candidatePath, 'utf8'))
  const targetPlan = JSON.parse(fs.readFileSync(targetPlanPath, 'utf8'))
  const result = verifyAcceptance(manifest, expectedCandidate, targetPlan, { evidenceRoot })
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
