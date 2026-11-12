import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { isolatedEnvironment, requireMethod, requireSession, metadataResult } from '../../scripts/native-cli/metadata-lifecycle.mjs'

test('MetadataLifecycle_EnvironmentNeverInheritsCredentials_001', () => {
  const root = mkdtempSync(join(tmpdir(), 'metadata-environment-'))
  try {
    const env = isolatedEnvironment(root, { SystemRoot: 'C:\\Windows', PATH: 'unsafe', HOME: 'real-home', OPENAI_API_KEY: 'never-forward', NODE_OPTIONS: '--require secret', OPENCODE_CONFIG_CONTENT: 'real-config' })
    assert.equal(env.HOME, join(root, 'home'))
    assert.equal(env.CODEX_HOME, join(root, 'codex-home'))
    assert.equal(env.OPENAI_API_KEY, undefined)
    assert.equal(env.NODE_OPTIONS, undefined)
    assert.equal(env.OPENCODE_CONFIG_CONTENT, undefined)
    assert.equal(env.PATH, 'C:\\Windows\\System32')
    assert.equal(env.OPENCODE_DISABLE_MODELS_FETCH, 'true')
    assert.equal(env.CLAUDE_CONFIG_DIR, join(root, 'claude-home'))
  } finally { rmSync(root, { recursive: true, force: true }) }
})
test('MetadataLifecycle_OnlyMetadataMethodsCanBeSent_002', () => {
  for (const method of ['initialize', 'initialized', 'thread/read', 'thread/list', 'thread/name/set']) assert.doesNotThrow(() => requireMethod(method))
  for (const method of ['turn/start', 'thread/start', 'thread/resume', 'account/login/start', 'thread/delete', 'exec', '']) assert.throws(() => requireMethod(method), /METHOD_FORBIDDEN/)
})
test('MetadataLifecycle_DuplicateAndWrongIdentityCannotPass_003', () => {
  const row = { id: 'fixture', name: 'title', cwd: 'fixture-project' }
  assert.doesNotThrow(() => requireSession([row], 'fixture', 'title', 'fixture-project'))
  for (const rows of [[], [row, row], [{ ...row, name: 'stale' }], [{ ...row, cwd: 'other' }]]) assert.throws(() => requireSession(rows, 'fixture', 'title', 'fixture-project'))
})
test('MetadataLifecycle_MetadataNeverClaimsFullAcceptance_004', () => {
  for (const cli of ['claude', 'codex', 'opencode']) {
    const r = metadataResult(cli, ['read', 'rename', 'reopen', 'unique-list'])
    assert.equal(r.status, 'METADATA_PASS')
    assert.equal(r.nativeLifecycleAccepted, false)
    assert.equal(r.guiAccepted, false)
    assert.equal(r.modelCalled, false)
    assert.equal(r.installerRoundtripAccepted, false)
    assert.equal(r.cli, cli)
  }
  assert.throws(() => metadataResult('codex', ['read']), /INCOMPLETE/)
  assert.throws(() => metadataResult('other', ['read', 'rename', 'reopen', 'unique-list']), /CLI_REQUIRED/)
})

test('MetadataLifecycle_HttpResponsesAreBoundedWhileReading_005', async () => {
  const { readBoundedJson } = await import('../../scripts/native-cli/metadata-lifecycle.mjs')
  assert.deepEqual(await readBoundedJson(new Response('{"ok":true}'), 32), { ok: true })
  let cancelled = false
  const response = new Response(new ReadableStream({ pull(controller) { controller.enqueue(new Uint8Array(40)) }, cancel() { cancelled = true } }))
  await assert.rejects(readBoundedJson(response, 32), /HTTP_OUTPUT_LIMIT/)
  assert.equal(cancelled, true)
})

test('MetadataLifecycle_HelperCannotImportSdkInDefaultEnvironment_006', async () => {
  const { spawnSync } = await import('node:child_process')
  const { writeFileSync, existsSync } = await import('node:fs')
  const { fileURLToPath } = await import('node:url')
  const root = mkdtempSync(join(tmpdir(), 'metadata-helper-'))
  try {
    const marker = join(root, 'imported'), sdk = join(root, 'sdk.mjs')
    writeFileSync(sdk, `import fs from 'node:fs';fs.writeFileSync(${JSON.stringify(marker)},'unexpected import');`)
    const child = spawnSync(process.execPath, [fileURLToPath(new URL('../../scripts/native-cli/metadata-lifecycle.mjs', import.meta.url)), '--claude-sdk', 'rename'], { env: {}, input: JSON.stringify({ sdk, id: 'fixture', title: 'title', project: root }), encoding: 'utf8', timeout: 3000 })
    assert.equal(existsSync(marker), false)
    assert.equal(child.status, 1)
    assert.match(child.stderr, /OWNED_ENTRY_REQUIRED/)
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('MetadataLifecycle_CodexFixtureIncludesOnlySyntheticUserEvent_007', async () => {
  const { codexFixtureRecords } = await import('../../scripts/native-cli/metadata-lifecycle.mjs')
  const rows = codexFixtureRecords({ project: '/fixture', version: '0.167.0' }, 'fixture-id', '2026-11-12T00:00:00Z')
  assert.equal(rows[0].type, 'session_meta')
  assert.equal(rows[0].payload.source, 'cli')
  assert.equal(rows[1].type, 'event_msg')
  assert.equal(rows[1].payload.type, 'user_message')
  assert.equal(rows.length, 2)
})

test('MetadataLifecycle_WindowsIdentityDefaultsAreExplicitAndSynthetic_008', () => {
  const root = mkdtempSync(join(tmpdir(), 'metadata-windows-env-'))
  try {
    const env = isolatedEnvironment(root, { SystemRoot: 'C:\\Windows', USERNAME: 'real-user', USERDOMAIN: 'real-domain' })
    for (const key of ['HOMEDRIVE', 'HOMEPATH', 'SYSTEMDRIVE', 'USERDOMAIN', 'USERNAME']) assert.equal(typeof env[key], 'string', key)
    assert.equal(env.USERNAME, 'metadata-fixture')
    assert.equal(env.USERDOMAIN, 'metadata-fixture')
    assert.equal(env.HOMEDRIVE + env.HOMEPATH, env.HOME)
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('MetadataLifecycle_OpenCodeV2ShapesRetainIdentity_009', async () => {
  const { opencodeContract } = await import('../../scripts/native-cli/metadata-lifecycle.mjs')
  const v1 = opencodeContract('1.9.0'), v2 = opencodeContract('2.2.6')
  assert.equal(v1.route, '/session')
  assert.equal(v2.route, '/api/session')
  assert.deepEqual(v1.create('/fixture'), {})
  assert.deepEqual(v2.create('/fixture'), { location: { directory: '/fixture' } })
  assert.deepEqual(v2.session({ data: { id: 'ses_fixture', title: 'title', location: { directory: '/fixture' } } }), { id: 'ses_fixture', name: 'title', cwd: '/fixture' })
  assert.deepEqual(v1.session({ id: 'ses_fixture', title: 'title', directory: '/fixture' }), { id: 'ses_fixture', name: 'title', cwd: '/fixture' })
  assert.throws(() => opencodeContract('3.0.0'), /API_VERSION_UNSUPPORTED/)
})

test('MetadataLifecycle_RpcRejectsErrorsWrongIdsAndActiveRequests_010', async () => {
  const { rpcResult } = await import('../../scripts/native-cli/metadata-lifecycle.mjs')
  assert.deepEqual(rpcResult({ id: 1, result: { thread: {} } }, 1), { thread: {} })
  assert.equal(rpcResult({ method: 'thread/name/updated', params: {} }, 1), undefined)
  for (const frame of [{ id: 1, error: { message: 'error' } }, { id: 2, result: {} }, { id: 1, result: null }]) assert.throws(() => rpcResult(frame, 1), /RPC_RESPONSE_INVALID/)
  assert.throws(() => rpcResult({ id: 9, method: 'item/commandExecution/requestApproval' }, 1), /SERVER_REQUEST_FORBIDDEN/)
})
