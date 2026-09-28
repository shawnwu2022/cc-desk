#!/usr/bin/env node

import { createHash } from 'node:crypto'
import {
  lstatSync,
  readFileSync,
  readdirSync,
  realpathSync,
} from 'node:fs'
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

function regularFile(path, code) {
  const stat = lstatSync(path)
  if (!stat.isFile() || stat.isSymbolicLink()) fail(code)
}

export function verifyPublishedPromotion(downloadRoot, promotion, updater) {
  if (
    !promotion
    || promotion.schemaVersion !== 1
    || promotion.status !== 'READY_FOR_PROMOTION'
    || !Array.isArray(promotion.files)
    || promotion.files.length === 0
  ) {
    fail('PUBLISHED_PROMOTION_INVALID')
  }
  if (
    !updater
    || typeof updater.assetName !== 'string'
    || basename(updater.assetName) !== updater.assetName
    || !SHA256.test(updater.sha256 ?? '')
  ) {
    fail('PUBLISHED_UPDATER_RECORD_INVALID')
  }

  const root = realpathSync(resolve(downloadRoot))
  const expectedNames = new Set([updater.assetName])
  for (const file of promotion.files) {
    if (
      !file
      || typeof file.assetName !== 'string'
      || basename(file.assetName) !== file.assetName
      || expectedNames.has(file.assetName)
      || !SHA256.test(file.sha256 ?? '')
    ) {
      fail('PUBLISHED_ASSET_RECORD_INVALID')
    }
    expectedNames.add(file.assetName)
  }

  const actualNames = readdirSync(root).sort()
  const expected = [...expectedNames].sort()
  if (actualNames.join('\n') !== expected.join('\n')) {
    fail('PUBLISHED_ASSET_SET_MISMATCH')
  }

  for (const file of promotion.files) {
    const path = resolve(root, file.assetName)
    regularFile(path, 'PUBLISHED_ASSET_INVALID')
    if (hashFile(path) !== file.sha256) fail('PUBLISHED_ASSET_HASH_MISMATCH')
  }

  const updaterPath = resolve(root, updater.assetName)
  regularFile(updaterPath, 'PUBLISHED_UPDATER_INVALID')
  if (hashFile(updaterPath) !== updater.sha256) fail('PUBLISHED_UPDATER_HASH_MISMATCH')

  return {
    status: 'PASS',
    candidateId: promotion.candidateId,
    tag: promotion.tag,
    verifiedFiles: expected.length,
  }
}

function updaterRecord(path) {
  const full = realpathSync(resolve(path))
  regularFile(full, 'PUBLISHED_UPDATER_INVALID')
  return {
    assetName: basename(full),
    sha256: hashFile(full),
  }
}

function main() {
  try {
    const root = process.argv[2]
    const manifestPath = process.argv[3]
    const updaterPath = process.argv[4]
    if (!root || !manifestPath || !updaterPath) fail('PUBLISHED_VERIFY_ARGS_REQUIRED')
    const result = verifyPublishedPromotion(
      resolve(root),
      readJsonFile(resolve(manifestPath)),
      updaterRecord(updaterPath),
    )
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
