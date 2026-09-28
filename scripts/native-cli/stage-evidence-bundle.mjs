#!/usr/bin/env node

import { createHash } from 'node:crypto'
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from 'node:fs'
import {
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

const SHA256 = /^[0-9a-f]{64}$/

function fail(code) {
  throw new Error(code)
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

function sourceEvidence(root, relativePath, expectedHash) {
  if (!safeRelative(relativePath) || !SHA256.test(expectedHash ?? '')) {
    fail('EVIDENCE_STAGE_REFERENCE_INVALID')
  }
  const rootReal = realpathSync(root)
  const full = resolve(rootReal, relativePath)
  const stat = lstatSync(full)
  if (!stat.isFile() || stat.isSymbolicLink()) fail('EVIDENCE_STAGE_FILE_INVALID')
  const real = realpathSync(full)
  const rel = relative(rootReal, real)
  if (rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
    fail('EVIDENCE_STAGE_PATH_ESCAPE')
  }
  const actual = createHash('sha256').update(readFileSync(real)).digest('hex')
  if (actual !== expectedHash) fail('EVIDENCE_STAGE_HASH_MISMATCH')
  return real
}

export function stageEvidenceBundle({
  catalog,
  candidate,
  plan,
  records,
  evidenceRoot,
  outDir,
}) {
  const gate = verifyAcceptance({
    catalog,
    candidate,
    plan,
    records,
    evidenceRoot,
  })
  if (gate.status !== 'PASS') {
    const error = new Error('EVIDENCE_STAGE_GATE_REJECTED')
    error.gate = gate
    throw error
  }

  const out = resolve(outDir)
  if (existsSync(out)) fail('EVIDENCE_STAGE_OUTPUT_EXISTS')
  mkdirSync(resolve(out, 'evidence'), { recursive: true, mode: 0o700 })

  const references = new Map()
  for (const record of records) {
    for (const entry of record.evidence) {
      const previous = references.get(entry.path)
      if (previous && previous !== entry.sha256) fail('EVIDENCE_STAGE_HASH_CONFLICT')
      references.set(entry.path, entry.sha256)
    }
  }

  for (const [relativePath, expectedHash] of references) {
    const source = sourceEvidence(evidenceRoot, relativePath, expectedHash)
    const destination = resolve(out, 'evidence', relativePath)
    const evidenceOut = realpathSync(resolve(out, 'evidence'))
    const parent = resolve(dirname(destination))
    const rel = relative(evidenceOut, parent)
    if (rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
      fail('EVIDENCE_STAGE_PATH_ESCAPE')
    }
    mkdirSync(parent, { recursive: true, mode: 0o700 })
    copyFileSync(source, destination)
    const copied = createHash('sha256').update(readFileSync(destination)).digest('hex')
    if (copied !== expectedHash) fail('EVIDENCE_STAGE_COPY_MISMATCH')
  }

  writeFileSync(resolve(out, 'plan.json'), `${JSON.stringify(plan, null, 2)}\n`, {
    mode: 0o600,
  })
  writeFileSync(resolve(out, 'records.json'), `${JSON.stringify(records, null, 2)}\n`, {
    mode: 0o600,
  })

  const summary = {
    schemaVersion: 1,
    status: 'PASS',
    candidateId: candidate.candidateId,
    targetIds: gate.targetIds,
    recordCount: gate.recordCount,
    evidenceFileCount: references.size,
    planSha256: sha256Json(plan),
    recordsSha256: sha256Json(records),
  }
  writeFileSync(resolve(out, 'bundle-summary.json'), `${JSON.stringify(summary, null, 2)}\n`, {
    mode: 0o600,
  })
  return summary
}

function parse(argv) {
  const allowed = new Set([
    '--catalog',
    '--candidate',
    '--plan',
    '--records',
    '--evidence-root',
    '--out',
  ])
  const values = {}
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index]
    if (!allowed.has(option) || index + 1 >= argv.length || values[option]) {
      fail('EVIDENCE_STAGE_OPTION_INVALID')
    }
    values[option] = argv[++index]
  }
  for (const option of allowed) {
    if (!values[option]) fail('EVIDENCE_STAGE_OPTION_REQUIRED')
  }
  return values
}

function main() {
  try {
    const args = parse(process.argv.slice(2))
    const summary = stageEvidenceBundle({
      catalog: readJsonFile(resolve(args['--catalog'])),
      candidate: readJsonFile(resolve(args['--candidate'])),
      plan: readJsonFile(resolve(args['--plan'])),
      records: readJsonFile(resolve(args['--records'])),
      evidenceRoot: resolve(args['--evidence-root']),
      outDir: resolve(args['--out']),
    })
    process.stdout.write(`${JSON.stringify(summary)}\n`)
  } catch (error) {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'EVIDENCE_STAGE_FAILED'
    process.stderr.write(`[evidence-stage-error] ${reason}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
