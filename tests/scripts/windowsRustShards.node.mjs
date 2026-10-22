import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { estimatedWeight, partitionNames, validatePartition, aggregateHarness, validateShardExecution, validateRustShardRun } from '../../scripts/windows-rust-shards.mjs';
import { parseLibtestListing, parseLibtestResult } from '../../scripts/windows-native-validation.mjs';

const full = ['context_slow', 'bundle_slow', 'ordinary_new', 'worker'].map(name => ({ name, type: 'test' }));
const h = { identity: { name: 'fixture', kind: 'lib' }, full, selected: full.map(t => t.name), ignored: ['worker'], excluded: [] };
const listing = names => names.map(name => `${name}: test`).join('\n') + `\n${names.length} tests, 0 benchmarks\n`;
// 实测只用于显式候选；未测新名称仍保守估计，不能改变默认库存或准入。
test('WindowsRustShards_ExplicitWeights_010', () => {
  const names = ['fixture::FastA', 'fixture::FastB', 'fixture::Slow'];
  const original = partitionNames(names, 2);
  const candidate = partitionNames(names, 2, { 'fixture::Slow': 10 });
  assert.deepEqual(candidate, [['fixture::Slow'], ['fixture::FastA', 'fixture::FastB']]);
  assert.deepEqual(partitionNames([...names].reverse(), 2, { 'fixture::Slow': 10 }), candidate);
  assert.deepEqual(partitionNames(names, 2, {}), original);
  assert.equal(new Set(candidate.flat()).size, names.length);
  assert.deepEqual(new Set(candidate.flat()), new Set(names));
  for (const weights of [null, [], { stale: 1 }, { 'fixture::Slow': 0 }, { 'fixture::Slow': -1 },
    { 'fixture::Slow': NaN }, { 'fixture::Slow': Infinity }, { 'fixture::Slow': '10' },
    { 'fixture::Slow': 1201 }, Object.create({ 'fixture::Slow': 10 })]) {
    assert.throws(() => partitionNames(names, 2, weights), /weight/);
  }
});

test('WindowsRustShards_MeasuredPlacement_008: real nested bundle and retained-custody names receive heavy placement', () => {
  const bundle = 'version_history::windows::context::bundle_restore::tests::BundleReturn_RestorePresentImage_002';
  const context = 'tests::version_history_context_windows::HistoryContext_ReturnMissingImage_036';
  const custody = 'version_history::windows::context::switching::preinstall_custody_tests::HistoryPreinstallCustody_RetainedContextReturn_002';
  assert.equal(estimatedWeight(bundle), estimatedWeight(context));
  assert.ok(estimatedWeight(custody) > estimatedWeight(bundle));
  const shards = partitionNames([custody, bundle, context], 3);
  assert.ok(shards.every(names => names.length === 1));
});

// 候选只供离线比较；默认布局仍逐项重现原记录，未获净收益证据前不能直接采纳。
test('WindowsRustShards_TailCandidate_009', async () => {
  const observation = JSON.parse(fs.readFileSync(new URL('../../docs/ci-shard-observations-2026-10-19.json', import.meta.url), 'utf8'));
  const names = observation.shards.flatMap(shard => shard.names);
  const slow = 'version_history::windows::context::bundle_restore::tests::BundleReturn_RestorePresentImage_002';
  const { compareTailCandidate } = await import('../../scripts/windows-rust-shard-placement.mjs');
  const { candidate: shards, summary } = compareTailCandidate(observation);
  assert.deepEqual(shards.find(shard => shard.includes(slow)), [slow]);
  assert.equal(shards.length, 16);
  assert.deepEqual(new Set(shards.flat()), new Set(names));
  assert.equal(shards.flat().length, names.length);
  assert.equal(summary.advisoryOnly, true);
  assert.equal(summary.originalMaxHeuristicLoad, 1333);
  assert.equal(summary.candidateMaxHeuristicLoad, 1414);
  for (const shard of observation.shards) assert.deepEqual(partitionNames(names, 16)[shard.index], shard.names);
  const drift = structuredClone(observation); drift.shards[0].names.pop();
  assert.throws(() => compareTailCandidate(drift), /baseline/);
});
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
    const executable = path.join(temp, process.platform === 'win32' ? 'fixture.exe' : 'fixture');
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

test('WindowsRustShards_RunProof_007: all sixteen same-source receipts required, including empty slices', () => {
  const context = { sourceSha: 'a'.repeat(40), runId: '123', runAttempt: 1 };
  const artifactName = `windows-rust-bundle-${context.sourceSha}-123-1`;
  const harness = aggregateHarness(h, [slice(0, ['context_slow', 'worker']), slice(1, ['bundle_slow', 'ordinary_new'])], parseLibtestListing, parseLibtestResult);
  const binding = { ...context, planHash: 'b'.repeat(64), artifactName };
  const report = { ...context, host: { jobQuerySucceeded: true, inJob: true }, harnesses: [harness],
    rustShardRun: { ...binding, policy: 'same-source-compiled-rust-shards-v1', shardCount: 16, compiler: { rustcVerbose: 'rustc 1.98.1 (fixture)' }, bundleFiles: [{path:'src-tauri/target/debug/deps/fixture.exe', bytes:1,sha256:'c'.repeat(64)}],
      shards: Array.from({length:16}, (_, index) => ({ ...binding, index, host: {jobQuerySucceeded:true,inJob:true},completed:true,exitCode:0,durationSeconds:1,harnesses:[{identity:h.identity,names: index===0 ? ['context_slow','worker']:index===1?['bundle_slow','ordinary_new']:[],executed:index<2}]})) } };
  validateRustShardRun(report, context);
  for (const mutate of [r=>r.rustShardRun.shards.pop(),r=>r.rustShardRun.shards[7].index=0,r=>r.rustShardRun.shards[0].runAttempt=2,r=>r.rustShardRun.shards[0].completed=false,r=>r.rustShardRun.shards[7].harnesses[0].executed=true,r=>r.rustShardRun.compiler.rustcVerbose='rustc 1.97.0 (fixture)']) {
    const broken=structuredClone(report);mutate(broken);assert.throws(()=>validateRustShardRun(broken,context),/shard|compiler|binding/);
  }
});
