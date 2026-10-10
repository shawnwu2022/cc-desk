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
