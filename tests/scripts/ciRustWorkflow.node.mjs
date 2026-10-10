import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const workflow = fs.readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
const job = id => workflow.match(new RegExp(`^  ${id}:\\n([\\s\\S]*?)(?=^  [a-z][a-z-]*:|$(?![\\s\\S]))`, 'm'))?.[1];
const gateUrl = new URL('../../scripts/ci-rust-job-gate.mjs', import.meta.url);

test('RustDag_Producer_001: shards depend only on the complete source-bound producer', () => {
  const producer = job('rust-compile'), shards = job('rust-tests');
  assert.ok(producer && shards);
  assert.match(shards, /^    needs: rust-compile$/m);
  assert.doesNotMatch(producer, /cargo (fmt|clippy|build --locked)/);
  assert.match(producer, /windows-rust-tests\.ps1 -Action Compile/);
  assert.match(producer, /cargo test --locked --doc/);
  assert.match(producer, /windows-ordinary-preflight\.mjs/);
  assert.match(producer, /windows-rust-shard-runner\.mjs plan/);
  assert.match(producer, /windows-rust-bundle-\$\{\{ github\.sha \}\}-\$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}/);
  assert.ok(producer.indexOf('windows-ordinary-preflight.mjs') < producer.indexOf('Share only verified'));
  assert.match(shards, /shard: \[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15\]/);
  assert.match(shards, /timeout-minutes: 30/);
  assert.ok(shards.indexOf('CI_RUST_JOB_START_MS') < shards.indexOf('actions/checkout'), 'the shared budget includes checkout/download setup');
  assert.match(shards, /--job-start-ms "\$\{\{ env\.CI_RUST_JOB_START_MS \}\}"/);
  assert.match(shards, /name: Preserve original selection, execution and timing even on failure\n        if: \$\{\{ always\(\) \}\}/);
});

test('RustDag_Static_002: entry contracts, strict lint and actual loader remain independent mandatory work', () => {
  const staticJob = job('rust-static');
  assert.ok(staticJob, 'static work must not delay the artifact producer');
  assert.match(staticJob, /runs-on: windows-2022/);
  assert.doesNotMatch(staticJob, /^    needs:/m);
  for (const original of ['tests/scripts/windowsRustTests.ps1', 'cargo fmt --check', 'cargo clippy --locked --all-targets', '-D warnings', "'--check-conpty'", '$value.ptyLifecycle -ne $true', '$value.backend -ne \'bundled\'']) {
    assert.ok(staticJob.includes(original), `retained static/actual-loader contract: ${original}`);
  }
  assert.match(staticJob, /cargo build --locked/);
  assert.match(staticJob, /WaitForExit\(30000\)/);
  assert.match(staticJob, /dtolnay\/rust-toolchain@1\.98\.1/);
  assert.match(staticJob, /node-version: 22/);
});

test('RustDag_Aggregate_003: existing required check gates producer, static and every shard before coverage', () => {
  const aggregate = job('rust');
  assert.match(aggregate, /name: Rust checks/);
  assert.match(aggregate, /needs: \[rust-compile, rust-static, rust-tests\]/);
  assert.match(aggregate, /if: \$\{\{ always\(\) \}\}/);
  for (const [variable, id] of [['COMPILE_RESULT', 'rust-compile'], ['STATIC_RESULT', 'rust-static'], ['SHARD_RESULT', 'rust-tests']]) {
    assert.ok(aggregate.includes(`${variable}: \${{ needs.${id}.result }}`));
  }
  assert.ok(aggregate.indexOf('node scripts/ci-rust-job-gate.mjs') < aggregate.indexOf('actions/download-artifact'));
  assert.match(workflow, /name: Frontend checks/);
  assert.match(workflow, /name: Disposable roundtrip compile-only policy \(no native acceptance\)/);
});

test('RustDag_FailClosed_004: failures, cancellation, skips and missing results never pass the executable gate', async () => {
  assert.ok(fs.existsSync(gateUrl), 'the aggregator must execute a tested strict result gate');
  const { requireRustJobSuccess } = await import(gateUrl);
  assert.doesNotThrow(() => requireRustJobSuccess({ compile: 'success', static: 'success', shards: 'success' }));
  for (const key of ['compile', 'static', 'shards']) for (const result of ['failure', 'cancelled', 'skipped', '', undefined, 'Success']) {
    const values = { compile: 'success', static: 'success', shards: 'success', [key]: result };
    assert.throws(() => requireRustJobSuccess(values), new RegExp(key));
    const child = spawnSync(process.execPath, [gateUrl.pathname], { encoding: 'utf8', env: { ...process.env, COMPILE_RESULT: values.compile ?? '', STATIC_RESULT: values.static ?? '', SHARD_RESULT: values.shards ?? '' } });
    assert.notEqual(child.status, 0);
  }
});

test('RustDag_Cache_005: same compatible defaults share dependency cache and clear all source-bound evidence', () => {
  for (const id of ['rust-compile', 'rust-static']) {
    const text = job(id);
    assert.match(text, /shared-key: rust-compile/);
    assert.match(text, /cache-workspace-crates: false/);
    assert.match(text, /ci-build-metrics-\$\{\{ github\.job \}\}-\$\{\{ github\.sha \}\}-\$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}/);
    assert.match(text, /'src-tauri\/target\/ci-build-metrics'/);
  }
  assert.match(job('rust-static'), /save-if: false/);
  assert.match(workflow, /CARGO_PROFILE_DEV_DEBUG: '0'/);
  assert.match(workflow, /CARGO_PROFILE_TEST_DEBUG: '0'/);
});
