#!/usr/bin/env node

import { readFileSync, statSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const ALLOWED = new Map([
  ['claude', { package: '@anthropic-ai/claude-code', binary: 'claude' }],
  ['codex', { package: '@openai/codex', binary: 'codex' }],
])
const VERSION = /^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:[-+][A-Za-z0-9.-]+)?$/
const TAG = /^[a-z][a-z0-9._-]{0,31}$/
const MAX_BYTES = 64 * 1024

function fail(reason) {
  throw new Error(reason)
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

export function buildCanaryMatrix(config) {
  if (!isObject(config) || config.schemaVersion !== 1 || !Array.isArray(config.products)) {
    fail('INVALID_CANARY_CONFIG')
  }
  if (config.products.length !== ALLOWED.size) fail('INVALID_CANARY_PRODUCT_SET')

  const seen = new Set()
  const include = []
  for (const product of config.products) {
    if (!isObject(product) || !ALLOWED.has(product.cli)) {
      fail('INVALID_CANARY_PRODUCT')
    }
    if (seen.has(product.cli)) fail('DUPLICATE_CANARY_PRODUCT')
    seen.add(product.cli)

    const expected = ALLOWED.get(product.cli)
    if (
      product.package !== expected.package
      || product.binary !== expected.binary
      || !VERSION.test(product.pinned ?? '')
      || !TAG.test(product.stableTag ?? '')
    ) {
      fail('INVALID_CANARY_PRODUCT')
    }

    include.push({
      cli: product.cli,
      channel: 'pinned',
      package: product.package,
      binary: product.binary,
      selector: product.pinned,
      packageSpec: `${product.package}@${product.pinned}`,
    })
    include.push({
      cli: product.cli,
      channel: 'stable',
      package: product.package,
      binary: product.binary,
      selector: product.stableTag,
      packageSpec: `${product.package}@${product.stableTag}`,
    })
  }

  for (const cli of ALLOWED.keys()) {
    if (!seen.has(cli)) fail('INVALID_CANARY_PRODUCT_SET')
  }

  include.sort((left, right) =>
    `${left.cli}:${left.channel}`.localeCompare(`${right.cli}:${right.channel}`))
  return { include }
}

function loadConfig(path) {
  let stat
  try {
    stat = statSync(path)
  } catch {
    fail('CANARY_CONFIG_UNAVAILABLE')
  }
  if (!stat.isFile() || stat.size <= 0 || stat.size > MAX_BYTES) {
    fail('CANARY_CONFIG_SIZE_INVALID')
  }
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    fail('INVALID_CANARY_CONFIG_JSON')
  }
}

async function main() {
  const path = process.argv[2]
  if (!path || process.argv.length !== 3) fail('CANARY_CONFIG_PATH_REQUIRED')
  process.stdout.write(`${JSON.stringify(buildCanaryMatrix(loadConfig(resolve(path))))}\n`)
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  main().catch(error => {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'CANARY_MATRIX_FAILED'
    process.stderr.write(`[native-canary-matrix] ${reason}\n`)
    process.exitCode = 1
  })
}
