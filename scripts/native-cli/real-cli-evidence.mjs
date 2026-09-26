import { createHash } from 'node:crypto'

const SHA256 = /^[0-9a-f]{64}$/i
const COMMIT_SHA = /^[0-9a-f]{40}$/i
const EXACT_TRANSFORMS = new Set([
  'codex-user-prompt-submit-v1-exact',
  'claude-user-prompt-submit-v1-exact',
])
const CLAUDE_PASTED_TRANSFORM = 'claude-user-prompt-submit-pasted-content-v1'

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function text(value) {
  return typeof value === 'string' && value.length > 0
}

function fail(reason) {
  return { valid: false, reason }
}

function canonicalBase64(value) {
  if (typeof value !== 'string' || value.length === 0) return null
  try {
    const bytes = Buffer.from(value, 'base64')
    if (bytes.toString('base64') !== value) return null
    return bytes
  } catch {
    return null
  }
}

export function realCliFixtureSha256(fixture) {
  if (!isObject(fixture)) return null
  const identity = {
    nonce: fixture.nonce,
    originalText: fixture.originalText,
    hostPayloadBase64: fixture.hostPayloadBase64,
    transformId: fixture.transformId,
  }
  return createHash('sha256')
    .update(JSON.stringify(identity), 'utf8')
    .digest('hex')
}

function cliKindFrom(record) {
  return record?.target?.cli?.kind ?? record?.cli ?? null
}

function rawPrompt(record) {
  return record?.oracle?.rawEnvelope?.prompt
}

function targetFingerprint(record) {
  const value = record.target
  return JSON.stringify({
    targetId: value.targetId,
    os: value.os,
    osBuild: value.osBuild,
    arch: value.arch,
    cliKind: value.cli?.kind,
    cliVersion: value.cli?.version,
    binarySha256: value.cli?.binarySha256,
  })
}

function fixtureFingerprint(record) {
  const value = record.fixture
  return JSON.stringify({
    fixtureSha256: value.fixtureSha256,
    nonce: value.nonce,
    originalText: value.originalText,
    hostPayloadBase64: value.hostPayloadBase64,
    transformId: value.transformId,
  })
}

export function verifyPromptTransform(transformId, hostPayload, observedPrompt) {
  if (typeof hostPayload !== 'string' || typeof observedPrompt !== 'string') {
    return fail('CONTENT_MISMATCH')
  }

  if (EXACT_TRANSFORMS.has(transformId)) {
    return hostPayload === observedPrompt
      ? { valid: true }
      : fail('CONTENT_MISMATCH')
  }

  if (transformId !== CLAUDE_PASTED_TRANSFORM) {
    return fail('UNSUPPORTED_PROMPT_TRANSFORM')
  }

  const newline = observedPrompt.indexOf('\n')
  if (newline <= 0) return fail('CONTENT_MISMATCH')
  const opening = observedPrompt.slice(0, newline)
  const match = /^<pasted_content id="([^"\r\n]{1,128})">$/.exec(opening)
  if (!match) return fail('CONTENT_MISMATCH')

  const closing = `\n</pasted_content id="${match[1]}">`
  if (!observedPrompt.endsWith(closing)) return fail('CONTENT_MISMATCH')
  const body = observedPrompt.slice(newline + 1, observedPrompt.length - closing.length)
  return body === hostPayload
    ? { valid: true }
    : fail('CONTENT_MISMATCH')
}

function validateBlocked(record) {
  if (record.schemaVersion !== 1) return fail('SCHEMA_VERSION_REQUIRED')
  if (record.caseId !== 'NATIVE-63') return fail('CASE_ID_REQUIRED')
  if (record.evidenceLayer !== 'C') return fail('EVIDENCE_LAYER_REQUIRED')
  if (!text(record.runId)) return fail('RUN_ID_REQUIRED')
  if (!['claude', 'codex'].includes(record.cli)) return fail('CLI_KIND_REQUIRED')
  if (!text(record.reason)) return fail('BLOCKED_REASON_REQUIRED')
  return { valid: true }
}

export function validateRealCliRun(record) {
  if (!isObject(record)) return fail('INVALID_REAL_CLI_EVIDENCE')
  if (record.status === 'BLOCKED') return validateBlocked(record)
  if (record.status !== 'PASS') return fail('PASS_OR_BLOCKED_REQUIRED')
  if (record.schemaVersion !== 1) return fail('SCHEMA_VERSION_REQUIRED')
  if (record.caseId !== 'NATIVE-63') return fail('CASE_ID_REQUIRED')
  if (record.evidenceLayer !== 'C') return fail('EVIDENCE_LAYER_REQUIRED')
  if (!text(record.runId)) return fail('RUN_ID_REQUIRED')
  if (!['cc-desk', 'system-terminal'].includes(record.lane)) {
    return fail('LANE_REQUIRED')
  }
  if (!['off', 'on'].includes(record.observer)) return fail('OBSERVER_STATE_REQUIRED')

  const target = record.target
  if (
    !isObject(target)
    || !text(target.targetId)
    || !text(target.os)
    || !text(target.osBuild)
    || !text(target.arch)
    || !isObject(target.cli)
    || !['claude', 'codex'].includes(target.cli.kind)
    || !text(target.cli.version)
  ) {
    return fail('TARGET_IDENTITY_REQUIRED')
  }
  if (!SHA256.test(target.cli.binarySha256 ?? '')) {
    return fail('CLI_BINARY_IDENTITY_REQUIRED')
  }

  const host = record.host
  if (!isObject(host)) return fail('HOST_IDENTITY_REQUIRED')
  if (record.lane === 'cc-desk') {
    if (
      host.kind !== 'cc-desk'
      || !COMMIT_SHA.test(host.deskCommit ?? '')
      || !text(host.xtermVersion)
      || !text(host.webviewRuntime)
      || !text(host.renderer)
    ) {
      return fail('DESK_IDENTITY_REQUIRED')
    }
  } else if (host.kind !== 'system-terminal' || !text(host.terminalProgram)) {
    return fail('SYSTEM_TERMINAL_IDENTITY_REQUIRED')
  }

  const fixture = record.fixture
  if (
    !isObject(fixture)
    || !SHA256.test(fixture.fixtureSha256 ?? '')
    || !text(fixture.nonce)
    || typeof fixture.originalText !== 'string'
    || !text(fixture.transformId)
  ) {
    return fail('FIXTURE_IDENTITY_REQUIRED')
  }
  const payloadBytes = canonicalBase64(fixture.hostPayloadBase64)
  if (!payloadBytes) return fail('HOST_PAYLOAD_REQUIRED')
  const fixtureHash = realCliFixtureSha256(fixture)
  if (
    fixtureHash === null
    || fixture.fixtureSha256.toLowerCase() !== fixtureHash
  ) {
    return fail('FIXTURE_HASH_MISMATCH')
  }
  const hostPayload = payloadBytes.toString('utf8')

  const provenance = record.hostPayloadEvidence
  if (!isObject(provenance)) {
    return fail('HOST_PAYLOAD_PROVENANCE_REQUIRED')
  }

  if (record.lane === 'cc-desk') {
    if (
      provenance.kind !== 'native-input-frame'
      || !isObject(provenance.frame)
      || !text(provenance.frame.runId)
      || !Number.isInteger(provenance.frame.generation)
      || provenance.frame.generation < 0
      || provenance.frame.generation > 0xffffffff
      || !/^(?:0|[1-9][0-9]*)$/.test(provenance.frame.inputSeq ?? '')
      || !/^(?:0|[1-9][0-9]*)$/.test(provenance.frame.modeEpoch ?? '')
    ) {
      return fail('DESK_HOST_PAYLOAD_FRAME_REQUIRED')
    }
  } else if (
    provenance.kind !== 'terminal-driver-write'
    || !text(provenance.driver)
    || !/^(?:0|[1-9][0-9]*)$/.test(provenance.writeSeq ?? '')
  ) {
    return fail('SYSTEM_TERMINAL_WRITE_EVIDENCE_REQUIRED')
  }

  const provenanceBytes = canonicalBase64(provenance.bytesBase64)
  const actualPayloadHash = createHash('sha256').update(payloadBytes).digest('hex')
  if (
    !provenanceBytes
    || !provenanceBytes.equals(payloadBytes)
    || !SHA256.test(provenance.sha256 ?? '')
    || provenance.sha256.toLowerCase() !== actualPayloadHash
  ) {
    return fail('HOST_PAYLOAD_EVIDENCE_MISMATCH')
  }
  if (
    !fixture.originalText.includes(fixture.nonce)
    || !hostPayload.includes(fixture.nonce)
  ) {
    return fail('RUN_NONCE_REQUIRED')
  }

  if (
    target.cli.kind === 'codex'
    && fixture.transformId !== 'codex-user-prompt-submit-v1-exact'
  ) {
    return fail('TRANSFORM_CLI_MISMATCH')
  }
  if (
    target.cli.kind === 'claude'
    && ![
      'claude-user-prompt-submit-v1-exact',
      CLAUDE_PASTED_TRANSFORM,
    ].includes(fixture.transformId)
  ) {
    return fail('TRANSFORM_CLI_MISMATCH')
  }

  const oracle = record.oracle
  if (!isObject(oracle) || oracle.kind !== 'real-user-prompt-submit') {
    return fail('REAL_ORACLE_REQUIRED')
  }
  if (oracle.schemaVersion !== 1 || !isObject(oracle.validation) || oracle.validation.valid !== true) {
    return fail('VALID_ORACLE_REQUIRED')
  }
  if (
    oracle.cli !== target.cli.kind
    || oracle.runId !== record.runId
    || oracle.lane !== record.lane
    || oracle.observer !== record.observer
    || oracle.transformId !== fixture.transformId
  ) {
    return fail('ORACLE_PROVENANCE_MISMATCH')
  }
  if (!isObject(oracle.rawEnvelope)) return fail('RAW_ENVELOPE_REQUIRED')
  if (!text(oracle.sessionId) || !text(oracle.cwd)) return fail('ORACLE_IDENTITY_REQUIRED')
  if (oracle.rawEnvelope.hook_event_name !== 'UserPromptSubmit') {
    return fail('ORACLE_EVENT_MISMATCH')
  }
  if (
    oracle.rawEnvelope.session_id !== oracle.sessionId
    || oracle.rawEnvelope.cwd !== oracle.cwd
  ) {
    return fail('ORACLE_IDENTITY_MISMATCH')
  }

  if (target.cli.kind === 'codex') {
    if (!text(oracle.turnId)) return fail('CODEX_TURN_ID_REQUIRED')
    if (oracle.rawEnvelope.turn_id !== oracle.turnId) {
      return fail('CODEX_TURN_ID_MISMATCH')
    }
  } else if (oracle.turnId !== null && oracle.turnId !== undefined) {
    if (!text(oracle.turnId) || oracle.rawEnvelope.turn_id !== oracle.turnId) {
      return fail('CLAUDE_TURN_ID_MISMATCH')
    }
  }

  const transform = verifyPromptTransform(
    fixture.transformId,
    hostPayload,
    oracle.rawEnvelope.prompt,
  )
  if (!transform.valid) return transform
  return { valid: true }
}

function pairByLane(records, lane) {
  return {
    off: records.find(record => record.lane === lane && record.observer === 'off'),
    on: records.find(record => record.lane === lane && record.observer === 'on'),
  }
}

function exactObserverPromptChanged(pair) {
  if (!pair.off || !pair.on) return false
  const transformId = pair.off.fixture?.transformId
  return (
    EXACT_TRANSFORMS.has(transformId)
    && pair.on.fixture?.transformId === transformId
    && rawPrompt(pair.off) !== rawPrompt(pair.on)
  )
}

export function certifyCliComparison(records) {
  if (!Array.isArray(records)) {
    return { status: 'BLOCKED', reason: 'INCOMPLETE_REAL_CLI_EVIDENCE' }
  }
  if (records.some(record => record?.status === 'BLOCKED')) {
    return { status: 'BLOCKED', reason: 'INCOMPLETE_REAL_CLI_EVIDENCE' }
  }
  if (records.length !== 4 || records.some(record => record?.status !== 'PASS')) {
    return { status: 'BLOCKED', reason: 'INCOMPLETE_REAL_CLI_EVIDENCE' }
  }

  const keys = new Set(records.map(record => `${record.lane}:${record.observer}`))
  const required = [
    'cc-desk:off',
    'cc-desk:on',
    'system-terminal:off',
    'system-terminal:on',
  ]
  if (keys.size !== 4 || required.some(key => !keys.has(key))) {
    return { status: 'BLOCKED', reason: 'INCOMPLETE_REAL_CLI_EVIDENCE' }
  }

  const targetKey = targetFingerprint(records[0])
  if (records.some(record => targetFingerprint(record) !== targetKey)) {
    return { status: 'FAIL', reason: 'COMPARISON_TARGET_MISMATCH' }
  }

  const fixtureKey = fixtureFingerprint(records[0])
  if (records.some(record => fixtureFingerprint(record) !== fixtureKey)) {
    return { status: 'FAIL', reason: 'COMPARISON_FIXTURE_MISMATCH' }
  }

  for (const lane of ['cc-desk', 'system-terminal']) {
    const pair = pairByLane(records, lane)
    if (exactObserverPromptChanged(pair)) {
      return { status: 'FAIL', reason: 'OBSERVER_CHANGED_ORACLE' }
    }
  }

  for (const record of records) {
    const validation = validateRealCliRun(record)
    if (!validation.valid) {
      return {
        status: 'FAIL',
        reason: `INVALID_REAL_CLI_EVIDENCE:${validation.reason}`,
      }
    }
  }

  const cli = cliKindFrom(records[0])
  if (!['claude', 'codex'].includes(cli)) {
    return { status: 'FAIL', reason: 'CLI_KIND_REQUIRED' }
  }
  return { status: 'PASS', cli, caseId: 'NATIVE-63' }
}

export function certifyD20(input) {
  const results = {
    claude: certifyCliComparison(input?.claude),
    codex: certifyCliComparison(input?.codex),
  }

  const failedClis = Object.entries(results)
    .filter(([, result]) => result.status === 'FAIL')
    .map(([cli]) => cli)
  if (failedClis.length > 0) {
    return {
      status: 'FAIL',
      reason: 'REAL_CLI_COMPARISON_FAILED',
      failedClis,
    }
  }

  const blockedClis = Object.entries(results)
    .filter(([, result]) => result.status !== 'PASS')
    .map(([cli]) => cli)
  if (blockedClis.length > 0) {
    return {
      status: 'BLOCKED',
      reason: 'REAL_CLI_EVIDENCE_INCOMPLETE',
      blockedClis,
    }
  }

  return { status: 'PASS' }
}
