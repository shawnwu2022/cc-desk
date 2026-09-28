import assert from 'node:assert/strict'
import test from 'node:test'
import { probeCliIdentity } from '../../scripts/native-cli/canary-identity.mjs'

test('D29_Canary_NodeBinaryProducesNonCertificationIdentity_01', () => {
  const version = process.version.replace(/^v/, '')
  const result = probeCliIdentity({
    cli: 'codex',
    binaryPath: process.execPath,
    channel: 'pinned',
    expectedVersion: version,
    sourceEnv: {
      PATH: process.env.PATH,
      FIXTURE_SECRET: 'must-not-be-forwarded',
    },
  })

  assert.equal(result.status, 'PASS')
  assert.equal(result.certification, false)
  assert.equal(result.cli, 'codex')
  assert.equal(result.channel, 'pinned')
  assert.match(result.binarySha256, /^[0-9a-f]{64}$/)
  assert.match(result.helpSha256, /^[0-9a-f]{64}$/)
  assert.ok(result.observedVersion.includes(version))
})

test('D29_Canary_PinnedVersionMismatchFailsClosed_02', () => {
  assert.throws(
    () => probeCliIdentity({
      cli: 'claude',
      binaryPath: process.execPath,
      channel: 'pinned',
      expectedVersion: '0.0.1',
    }),
    /CANARY_PINNED_VERSION_MISMATCH/,
  )
})

test('D29_Canary_StableChannelDoesNotPretendPinnedIdentity_03', () => {
  const result = probeCliIdentity({
    cli: 'claude',
    binaryPath: process.execPath,
    channel: 'stable',
  })
  assert.equal(result.expectedVersion, null)
  assert.equal(result.certification, false)
})

test('D29_Canary_RejectsUnknownCliOrChannel_04', () => {
  assert.throws(
    () => probeCliIdentity({
      cli: 'shell',
      binaryPath: process.execPath,
      channel: 'stable',
    }),
    /CANARY_CLI_INVALID/,
  )
  assert.throws(
    () => probeCliIdentity({
      cli: 'codex',
      binaryPath: process.execPath,
      channel: 'nightly',
    }),
    /CANARY_CHANNEL_INVALID/,
  )
})
