import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { partitionNames, validatePartition, aggregateHarness, validateShardExecution, validateRustShardRun } from '../../scripts/windows-rust-shards.mjs';
import { parseLibtestListing, parseLibtestResult } from '../../scripts/windows-native-validation.mjs';

const full = ['context_slow', 'bundle_slow', 'ordinary_new', 'worker'].map(name => ({ name, type: 'test' }));
const h = { identity: { name: 'fixture', kind: 'lib' }, full, selected: full.map(t => t.name), ignored: ['worker'], excluded: [] };
const listing = names => names.map(name => `${name}: test`).join('\n') + `\n${names.length} tests, 0 benchmarks\n`;
function slice(index, names, fail = false) {
  const ignored = names.filter(n => n === 'worker').length;
  const result = { exitCode: fail ? 101 : 0, passed: names.length - ignored - Number(fail), failed: Number(fail), ignored, measured: 0, filteredOut: full.length - names.length };
  return { index, names, listing: listing(names), output: names.map(n => `test ${n} ... ${n === 'worker' ? 'ignored, supervised only' : fail ? 'FAILED' : 'ok'}`).join('\n') + `\ntest result: ${fail ? 'FAILED' : 'ok'}. ${result.passed} passed; ${result.failed} failed; ${result.ignored} ignored; 0 measured; ${result.filteredOut} filtered out; finished in 0.1s\n`, result, durationSeconds: 0.1 };
}
test('WindowsRustShards_Inventory_001: deterministic partition includes new names and workers exactly once', () => {
  const names = h.selected;
  const shards = partitionNames(names, 3);
  assert.deepEqual(partitionNames([...names].reverse(), 3), shards);
  assert.deepEqual(new Set(shards.flat()), new Set(names));
  assert.equal(shards.flat().length, names.length);
  validatePartition(names, shards);
});
test('WindowsRustShards_Drift_002: duplicate, missing, invented and zero-shard inventories rejected', () => {
  for (const shards of [[['worker', 'worker']], [['ordinary_new']], [[...h.selected, 'invented']], []]) {
    assert.throws(() => validatePartition(h.selected, shards), /partition|duplicate|shard/);
  }
  assert.throws(() => partitionNames(['same', 'same'], 3), /duplicate/);
  assert.throws(() => partitionNames(h.selected, 0), /shard/);
});
test('WindowsRustShards_Aggregate_003: raw exact listing and results reconcile all original ignored reasons', () => {
  const slices = [slice(0, ['context_slow', 'worker']), slice(1, ['bundle_slow', 'ordinary_new'])];
  const merged = aggregateHarness(h, slices, parseLibtestListing, parseLibtestResult);
  assert.equal(merged.result.passed, 3);
  assert.equal(merged.result.ignored, 1);
  assert.equal(merged.result.filteredOut, 0);
  assert.equal(merged.defaultIgnored[0].reason, 'supervised only');
  validateShardExecution(merged, merged.execution, parseLibtestListing, parseLibtestResult);
});
test('WindowsRustShards_Failure_004: failure, truncated output and mismatched raw selection never green', () => {
  const slices = [slice(0, ['context_slow', 'worker']), slice(1, ['bundle_slow', 'ordinary_new'])];
  assert.throws(() => aggregateHarness(h, [slices[0], slice(1, slices[1].names, true)], parseLibtestListing, parseLibtestResult), /failed/);
  const broken = structuredClone(slices); broken[0].output = 'interrupted';
  assert.throws(() => aggregateHarness(h, broken, parseLibtestListing, parseLibtestResult), /summary/);
  const wrong = structuredClone(slices); wrong[1].listing = listing(['bundle_slow']);
  assert.throws(() => aggregateHarness(h, wrong, parseLibtestListing, parseLibtestResult), /listing/);
  assert.throws(() => aggregateHarness(h, [slices[0], slices[0], slices[1]], parseLibtestListing, parseLibtestResult), /duplicate/);
});
test('WindowsRustShards_Evidence_005: canonical counts cannot conceal tampered shard execution', () => {
  const merged = aggregateHarness(h, [slice(0, ['context_slow', 'worker']), slice(1, ['bundle_slow', 'ordinary_new'])], parseLibtestListing, parseLibtestResult);
  assert.throws(() => validateShardExecution(merged, merged.execution.replace('2 passed;', '9 passed;'), parseLibtestListing, parseLibtestResult), /count|result/);
  assert.throws(() => validateShardExecution(merged, merged.execution.replace('ordinary_new: test', 'invented: test'), parseLibtestListing, parseLibtestResult), /listing/);
});

test('WindowsRustShards_StableHarness_006: exact multi-name arguments exercise real libtest without substring matches', t => {
  const available = spawnSync('rustc', ['--version'], { encoding: 'utf8' });
  if (available.error?.code === 'ENOENT') { t.skip('local rustc absent; Windows contract CI exercises real harness'); return; }
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'ccdesk-shard-libtest-'));
  try {
    fs.writeFileSync(path.join(temp, 'fixture.rs'), '#[test] fn one(){} #[test] fn one_suffix(){panic!("substring must not run")} #[test] fn two(){} #[test] #[ignore="supervised only"] fn worker(){}');
    const executable = path.join(temp, 'fixture');
    const compiled = spawnSync('rustc', ['--test', path.join(temp, 'fixture.rs'), '-o', executable], { encoding: 'utf8' });
    assert.equal(compiled.status, 0, compiled.stderr);
    const args = ['one', 'two', 'worker', '--exact'];
    const list = spawnSync(executable, [...args, '--list', '--format', 'pretty'], { encoding: 'utf8' });
    const run = spawnSync(executable, args, { encoding: 'utf8' });
    assert.equal(list.status, 0, list.stderr); assert.equal(run.status, 0, run.stderr);
    assert.deepEqual(parseLibtestListing(list.stdout).map(t => t.name), ['one', 'two', 'worker']);
    assert.deepEqual(parseLibtestResult(run.stdout), { passed: 2, failed: 0, ignored: 1, measured: 0, filteredOut: 1 });
  } finally { fs.rmSync(temp, { recursive: true, force: true }); }
});

test('WindowsRustShards_RunProof_007: all eight same-source receipts required, including empty slices', () => {
  const context = { sourceSha: 'a'.repeat(40), runId: '123', runAttempt: 1 };
  const artifactName = `windows-rust-bundle-${context.sourceSha}-123-1`;
  const harness = aggregateHarness(h, [slice(0, ['context_slow', 'worker']), slice(1, ['bundle_slow', 'ordinary_new'])], parseLibtestListing, parseLibtestResult);
  const binding = { ...context, planHash: 'b'.repeat(64), artifactName };
  const report = { ...context, host: { jobQuerySucceeded: true, inJob: true }, harnesses: [harness],
    rustShardRun: { ...binding, policy: 'same-source-compiled-rust-shards-v1', shardCount: 8, compiler: { rustcVerbose: 'rustc 1.98.1 (fixture)' }, bundleFiles: [{path:'src-tauri/target/debug/deps/fixture.exe', bytes:1,sha256:'c'.repeat(64)}],
      shards: Array.from({length:8}, (_, index) => ({ ...binding, index, host: {jobQuerySucceeded:true,inJob:true},completed:true,exitCode:0,durationSeconds:1,harnesses:[{identity:h.identity,names: index===0 ? ['context_slow','worker']:index===1?['bundle_slow','ordinary_new']:[],executed:index<2}]})) } };
  validateRustShardRun(report, context);
  for (const mutate of [r=>r.rustShardRun.shards.pop(),r=>r.rustShardRun.shards[7].index=0,r=>r.rustShardRun.shards[0].runAttempt=2,r=>r.rustShardRun.shards[0].completed=false,r=>r.rustShardRun.shards[7].harnesses[0].executed=true,r=>r.rustShardRun.compiler.rustcVerbose='rustc 1.97.0 (fixture)']) {
    const broken=structuredClone(report);mutate(broken);assert.throws(()=>validateRustShardRun(broken,context),/shard|compiler|binding/);
  }
});
