import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const metricsUrl = new URL('../../scripts/ci-build-metrics.mjs', import.meta.url);
const artifact = (name, fresh, features = []) => ({ reason: 'compiler-artifact', target: { name, kind: ['lib'] }, fresh, features, filenames: ['C:/PRIVATE/name.rlib'], manifest_path: 'C:/PRIVATE/Cargo.toml' });

test('BuildMetrics_Fresh_001: compiler messages measure actual fresh/rebuilt artifacts and build scripts', async () => {
  assert.ok(fs.existsSync(metricsUrl), 'fresh/rebuilt metrics must be derived from actual Cargo JSON');
  const { summarizeCargoMessages } = await import(metricsUrl);
  const records = [artifact('cc_desk', false), artifact('tauri', true), artifact('windows', false), artifact('libsqlite3_sys', true), { reason: 'build-script-executed', env: [['SECRET', 'PRIVATE']] }, { reason: 'build-finished', success: true }];
  const result = summarizeCargoMessages(records.map(r => JSON.stringify(r)).join('\n'));
  assert.deepEqual(result.counts, { fresh: 2, rebuilt: 2, buildScripts: 1 });
  assert.equal(result.buildFinishedSuccess, true);
  assert.deepEqual(result.crates.find(c => c.name === 'cc_desk'), { name: 'cc_desk', kinds: ['lib'], features: [], fresh: 0, rebuilt: 1 });
  assert.equal(JSON.stringify(result).includes('PRIVATE'), false, 'paths, diagnostics and build-script environment are not copied');
});

test('BuildMetrics_Incomplete_002: partial or failed builds retain counts without successful-build claims', async () => {
  assert.ok(fs.existsSync(metricsUrl));
  const { summarizeCargoMessages } = await import(metricsUrl);
  const partial = summarizeCargoMessages(JSON.stringify(artifact('cc_desk', false)));
  assert.equal(partial.buildFinishedSuccess, null);
  assert.equal(partial.counts.rebuilt, 1);
  assert.equal(summarizeCargoMessages(JSON.stringify({ reason: 'build-finished', success: false })).buildFinishedSuccess, false);
  for (const text of ['not json', JSON.stringify(artifact('tauri', 'true')), JSON.stringify({ reason: 'build-finished', success: true }) + '\n' + JSON.stringify({ reason: 'build-finished', success: true })]) {
    assert.throws(() => summarizeCargoMessages(text), /JSON|fresh|finished/);
  }
});

test('BuildMetrics_Cache_003: exact hit does not claim reuse and non-exact output cannot distinguish fallback from miss', async () => {
  assert.ok(fs.existsSync(metricsUrl));
  const { classifyCacheHit } = await import(metricsUrl);
  assert.equal(classifyCacheHit('true'), 'exact');
  assert.equal(classifyCacheHit('false'), 'non-exact-or-miss');
  assert.equal(classifyCacheHit(''), 'unknown');
  assert.throws(() => classifyCacheHit('yes'), /cache/);
});

test('BuildMetrics_FailurePrivacy_004: CLI failure never echoes raw Cargo excerpts or absolute checkout paths', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ccdesk-private-metrics-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const input = path.join(directory, 'cargo.jsonl'), secret = 'FAKE_PRIVATE_TOKEN_FOR_REGRESSION';
  fs.writeFileSync(input, `{"reason":"${secret}","broken": }`);
  for (const filename of [input, path.join(directory, 'missing-input.jsonl')]) {
    const child = spawnSync(process.execPath, [metricsUrl.pathname, 'cargo', 'tests', filename, '0.1', '0'], { encoding: 'utf8' });
    assert.notEqual(child.status, 0);
    assert.equal(child.stdout, '');
    assert.equal(child.stderr.includes(secret), false);
    assert.equal(child.stderr.includes(directory), false);
    assert.match(child.stderr, /^Build metrics: recording failed; raw paths and build output withheld\n$/);
  }
});
