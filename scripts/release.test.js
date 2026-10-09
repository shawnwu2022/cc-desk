#!/usr/bin/env node

// The legacy updater/OSS generator contract is retired. Keep this historical
// test command as an alias for the current fail-closed publication policy suite.
const { spawnSync } = require('node:child_process')
const path = require('node:path')

console.log('Legacy updater/OSS generator tests are retired; running the current release policy suite.')
const result = spawnSync(process.execPath, [
  '--test',
  path.join(__dirname, '../tests/scripts/nativeReleasePolicy.node.mjs'),
], { stdio: 'inherit' })
if (result.error) console.error('Unable to run the release policy suite:', result.error.message)
process.exitCode = result.status ?? 1
