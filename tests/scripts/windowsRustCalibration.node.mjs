import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';

const moduleUrl = new URL('../../scripts/windows-rust-calibration.mjs', import.meta.url);
test('RustCalibration_DefaultDisabled_001: only explicit opt-in enables the bounded advisory jobs', async () => {
  assert.ok(fs.existsSync(moduleUrl), 'one-time calibration needs an explicit disabled-default entry');
  const { calibrationEnabled } = await import(moduleUrl);
  for (const value of [undefined, '', 'false']) assert.equal(calibrationEnabled(value), false);
  assert.equal(calibrationEnabled('true'), true);
  assert.throws(() => calibrationEnabled('yes'), /opt-in/);
});

test('RustCalibration_Bounds_002: exact unique targets are split into eight groups without broad selectors', async () => {
  assert.ok(fs.existsSync(moduleUrl));
  const { calibrationGroups } = await import(moduleUrl);
  const targets = Array.from({ length: 16 }, (_, index) => `owned::Exact_${index}`);
  const groups = calibrationGroups({ schema: 1, targets });
  assert.equal(groups.length, 8);
  assert.ok(groups.every(group => group.length === 2));
  assert.deepEqual(new Set(groups.flat()), new Set(targets));
  assert.deepEqual(calibrationGroups({ schema: 1, targets: [...targets].reverse() }), groups);
  for (const input of [{ schema: 2, targets }, { schema: 1, targets: [...targets, 'extra'] }, { schema: 1, targets: [] }, { schema: 1, targets: [...targets.slice(1), targets[1]] }, { schema: 1, targets: ['--ignored'] }, { schema: 1, targets: ['outside\ncommand'] }]) {
    assert.throws(() => calibrationGroups(input), /calibration|target|schema|duplicate/);
  }
});

test('RustCalibration_Workflow_003: advisory calibration retains full gates and requires explicit PR/dispatch opt-in', () => {
  const yaml = fs.readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  assert.match(yaml, /calibrate_rust:[\s\S]*?default: false/);
  assert.match(yaml, /\[calibrate-rust\]/);
  assert.match(yaml, /max-parallel: 4/);
  assert.match(yaml, /name: Rust exact-name calibration/);
  assert.match(yaml, /timeout-minutes: 10/);
  assert.match(yaml, /calibration.*run --index \$\{\{ matrix\.group \}\}/);
  assert.match(yaml, /needs: \[rust-compile, rust-static, rust-tests\]/);
  assert.match(yaml, /cargo test --locked --doc/);
});

test('RustCalibration_MeasuredTable_004: sixteen committed weights retain the exact reviewed calibration evidence', async () => {
  const filename = new URL('../../scripts/windows-rust-timings.json', import.meta.url);
  assert.ok(fs.existsSync(filename), 'the placement head must contain actual reviewed timings');
  const value = JSON.parse(fs.readFileSync(filename, 'utf8'));
  const targets = JSON.parse(fs.readFileSync(new URL('../../scripts/windows-rust-calibration-targets.json', import.meta.url), 'utf8')).targets;
  assert.equal(value.schema, 1); assert.equal(value.kind, 'original-libtest-duration-weights-v1');
  assert.equal(value.calibration.sourceSha, 'e77f1e4a87bae43678b2881dcf4efc1f60721c52');
  assert.equal(value.calibration.headSha, '189650d30dc9d63ebccab2e05d24b11793164d47');
  assert.equal(value.calibration.runId, '38049514916'); assert.equal(value.calibration.runAttempt, 1);
  assert.equal(value.calibration.compiler.runnerOs, 'Windows');
  assert.equal(value.calibration.compiler.profileTestDebug, '0');
  assert.equal(value.calibration.nativeAcceptanceProven, false);
  assert.deepEqual(value.calibration.receipts.map(r => r.index).sort((a,b) => a-b), [0,1,2,3,4,5,6,7]);
  assert.ok(value.calibration.receipts.every(r => /^[a-f0-9]{64}$/.test(r.sha256) && Number.isSafeInteger(r.jobId)));
  assert.equal(value.weights.length, 16); assert.equal(new Set(value.weights.map(w => w.name)).size, 16);
  assert.deepEqual([...value.weights.map(w => w.name)].sort(), [...targets].sort());
  const { partitionNames } = await import('../../scripts/windows-rust-shards.mjs');
  const shards = partitionNames(targets, 16, Object.fromEntries(value.weights.map(w => [w.name, w.durationSeconds])));
  assert.ok(shards.every(s => s.length === 1), 'every originally measured test remains admitted exactly once');
});
