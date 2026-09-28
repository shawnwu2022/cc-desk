#!/usr/bin/env node

import { createHash } from 'node:crypto'
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { basename, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { verifyCandidateFiles } from './candidate-manifest.mjs'

const SHA256 = /^[0-9a-f]{64}$/
const TAG = /^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function publishedName(path) {
  const name = basename(path).replaceAll(' ', '.')
  if (!name || name === '.' || name === '..' || name.includes('/') || name.includes('\\')) {
    fail('PROMOTION_ASSET_NAME_INVALID')
  }
  return name
}

function ensureEmptyDirectory(path) {
  const output = resolve(path)
  if (existsSync(output)) {
    if (!statSync(output).isDirectory()) fail('PROMOTION_OUTPUT_INVALID')
    if (readdirSync(output).length !== 0) fail('PROMOTION_OUTPUT_NOT_EMPTY')
  } else {
    mkdirSync(output, { recursive: true })
  }
  return output
}

function validateAcceptance(acceptance, candidate) {
  if (!acceptance || acceptance.schemaVersion !== 1 || acceptance.status !== 'PASS'
    || acceptance.candidateId !== candidate.candidateId
    || !SHA256.test(String(acceptance.catalogId ?? ''))
    || !SHA256.test(String(acceptance.planId ?? ''))
    || !Number.isSafeInteger(acceptance.targetCount) || acceptance.targetCount <= 0
    || !Number.isSafeInteger(acceptance.requirementCount) || acceptance.requirementCount <= 0
    || !Number.isSafeInteger(acceptance.recordCount) || acceptance.recordCount <= 0) {
    fail('PROMOTION_ACCEPTANCE_INVALID')
  }
}

export function preparePromotion({
  candidate,
  candidateRoot,
  acceptance,
  expectedCandidateId,
  tag,
  approval,
  outputRoot,
}) {
  if (!candidate || candidate.schemaVersion !== 1 || !SHA256.test(String(candidate.candidateId ?? ''))) {
    fail('PROMOTION_CANDIDATE_INVALID')
  }
  if (expectedCandidateId !== candidate.candidateId) fail('PROMOTION_CANDIDATE_ID_MISMATCH')
  if (!TAG.test(String(tag ?? '')) || tag !== `v${candidate.version}`) {
    fail('PROMOTION_TAG_MISMATCH')
  }
  if (approval !== `PROMOTE:${candidate.candidateId}`) fail('PROMOTION_APPROVAL_REQUIRED')

  verifyCandidateFiles(candidate, candidateRoot)
  validateAcceptance(acceptance, candidate)

  const output = ensureEmptyDirectory(outputRoot)
  const names = new Set()
  const files = candidate.files.map(file => {
    const name = publishedName(file.path)
    if (names.has(name)) fail('PROMOTION_ASSET_NAME_COLLISION')
    names.add(name)

    const source = resolve(candidateRoot, file.path)
    const target = join(output, name)
    copyFileSync(source, target)
    const bytes = readFileSync(target)
    if (bytes.byteLength !== file.size || sha256(bytes) !== file.sha256) {
      fail('PROMOTION_STAGED_HASH_MISMATCH')
    }
    return {
      candidatePath: file.path,
      publishedName: name,
      sha256: file.sha256,
      size: file.size,
      platform: file.platform,
    }
  })

  return {
    schemaVersion: 1,
    candidateId: candidate.candidateId,
    sourceSha: candidate.sourceSha,
    version: candidate.version,
    tag,
    acceptanceCatalogId: acceptance.catalogId,
    acceptancePlanId: acceptance.planId,
    files,
  }
}

export function verifyPublishedPromotion(plan, downloadedRoot, { allowLatestJson = true } = {}) {
  if (!plan || plan.schemaVersion !== 1 || !SHA256.test(String(plan.candidateId ?? ''))
    || !Array.isArray(plan.files) || plan.files.length === 0) {
    fail('PROMOTION_PLAN_INVALID')
  }
  const root = resolve(downloadedRoot)
  const expected = new Map(plan.files.map(file => [file.publishedName, file]))
  const entries = readdirSync(root, { withFileTypes: true }).filter(entry => entry.isFile())

  for (const [name, file] of expected) {
    const path = join(root, name)
    if (!existsSync(path) || !statSync(path).isFile()) fail('PROMOTION_PUBLISHED_FILE_MISSING')
    const bytes = readFileSync(path)
    if (bytes.byteLength !== file.size || sha256(bytes) !== file.sha256) {
      fail('PROMOTION_PUBLISHED_HASH_MISMATCH')
    }
  }

  for (const entry of entries) {
    if (!expected.has(entry.name) && !(allowLatestJson && entry.name === 'latest.json')) {
      fail('PROMOTION_UNDECLARED_PUBLISHED_FILE')
    }
  }
  return true
}

function parseArgs(argv) {
  const result = {}
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    if (![
      '--candidate', '--candidate-root', '--acceptance', '--candidate-id',
      '--tag', '--approval', '--output-root', '--plan',
    ].includes(key)) fail('PROMOTION_OPTION_INVALID')
    if (index + 1 >= argv.length) fail('PROMOTION_OPTION_VALUE_REQUIRED')
    result[key.slice(2)] = argv[++index]
  }
  for (const key of ['candidate', 'candidate-root', 'acceptance', 'candidate-id', 'tag', 'approval', 'output-root', 'plan']) {
    if (!result[key]) fail('PROMOTION_OPTION_REQUIRED')
  }
  return result
}

function main() {
  const args = parseArgs(process.argv.slice(2))
  const candidate = JSON.parse(readFileSync(resolve(args.candidate), 'utf8'))
  const acceptance = JSON.parse(readFileSync(resolve(args.acceptance), 'utf8'))
  const plan = preparePromotion({
    candidate,
    candidateRoot: args['candidate-root'],
    acceptance,
    expectedCandidateId: args['candidate-id'],
    tag: args.tag,
    approval: args.approval,
    outputRoot: args['output-root'],
  })
  writeFileSync(resolve(args.plan), JSON.stringify(plan, null, 2) + '\n')
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code ?? error?.message ?? 'PROMOTION_FAILED') + '\n')
    process.exitCode = 1
  }
}
