#!/usr/bin/env node

import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'
import {
  mayPublish,
  promotionComplete,
} from '../release-policy.mjs'

function fail(code) {
  throw new Error(code)
}

function parse(argv) {
  const context = {
    event: null,
    operation: null,
    gatePassed: false,
    manifestVerified: false,
    sameCandidate: false,
    explicitApproval: false,
    rebuildPerformed: true,
    publishedBytesVerified: false,
  }
  let phase = null

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--phase') {
      phase = argv[++index]
    } else if (arg === '--event') {
      context.event = argv[++index]
    } else if (arg === '--operation') {
      context.operation = argv[++index]
    } else if (arg === '--gate-passed') {
      context.gatePassed = true
    } else if (arg === '--manifest-verified') {
      context.manifestVerified = true
    } else if (arg === '--same-candidate') {
      context.sameCandidate = true
    } else if (arg === '--explicit-approval') {
      context.explicitApproval = true
    } else if (arg === '--no-rebuild') {
      context.rebuildPerformed = false
    } else if (arg === '--published-bytes-verified') {
      context.publishedBytesVerified = true
    } else {
      fail('PROMOTION_POLICY_OPTION_INVALID')
    }
  }

  if (!['pre', 'complete'].includes(phase)) fail('PROMOTION_POLICY_PHASE_INVALID')
  return { phase, context }
}

export function checkPromotionPolicy(argv) {
  const { phase, context } = parse(argv)
  const allowed = phase === 'pre'
    ? mayPublish(context)
    : promotionComplete(context)
  if (!allowed) fail('PROMOTION_POLICY_REJECTED')
  return { status: 'PASS', phase }
}

function main() {
  try {
    const result = checkPromotionPolicy(process.argv.slice(2))
    process.stdout.write(`${JSON.stringify(result)}\n`)
  } catch (error) {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'PROMOTION_POLICY_FAILED'
    process.stderr.write(`[promotion-policy-error] ${reason}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
