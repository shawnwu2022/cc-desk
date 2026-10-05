#!/usr/bin/env node

console.error(
  'Legacy release publishing is disabled. Current policy: signed candidates only. ' +
  'Public promotion is not enabled. See docs/release-process.md for the controlled release process.',
)
process.exitCode = 1
