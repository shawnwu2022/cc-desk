import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import test from 'node:test'

function read(relative) {
  return readFileSync(fileURLToPath(new URL(`../../${relative}`, import.meta.url)), 'utf8')
}

test('D29_EvidenceIngest_RequiresProtectedSelfHostedProvenance_01', () => {
  const workflow = read('.github/workflows/native-cli-installed-evidence.yml')
  assert.match(workflow, /runs-on: \[self-hosted, linux, native-cli-evidence-ingest\]/)
  assert.match(workflow, /environment: native-cli-certification-ingest/)
  assert.match(workflow, /NATIVE_CLI_EVIDENCE_DROP_ROOT/)
  assert.match(workflow, /verify-workflow-run\.mjs/)
  assert.match(workflow, /stage-evidence-bundle\.mjs/)
  assert.match(workflow, /native-installed-evidence-\$\{\{ inputs\.candidate_sha \}\}-\$\{\{ inputs\.bundle_id \}\}/)
  assert.doesNotMatch(workflow, /OPENAI_API_KEY|ANTHROPIC_API_KEY|CODEX_API_KEY/)
})

test('D29_AcceptanceGate_DerivesEvidenceArtifactAndChecksBothRuns_02', () => {
  const workflow = read('.github/workflows/native-cli-acceptance-gate.yml')
  assert.match(workflow, /candidate-run\.json/)
  assert.match(workflow, /evidence-run\.json/)
  assert.match(workflow, /workflow-path \.github\/workflows\/release\.yml/)
  assert.match(workflow, /workflow-path \.github\/workflows\/native-cli-installed-evidence\.yml/)
  assert.match(workflow, /native-installed-evidence-\$\{\{ inputs\.candidate_sha \}\}-\$\{\{ inputs\.bundle_id \}\}/)
  assert.doesNotMatch(workflow, /evidence_artifact/)
})

test('D29_Canary_IsCrossPlatformCredentialFreeAndNonCertifying_03', () => {
  const workflow = read('.github/workflows/native-cli-canary.yml')
  assert.match(workflow, /os: \[ubuntu-22\.04, macos-14, windows-2022\]/)
  assert.match(workflow, /cli: \[claude, codex\]/)
  assert.match(workflow, /lane: \[pinned, latest-stable\]/)
  assert.match(workflow, /certificationStatus remains NOT_RUN/)
  assert.doesNotMatch(workflow, /\$\{\{\s*secrets\./)
})
