#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { verifyAcceptance } from './verify-acceptance.mjs'
import { candidateIdFor } from './candidate-manifest.mjs'

const SHA256 = /^[0-9a-f]{64}$/

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex')
}

export function verifyPromotion({ candidate, acceptance, artifactsRoot, expectedCommitSha }) {
  if (!candidate || candidate.schemaVersion !== 1) fail('INVALID_CANDIDATE_MANIFEST')
  if (candidate.commitSha !== expectedCommitSha) fail('CANDIDATE_COMMIT_MISMATCH')
  if (!Array.isArray(candidate.files) || candidate.files.length === 0) fail('INVALID_CANDIDATE_MANIFEST')
  if (candidate.candidateId !== candidateIdFor(candidate.commitSha, candidate.files)) {
    fail('INVALID_CANDIDATE_IDENTITY')
  }
  if (acceptance?.candidate?.candidateId !== candidate.candidateId) fail('ACCEPTANCE_CANDIDATE_MISMATCH')

  const acceptanceResult = verifyAcceptance(acceptance)
  if (!acceptanceResult.ok) fail('ACCEPTANCE_GATE_FAILED')

  const files = []
  for (const entry of candidate.files ?? []) {
    if (!SHA256.test(String(entry.sha256))) fail('INVALID_CANDIDATE_HASH')
    const file = path.resolve(artifactsRoot, entry.path)
    const root = path.resolve(artifactsRoot) + path.sep
    if (!file.startsWith(root) || !existsSync(file)) fail('CANDIDATE_FILE_MISSING')
    if (statSync(file).size !== entry.size) fail('CANDIDATE_FILE_SIZE_MISMATCH')
    if (sha256(file) !== entry.sha256) fail('CANDIDATE_FILE_HASH_MISMATCH')
    files.push(file)
  }
  if (files.length === 0) fail('CANDIDATE_FILES_EMPTY')

  return Object.freeze({
    ok: true,
    candidateId: candidate.candidateId,
    commitSha: candidate.commitSha,
    files,
  })
}

function main() {
  const candidatePath = path.resolve(process.argv[2] || '')
  const acceptancePath = path.resolve(process.argv[3] || '')
  const artifactsRoot = path.resolve(process.argv[4] || '')
  const expectedCommitSha = process.argv[5] || ''
  if (!candidatePath || !acceptancePath || !artifactsRoot || !expectedCommitSha) fail('PROMOTION_ARGUMENTS_REQUIRED')
  const result = verifyPromotion({
    candidate: JSON.parse(readFileSync(candidatePath, 'utf8')),
    acceptance: JSON.parse(readFileSync(acceptancePath, 'utf8')),
    artifactsRoot,
    expectedCommitSha,
  })
  process.stdout.write(JSON.stringify({
    ok: true,
    candidateId: result.candidateId,
    commitSha: result.commitSha,
    files: result.files.map(file => path.relative(artifactsRoot, file).split(path.sep).join('/')),
  }) + '\n')
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code || 'PROMOTION_VERIFY_FAILED') + '\n')
    process.exit(1)
  }
}
