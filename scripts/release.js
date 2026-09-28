#!/usr/bin/env node

/**
 * Legacy local publisher intentionally disabled.
 *
 * Native CLI releases must use:
 *   Signed candidate packages
 *     -> installed-package evidence
 *     -> Native CLI acceptance gate
 *     -> Promote verified native candidate
 *
 * The promotion workflow republishes the exact previously built candidate
 * bytes and verifies the published hashes. This shim exists so old local
 * instructions fail closed instead of silently bypassing the release gate.
 */

const CODE = 'LEGACY_RELEASE_DISABLED_USE_NATIVE_PROMOTION'

function main() {
  if (process.argv.includes('--help') || process.argv.includes('-h')) {
    process.stdout.write(
      [
        CODE,
        '',
        'Direct local release/tag/publish is disabled.',
        'Use the GitHub Actions workflow: Promote verified native candidate.',
        '',
      ].join('\n'),
    )
    return
  }

  process.stderr.write(`${CODE}\n`)
  process.exitCode = 2
}

module.exports = { CODE }

if (require.main === module) main()
