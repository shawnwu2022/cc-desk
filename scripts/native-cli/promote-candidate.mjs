#!/usr/bin/env node

import { createHash } from 'node:crypto'
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  writeFileSync,
} from 'node:fs'
import {
  basename,
  dirname,
  isAbsolute,
  relative,
  resolve,
  sep,
} from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  readJsonFile,
  sha256Json,
  verifyAcceptance,
} from './verify-acceptance.mjs'

const COMMIT = /^[0-9a-f]{40}$/
const TAG = /^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/
const SHA256 = /^[0-9a-f]{64}$/
const MAX_FILE_BYTES = 2 * 1024 * 1024 * 1024

function fail(code) {
  throw new Error(code)
}

function hashFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function safeRelative(value) {
  return (
    typeof value === 'string'
    && value.length > 0
    && !value.includes('\0')
    && !isAbsolute(value)
    && !/^[A-Za-z]:[\\/]/.test(value)
    && !value.split(/[\\/]+/).includes('..')
  )
}

function resolveCandidateFile(root, relativePath) {
  if (!safeRelative(relativePath)) fail('PROMOTION_CANDIDATE_PATH_INVALID')
  const full = resolve(root, relativePath)
  const stat = lstatSync(full)
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size <= 0 || stat.size > MAX_FILE_BYTES) {
    fail('PROMOTION_CANDIDATE_FILE_INVALID')
  }
  const real = realpathSync(full)
  const rel = relative(root, real)
  if (rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
    fail('PROMOTION_CANDIDATE_PATH_ESCAPE')
  }
  return real
}

function walk(root, directory = root) {
  const paths = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name)
    const stat = lstatSync(path)
    if (stat.isSymbolicLink()) fail('PROMOTION_CANDIDATE_SYMLINK')
    if (stat.isDirectory()) {
      paths.push(...walk(root, path))
    } else if (stat.isFile()) {
      paths.push(relative(root, path).split(sep).join('/'))
    } else {
      fail('PROMOTION_CANDIDATE_FILE_INVALID')
    }
  }
  return paths.sort()
}

function verifyCandidateFiles(candidateRoot, candidate) {
  const root = realpathSync(resolve(candidateRoot))
  const expected = candidate.files.map(file => file.path).sort()
  const actual = walk(root)
  if (expected.join('\n') !== actual.join('\n')) fail('PROMOTION_CANDIDATE_FILE_SET_MISMATCH')

  for (const file of candidate.files) {
    if (!SHA256.test(file.sha256 ?? '')) fail('PROMOTION_CANDIDATE_HASH_INVALID')
    const path = resolveCandidateFile(root, file.path)
    if (hashFile(path) !== file.sha256) fail('PROMOTION_CANDIDATE_HASH_MISMATCH')
  }
  return root
}

function packageVersion(repoRoot) {
  const path = resolve(repoRoot, 'package.json')
  const value = JSON.parse(readFileSync(path, 'utf8'))
  if (typeof value.version !== 'string' || value.version.length === 0) {
    fail('PROMOTION_VERSION_INVALID')
  }
  return value.version
}

function publishedAssetName(path) {
  const name = basename(path)
  if (!name) fail('PROMOTION_ASSET_NAME_INVALID')
  return name.replaceAll(' ', '.')
}

function ensureEmptyPublishDir(path) {
  if (existsSync(path)) {
    const stat = lstatSync(path)
    if (!stat.isDirectory() || readdirSync(path).length !== 0) {
      fail('PROMOTION_PUBLISH_DIR_NOT_EMPTY')
    }
  } else {
    mkdirSync(path, { recursive: true, mode: 0o700 })
  }
}

export function preparePromotion({
  repoRoot,
  candidateRoot,
  candidate,
  catalog,
  plan,
  records,
  evidenceRoot,
  expectedSourceCommit,
  tag,
  publishDir,
}) {
  if (!COMMIT.test(expectedSourceCommit ?? '') || candidate?.sourceCommit !== expectedSourceCommit) {
    fail('PROMOTION_SOURCE_COMMIT_MISMATCH')
  }
  const tagMatch = TAG.exec(tag ?? '')
  if (!tagMatch) fail('PROMOTION_TAG_INVALID')
  const version = packageVersion(repoRoot)
  if (tag !== `v${version}`) fail('PROMOTION_VERSION_MISMATCH')

  const verifiedRoot = verifyCandidateFiles(candidateRoot, candidate)
  const acceptance = verifyAcceptance({
    catalog,
    plan,
    candidate,
    records,
    evidenceRoot,
  })
  if (acceptance.status !== 'PASS') {
    const error = new Error('PROMOTION_ACCEPTANCE_REJECTED')
    error.acceptance = acceptance
    throw error
  }

  const publish = resolve(publishDir)
  ensureEmptyPublishDir(publish)
  const assetNames = new Set()
  const releaseFiles = []
  for (const file of candidate.files) {
    const assetName = publishedAssetName(file.path)
    if (assetNames.has(assetName)) fail('PROMOTION_ASSET_NAME_COLLISION')
    assetNames.add(assetName)
    const source = resolveCandidateFile(verifiedRoot, file.path)
    const destination = resolve(publish, assetName)
    copyFileSync(source, destination)
    if (hashFile(destination) !== file.sha256) fail('PROMOTION_STAGING_HASH_MISMATCH')
    releaseFiles.push({
      assetName,
      sourcePath: file.path,
      platform: file.platform,
      arch: file.arch,
      sha256: file.sha256,
    })
  }

  const acceptanceSummary = {
    schemaVersion: 1,
    status: 'PASS',
    candidateId: candidate.candidateId,
    targetIds: acceptance.targetIds,
    recordCount: acceptance.recordCount,
    inputs: {
      catalogSha256: sha256Json(catalog),
      candidateSha256: sha256Json(candidate),
      planSha256: sha256Json(plan),
      recordsSha256: sha256Json(records),
    },
  }

  const promotion = {
    schemaVersion: 1,
    status: 'READY_FOR_PROMOTION',
    candidateId: candidate.candidateId,
    sourceCommit: candidate.sourceCommit,
    tag,
    version,
    targetIds: acceptance.targetIds,
    files: releaseFiles.sort((a, b) => a.assetName.localeCompare(b.assetName)),
    acceptanceSummarySha256: sha256Json(acceptanceSummary),
  }

  const auditDir = dirname(publish)
  writeFileSync(resolve(auditDir, 'candidate.json'), `${JSON.stringify(candidate, null, 2)}\n`, {
    mode: 0o600,
  })
  writeFileSync(
    resolve(auditDir, 'acceptance-summary.json'),
    `${JSON.stringify(acceptanceSummary, null, 2)}\n`,
    { mode: 0o600 },
  )
  writeFileSync(resolve(auditDir, 'promotion.json'), `${JSON.stringify(promotion, null, 2)}\n`, {
    mode: 0o600,
  })

  return { promotion, acceptanceSummary }
}

function parseArgs(argv) {
  const allowed = new Set([
    '--repo-root',
    '--candidate-root',
    '--candidate',
    '--catalog',
    '--plan',
    '--records',
    '--evidence-root',
    '--source-commit',
    '--tag',
    '--publish-dir',
    '--out',
  ])
  const values = {}
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index]
    if (!allowed.has(option) || index + 1 >= argv.length || values[option]) {
      fail('PROMOTION_OPTION_INVALID')
    }
    values[option] = argv[++index]
  }
  for (const option of allowed) {
    if (!values[option]) fail('PROMOTION_OPTION_REQUIRED')
  }
  return values
}

function main() {
  try {
    const args = parseArgs(process.argv.slice(2))
    const candidate = readJsonFile(resolve(args['--candidate']))
    const catalog = readJsonFile(resolve(args['--catalog']))
    const plan = readJsonFile(resolve(args['--plan']))
    const records = readJsonFile(resolve(args['--records']))
    const result = preparePromotion({
      repoRoot: resolve(args['--repo-root']),
      candidateRoot: resolve(args['--candidate-root']),
      candidate,
      catalog,
      plan,
      records,
      evidenceRoot: resolve(args['--evidence-root']),
      expectedSourceCommit: args['--source-commit'],
      tag: args['--tag'],
      publishDir: resolve(args['--publish-dir']),
    })
    const out = resolve(args['--out'])
    mkdirSync(dirname(out), { recursive: true })
    writeFileSync(out, `${JSON.stringify(result.promotion, null, 2)}\n`, {
      mode: 0o600,
    })
    process.stdout.write(`${JSON.stringify({
      status: result.promotion.status,
      candidateId: result.promotion.candidateId,
      tag: result.promotion.tag,
      fileCount: result.promotion.files.length,
    })}\n`)
  } catch (error) {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'PROMOTION_FAILED'
    process.stderr.write(`[promotion-error] ${reason}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
