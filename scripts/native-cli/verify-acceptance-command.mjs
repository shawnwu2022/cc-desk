#!/usr/bin/env node

import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  readJsonFile,
  sha256Json,
  verifyAcceptance,
} from './verify-acceptance.mjs'

function parseArgs(argv) {
  const values = {}
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    if (!['--catalog', '--candidate', '--plan', '--records', '--evidence-root', '--out'].includes(key)) {
      throw new Error('UNKNOWN_OPTION')
    }
    if (index + 1 >= argv.length) throw new Error('MISSING_OPTION_VALUE')
    if (values[key]) throw new Error('DUPLICATE_OPTION')
    values[key] = argv[++index]
  }
  for (const required of ['--catalog', '--candidate', '--plan', '--records', '--evidence-root']) {
    if (!values[required]) throw new Error('REQUIRED_OPTION_MISSING')
  }
  return {
    catalogPath: resolve(values['--catalog']),
    candidatePath: resolve(values['--candidate']),
    planPath: resolve(values['--plan']),
    recordsPath: resolve(values['--records']),
    evidenceRoot: resolve(values['--evidence-root']),
    outPath: values['--out'] ? resolve(values['--out']) : null,
  }
}

export function runAcceptanceCommand(args) {
  const catalog = readJsonFile(args.catalogPath)
  const candidate = readJsonFile(args.candidatePath)
  const plan = readJsonFile(args.planPath)
  const records = readJsonFile(args.recordsPath)
  const result = verifyAcceptance({
    catalog,
    candidate,
    plan,
    records,
    evidenceRoot: args.evidenceRoot,
  })
  const summary = {
    schemaVersion: 1,
    ...result,
    inputs: {
      catalogSha256: sha256Json(catalog),
      candidateSha256: sha256Json(candidate),
      planSha256: sha256Json(plan),
      recordsSha256: sha256Json(records),
    },
  }
  if (args.outPath) {
    mkdirSync(dirname(args.outPath), { recursive: true })
    writeFileSync(args.outPath, `${JSON.stringify(summary, null, 2)}\n`, {
      encoding: 'utf8',
      mode: 0o600,
    })
  }
  return summary
}

function main() {
  let result
  try {
    result = runAcceptanceCommand(parseArgs(process.argv.slice(2)))
  } catch (error) {
    result = {
      schemaVersion: 1,
      status: 'FAIL',
      reason: error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
        ? error.message
        : 'ACCEPTANCE_GATE_FAILED',
    }
  }
  process.stdout.write(`${JSON.stringify(result)}\n`)
  process.exitCode = result.status === 'PASS' ? 0 : 1
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main()
}
