import { createHash } from 'node:crypto'
import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { describe, expect, it } from 'vitest'

const evidencePath = resolve(process.cwd(), 'scripts/native-cli/real-cli-evidence.mjs')
const sha256 = 'a'.repeat(64)
const otherSha256 = 'b'.repeat(64)
const commitSha = 'c'.repeat(40)
const payload = 'd20-nonce-123\n你好\n<pasted_content id="literal">keep me</pasted_content id="literal">\n'
const payloadBase64 = Buffer.from(payload, 'utf8').toString('base64')
const payloadSha256 = createHash('sha256').update(Buffer.from(payloadBase64, 'base64')).digest('hex')

async function loadEvidence() {
  expect(existsSync(evidencePath), 'real-cli-evidence.mjs must exist').toBe(true)
  return import(`${pathToFileURL(evidencePath).href}?case=${Date.now()}-${Math.random()}`)
}

function target(cli: 'claude' | 'codex') {
  return {
    targetId: 'windows-11-x64-d20-fixture',
    os: 'windows',
    osBuild: '10.0.26100',
    arch: 'x86_64',
    cli: {
      kind: cli,
      version: cli === 'codex' ? 'codex-cli-fixture-1' : 'claude-code-fixture-1',
      binarySha256: sha256,
    },
  }
}

function run(
  cli: 'claude' | 'codex',
  lane: 'cc-desk' | 'system-terminal',
  observer: 'off' | 'on',
  overrides: Record<string, unknown> = {},
) {
  const envelope = cli === 'codex'
    ? {
        session_id: 'session-d20',
        turn_id: 'turn-d20',
        cwd: 'C:\\d20\\repo',
        hook_event_name: 'UserPromptSubmit',
        prompt: payload,
      }
    : {
        session_id: 'session-d20',
        cwd: 'C:\\d20\\repo',
        hook_event_name: 'UserPromptSubmit',
        prompt: payload,
      }

  return {
    schemaVersion: 1,
    caseId: 'NATIVE-63',
    evidenceLayer: 'C',
    status: 'PASS',
    runId: `${cli}-${lane}-${observer}`,
    lane,
    observer,
    target: target(cli),
    fixture: {
      fixtureSha256: otherSha256,
      nonce: 'd20-nonce-123',
      originalText: payload,
      hostPayloadBase64: payloadBase64,
      transformId: cli === 'codex'
        ? 'codex-user-prompt-submit-v1-exact'
        : 'claude-user-prompt-submit-v1-exact',
    },
    host: lane === 'cc-desk'
      ? {
          kind: 'cc-desk',
          deskCommit: commitSha,
          xtermVersion: '5.5.0',
          webviewRuntime: 'fixture-webview',
          renderer: 'webgl',
        }
      : {
          kind: 'system-terminal',
          terminalProgram: 'fixture-terminal',
        },
    hostPayloadEvidence: lane === 'cc-desk'
      ? {
          kind: 'native-input-frame',
          bytesBase64: payloadBase64,
          sha256: payloadSha256,
          frame: {
            runId: `${cli}-native-run-${observer}`,
            generation: 7,
            inputSeq: '1',
            modeEpoch: '3',
          },
        }
      : {
          kind: 'terminal-driver-write',
          bytesBase64: payloadBase64,
          sha256: payloadSha256,
          driver: 'd20-system-terminal-driver-v1',
          writeSeq: '1',
        },
    oracle: {
      kind: 'real-user-prompt-submit',
      schemaVersion: 1,
      validation: { valid: true },
      sessionId: 'session-d20',
      turnId: cli === 'codex' ? 'turn-d20' : null,
      cwd: 'C:\\d20\\repo',
      rawEnvelope: envelope,
    },
    ...overrides,
  }
}

function comparisonRuns(cli: 'claude' | 'codex') {
  return [
    run(cli, 'cc-desk', 'off'),
    run(cli, 'cc-desk', 'on'),
    run(cli, 'system-terminal', 'off'),
    run(cli, 'system-terminal', 'on'),
  ]
}

describe('D20 real CLI certification evidence', () => {
  it('D20_Evidence_ModuleExists_00', async () => {
    await loadEvidence()
  })

  it('D20_Evidence_PassRequiresExactRealTargetIdentity_01', async () => {
    const { validateRealCliRun } = await loadEvidence()
    expect(validateRealCliRun(run('codex', 'cc-desk', 'off'))).toEqual({ valid: true })

    const missingHash = run('codex', 'cc-desk', 'off')
    ;(missingHash.target as { cli: Record<string, unknown> }).cli.binarySha256 = null
    expect(validateRealCliRun(missingHash)).toEqual({
      valid: false,
      reason: 'CLI_BINARY_IDENTITY_REQUIRED',
    })

    const missingDeskCommit = run('codex', 'cc-desk', 'off')
    ;(missingDeskCommit.host as Record<string, unknown>).deskCommit = null
    expect(validateRealCliRun(missingDeskCommit)).toEqual({
      valid: false,
      reason: 'DESK_IDENTITY_REQUIRED',
    })
  })

  it('D20_Evidence_BlockedCannotMasqueradeAsPass_02', async () => {
    const { validateRealCliRun } = await loadEvidence()
    expect(validateRealCliRun({
      schemaVersion: 1,
      caseId: 'NATIVE-63',
      evidenceLayer: 'C',
      status: 'BLOCKED',
      runId: 'blocked-no-authorized-account',
      cli: 'codex',
      reason: 'AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE',
    })).toEqual({ valid: true })

    const forged = run('codex', 'cc-desk', 'off', {
      oracle: {
        kind: 'synthetic-user-prompt-submit',
        validation: { valid: true },
      },
    })
    expect(validateRealCliRun(forged)).toEqual({
      valid: false,
      reason: 'REAL_ORACLE_REQUIRED',
    })
  })

  it('D20_Evidence_PassRequiresRawEnvelopeAndExactPayloadEvidence_03', async () => {
    const { validateRealCliRun } = await loadEvidence()

    const noEnvelope = run('codex', 'cc-desk', 'off')
    ;(noEnvelope.oracle as Record<string, unknown>).rawEnvelope = null
    expect(validateRealCliRun(noEnvelope)).toEqual({
      valid: false,
      reason: 'RAW_ENVELOPE_REQUIRED',
    })

    const noPayload = run('codex', 'cc-desk', 'off')
    ;(noPayload.fixture as Record<string, unknown>).hostPayloadBase64 = null
    expect(validateRealCliRun(noPayload)).toEqual({
      valid: false,
      reason: 'HOST_PAYLOAD_REQUIRED',
    })
  })

  it('D20_Evidence_PassRequiresRecordedHostPayloadProvenance_03b', async () => {
    const { validateRealCliRun } = await loadEvidence()

    const missing = run('codex', 'cc-desk', 'off')
    ;(missing as Record<string, unknown>).hostPayloadEvidence = null
    expect(validateRealCliRun(missing)).toEqual({
      valid: false,
      reason: 'HOST_PAYLOAD_PROVENANCE_REQUIRED',
    })

    const forgedBytes = run('codex', 'cc-desk', 'off')
    ;(forgedBytes.hostPayloadEvidence as Record<string, unknown>).bytesBase64 =
      Buffer.from('different', 'utf8').toString('base64')
    expect(validateRealCliRun(forgedBytes)).toEqual({
      valid: false,
      reason: 'HOST_PAYLOAD_EVIDENCE_MISMATCH',
    })

    const forgedHash = run('codex', 'system-terminal', 'off')
    ;(forgedHash.hostPayloadEvidence as Record<string, unknown>).sha256 = otherSha256
    expect(validateRealCliRun(forgedHash)).toEqual({
      valid: false,
      reason: 'HOST_PAYLOAD_EVIDENCE_MISMATCH',
    })
  })

  it('D20_Evidence_HostPayloadProvenanceIsLaneSpecific_03c', async () => {
    const { validateRealCliRun } = await loadEvidence()

    const desk = run('codex', 'cc-desk', 'off')
    ;(desk.hostPayloadEvidence as Record<string, unknown>).kind = 'terminal-driver-write'
    expect(validateRealCliRun(desk)).toEqual({
      valid: false,
      reason: 'DESK_HOST_PAYLOAD_FRAME_REQUIRED',
    })

    const terminal = run('codex', 'system-terminal', 'off')
    ;(terminal.hostPayloadEvidence as Record<string, unknown>).kind = 'native-input-frame'
    expect(validateRealCliRun(terminal)).toEqual({
      valid: false,
      reason: 'SYSTEM_TERMINAL_WRITE_EVIDENCE_REQUIRED',
    })
  })

  it('D20_Evidence_CodexRequiresTurnIdentity_04', async () => {
    const { validateRealCliRun } = await loadEvidence()
    const value = run('codex', 'cc-desk', 'off')
    ;(value.oracle as Record<string, unknown>).turnId = null
    expect(validateRealCliRun(value)).toEqual({
      valid: false,
      reason: 'CODEX_TURN_ID_REQUIRED',
    })
  })

  it('D20_Evidence_ClaudeDoesNotInventMissingTurnIdentity_05', async () => {
    const { validateRealCliRun } = await loadEvidence()
    const value = run('claude', 'cc-desk', 'off')
    expect(validateRealCliRun(value)).toEqual({ valid: true })
    expect((value.oracle as { turnId: unknown }).turnId).toBeNull()
  })

  it('D20_Evidence_ClaudePasteWrapperIsStructuralNotBroadRegex_06', async () => {
    const { verifyPromptTransform } = await loadEvidence()
    const body = [
      'd20-nonce-123',
      'literal before',
      '<pasted_content id="literal">do not strip me</pasted_content id="literal">',
      'literal after',
      '',
    ].join('\n')
    const wrapped = [
      '<pasted_content id="outer-123">',
      body,
      '</pasted_content id="outer-123">',
    ].join('\n')

    expect(verifyPromptTransform(
      'claude-user-prompt-submit-pasted-content-v1',
      body,
      wrapped,
    )).toEqual({ valid: true })

    expect(verifyPromptTransform(
      'claude-user-prompt-submit-pasted-content-v1',
      body,
      wrapped.replace(
        '</pasted_content id="outer-123">',
        '</pasted_content id="wrong">',
      ),
    )).toEqual({ valid: false, reason: 'CONTENT_MISMATCH' })

    expect(verifyPromptTransform(
      'claude-user-prompt-submit-pasted-content-v1',
      body,
      wrapped.replace('do not strip me', 'changed'),
    )).toEqual({ valid: false, reason: 'CONTENT_MISMATCH' })
  })

  it('D20_Evidence_ComparisonRequiresSameBinaryFixtureAndTarget_07', async () => {
    const { certifyCliComparison } = await loadEvidence()
    const good = comparisonRuns('codex')
    expect(certifyCliComparison(good)).toEqual({
      status: 'PASS',
      cli: 'codex',
      caseId: 'NATIVE-63',
    })

    const wrongBinary = comparisonRuns('codex')
    ;((wrongBinary[3].target as { cli: Record<string, unknown> }).cli).binarySha256 = otherSha256
    expect(certifyCliComparison(wrongBinary)).toEqual({
      status: 'FAIL',
      reason: 'COMPARISON_TARGET_MISMATCH',
    })

    const wrongFixture = comparisonRuns('codex')
    ;(wrongFixture[2].fixture as Record<string, unknown>).fixtureSha256 = sha256
    expect(certifyCliComparison(wrongFixture)).toEqual({
      status: 'FAIL',
      reason: 'COMPARISON_FIXTURE_MISMATCH',
    })
  })

  it('D20_Evidence_ObserverOnOffMustAgreePerLane_08', async () => {
    const { certifyCliComparison } = await loadEvidence()
    const values = comparisonRuns('claude')
    ;((values[1].oracle as { rawEnvelope: Record<string, unknown> }).rawEnvelope).prompt = 'changed by observer'
    expect(certifyCliComparison(values)).toEqual({
      status: 'FAIL',
      reason: 'OBSERVER_CHANGED_ORACLE',
    })
  })

  it('D20_Evidence_ComparisonRequiresBothLanesAndObserverPairs_09', async () => {
    const { certifyCliComparison } = await loadEvidence()
    expect(certifyCliComparison(comparisonRuns('codex').slice(0, 3))).toEqual({
      status: 'BLOCKED',
      reason: 'INCOMPLETE_REAL_CLI_EVIDENCE',
    })
  })

  it('D20_Evidence_TaskRequiresClaudeAndCodex_10', async () => {
    const { certifyD20 } = await loadEvidence()
    expect(certifyD20({
      claude: comparisonRuns('claude'),
      codex: comparisonRuns('codex'),
    })).toEqual({ status: 'PASS' })

    expect(certifyD20({
      claude: comparisonRuns('claude'),
      codex: [{
        schemaVersion: 1,
        caseId: 'NATIVE-63',
        evidenceLayer: 'C',
        status: 'BLOCKED',
        runId: 'codex-blocked',
        cli: 'codex',
        reason: 'AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE',
      }],
    })).toEqual({
      status: 'BLOCKED',
      reason: 'REAL_CLI_EVIDENCE_INCOMPLETE',
      blockedClis: ['codex'],
    })
  })
})
