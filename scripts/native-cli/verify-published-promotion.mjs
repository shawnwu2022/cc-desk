#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { lstatSync, readFileSync, realpathSync } from 'node:fs'
import { basename, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { readJsonFile } from './verify-acceptance.mjs'

const SHA256 = /^[0-9a-f]{64}$/

function fail(code) {
  throw new Error(code)
}

function hashFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function verifyPublishedPromotion(downloadRoot, promotion) {
  if (
    !promotion
    || promotion.schemaVersion !== 1
    || promotion.status !== 'READY_FOR_PROMOTION'
    || !Array.isArray(promotion.files)
    || promotion.files.length === 0
  ) {
    fail('PUBLISHED_PROMOTION_INVALID')
  }

  const root = realpathSync(resolve(downloadRoot))
  const seen = new Set()
  for (const file of promotion.files) {
    if (
      !file
      || typeof file.assetName !== 'string'
      || basename(file.assetName) !== file.assetName
      || seen.has(file.assetName)
      || !SHA256.test(file.sha256 ?? '')
    ) {
      fail('PUBLISHED_ASSET_RECORD_INVALID')
    }
    seen.add(file.assetName)
    const path = resolve(root, file.assetName)
    const stat = lstatSync(path)
    if (!stat.isFile() || stat.isSymbolicLink()) fail('PUBLISHED_ASSET_INVALID')
    if (hashFile(path) !== file.sha256) fail('PUBLISHED_ASSET_HASH_MISMATCH')
  }

  return {
    status: 'PASS',
    candidateId: promotion.candidateId,
    tag: promotion.tag,
    verifiedFiles: promotion.files.length,
  }
}

function main() {
  try {
    const root = process.argv[2]
    const manifestPath = process.argv[3]
    if (!root || !manifestPath) fail('PUBLISHED_VERIFY_ARGS_REQUIRED')
    const result = verifyPublishedPromotion(resolve(root), readJsonFile(resolve(manifestPath)))
    process.stdout.write(`${JSON.stringify(result)}\n`)
  } catch (error) {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'PUBLISHED_VERIFY_FAILED'
    process.stderr.write(`[published-promotion-error] ${reason}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
