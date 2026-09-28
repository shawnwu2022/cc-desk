#!/usr/bin/env node

/**
 * Legacy direct-release entrypoint intentionally disabled by D31.
 *
 * Publishing must flow through:
 *   1. Signed candidate packages
 *   2. Native CLI acceptance gate
 *   3. Promote accepted candidate
 *
 * This shim remains so old local automation fails closed with an explicit
 * diagnostic instead of silently recreating the pre-gate publish path.
 */

process.stderr.write(
  'DIRECT_RELEASE_DISABLED: use the signed candidate, acceptance gate, and promotion workflows\n',
)
process.exitCode = 1
