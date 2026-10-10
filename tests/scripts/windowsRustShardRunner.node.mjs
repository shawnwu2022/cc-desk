import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { shardFailureDiagnostics } from '../../scripts/windows-rust-shard-runner.mjs';

// 预检只在实际提升主机保留四条正向未验证；三条拒绝与清理契约始终选择。
test('RustRunner_PreflightElevation_006', async () => {
  const { ordinaryPreflightNames } = await import('../../scripts/windows-ordinary-preflight.mjs');
  const scope = JSON.parse(fs.readFileSync(new URL('../../scripts/windows-native-scope.json', import.meta.url), 'utf8'));
  assert.deepEqual(ordinaryPreflightNames(true), scope.ordinaryRequiredSelectedTests);
  assert.deepEqual(new Set(ordinaryPreflightNames(false)), new Set([...scope.ordinaryRequiredSelectedTests, ...scope.unelevatedTests]));
  assert.throws(() => ordinaryPreflightNames(undefined), /Actual elevation/, 'unknown elevation cannot silently select an unavailable range');
});

// 在子进程退出前输出已分配测试进度，诊断不得泄露原始正文。
test('RustRunner_LiveProgress_003', async t => {
  const runner = await import(moduleUrl);
  assert.equal(typeof runner.executeHarness, 'function', 'harness bodies need bounded streaming execution');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rust-live-output-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const executionLog = path.join(root, 'execution.log'), phases = [];
  const program = "process.stdout.write('test owned::one ... '); setTimeout(()=>{console.log('ok'); console.error('PRIVATE captured assertion'); console.log('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s');},100);";
  const result = await runner.executeHarness(process.execPath, ['-e', program], { root, environment: process.env, executionLog, assignedNames: ['owned::one'], timeoutMs: 5000, onProgress: phase => {
    phases.push(phase);
    if (phase.status === 'running') assert.equal(fs.readFileSync(executionLog, 'utf8').includes('test result:'), false, 'running progress must arrive before the outer summary');
  } });
  assert.deepEqual(phases.filter(p => p.name).map(p => [p.name, p.status]), [['owned::one', 'running'], ['owned::one', 'ok']]);
  assert.equal(JSON.stringify(phases).includes('PRIVATE'), false, 'live diagnostics expose assigned names and statuses only');
  assert.equal(result.exitCode, 0);
  assert.equal(result.timedOut, false);
  assert.equal(result.output, fs.readFileSync(executionLog, 'utf8'), 'the raw execution log retains the entire captured output');
  assert.ok(result.output.includes('PRIVATE captured assertion'));
});

// 外层汇总先出现也不能将仍未退出且超时的进程认定为成功。
test('RustRunner_Watchdog_004', async t => {
  const runner = await import(moduleUrl);
  assert.equal(typeof runner.executeHarness, 'function', 'harness bodies need a failing execution deadline');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rust-harness-timeout-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const executionLog = path.join(root, 'execution.log');
  const result = await runner.executeHarness(process.execPath, ['-e', "console.log('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'); process.stdout.write('test owned::pending ... '); setInterval(()=>{},1000);"], { root, environment: process.env, executionLog, assignedNames: ['owned::pending'], timeoutMs: 500 });
  assert.equal(result.timedOut, true);
  assert.notEqual(result.exitCode, 0, 'a terminal summary cannot override watchdog failure');
  assert.equal(result.outputIncomplete, true);
  assert.deepEqual(result.pendingNames, ['owned::pending']);
  assert.match(result.error, /deadline/);
  assert.ok(result.durationSeconds < 5, 'a stuck fixture must be terminated within the shortened test deadline');
  assert.equal(result.output, fs.readFileSync(executionLog, 'utf8'));
  assert.ok(result.output.includes('test result: ok.'), 'the pre-timeout raw summary is retained as evidence');
});

const moduleUrl = new URL('../../scripts/windows-rust-shard-runner.mjs', import.meta.url);
const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

async function runnerFixture(t, failureName = null, elevated = false) {
  assert.equal(fs.existsSync(moduleUrl), true, 'the compiled-artifact runner must exist');
  const { createPlan, runShard, aggregateResults, verifyBundle, bundleArtifactName } = await import(moduleUrl);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rust-shard-runner-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const files = ['src-tauri/Cargo.lock', '.github/workflows/ci.yml', 'scripts/windows-rust-tests.ps1', 'scripts/windows-rust-shard-runner.mjs', 'scripts/windows-rust-shards.mjs', 'scripts/windows-native-validation.mjs', 'scripts/windows-native-scope.json', 'scripts/windows-ordinary-preflight.mjs', 'scripts/prepare-conpty.mjs', 'src-tauri/conpty/manifest.json'];
  for (const name of files) {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.copyFileSync(path.join(repository, name), path.join(root, name));
  }
  fs.writeFileSync(path.join(root, 'src-tauri/Cargo.toml'), '[package]\nname="fixture"\nversion="1.0.0"\n');
  execFileSync('git', ['init', '--quiet', root]);
  execFileSync('git', ['-C', root, 'add', '.']);
  execFileSync('git', ['-C', root, '-c', 'user.name=Runner Test', '-c', 'user.email=runner@example.invalid', 'commit', '--quiet', '-m', 'fixture']);
  const sourceSha = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const environment = { ...process.env, GITHUB_SHA: sourceSha, GITHUB_RUN_ID: '1234', GITHUB_RUN_ATTEMPT: '2', CARGO_PROFILE_DEV_DEBUG: '0', CARGO_PROFILE_TEST_DEBUG: '0' };
  fs.mkdirSync(path.join(root, 'tools'));
  fs.writeFileSync(path.join(root, 'tools/rustc'), '#!/usr/bin/env node\nconsole.log("rustc 1.98.1\\nrelease: 1.98.1\\nhost: x86_64-pc-windows-msvc");\n', { mode: 0o755 });
  environment.PATH = path.join(root, 'tools') + path.delimiter + process.env.PATH;
  const scope = JSON.parse(fs.readFileSync(path.join(root, 'scripts/windows-native-scope.json'), 'utf8'));
  const runtimeNames = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/conpty/manifest.json'), 'utf8')).files.map(f => f.name);
  const worker = 'tests::fixture::ignored_worker';
  const integrationNames = name => failureName ? Array.from({ length: 16 }, (_, index) => `${name}_${index}`) : [name];
  const inventories = [scope.jobFreeTests.concat(scope.unelevatedTests, scope.requiredSelectedTests, scope.ordinaryRequiredSelectedTests, worker, 'ordinary::new_test'), [], integrationNames('integration::one'), integrationNames('integration::two')];
  const records = scope.harnesses.map((identity, index) => {
    const executable = path.join(root, 'src-tauri/target/debug/deps', `harness-${index}.exe`);
    fs.mkdirSync(path.dirname(executable), { recursive: true });
    const program = `#!/usr/bin/env node
if(process.cwd()!==${JSON.stringify(path.join(root, 'src-tauri'))}) { console.error('wrong original Cargo working directory'); process.exit(99); }
const fs=require('node:fs'), path=require('node:path'); for(const dir of [__dirname,path.dirname(__dirname)]) for(const name of ${JSON.stringify(runtimeNames)}) if(!fs.existsSync(path.join(dir,name))) { console.error('missing runtime beside test executable: '+dir+'/'+name); process.exit(98); }
const full=${JSON.stringify(inventories[index])}, ignored=${JSON.stringify(index === 0 ? [worker] : [])}, failureName=${JSON.stringify(failureName)}, args=process.argv.slice(2);
let selected=full; if(args.includes('--exact')) selected=full.filter(n=>args.includes(n)); else for(let i=0;i<args.length;i++) if(args[i]==='--skip') { const skip=args[++i]; selected=selected.filter(n=>n!==skip); }
if(args.includes('--ignored')) selected=selected.filter(n=>ignored.includes(n));
if(args.includes('--list')) { for(const n of selected) console.log(n+': test'); console.log(selected.length+' tests, 0 benchmarks'); }
else if(process.env.RUST_SHARD_FIXTURE_HANG==='1') { console.log('PRIVATE pending fixture assertion'); if(selected.length) process.stdout.write('test '+selected[0]+' ... '); setInterval(()=>{},1000); }
else {
  for(const n of selected) console.log('test '+n+' ... '+(ignored.includes(n)?'ignored, supervised only':n===failureName?'FAILED':'ok'));
  const count=selected.filter(n=>ignored.includes(n)).length, failed=selected.includes(failureName)?1:0;
  if(failed) console.log('\\nfailures:\\n\\n---- '+failureName+' stdout ----\\nPRIVATE fixture assertion and Debug value\\n\\nfailures:\\n    '+failureName+'\\n');
  console.log('test result: '+(failed?'FAILED':'ok')+'. '+(selected.length-count-failed)+' passed; '+failed+' failed; '+count+' ignored; 0 measured; '+(full.length-selected.length)+' filtered out; finished in 0.01s');
  if(failed) process.exitCode=101;
}
`;
    fs.writeFileSync(executable, program, { mode: 0o755 });
    return { reason: 'compiler-artifact', profile: { test: true }, manifest_path: path.join(root, 'src-tauri/Cargo.toml'), target: { name: identity.name, kind: [identity.kind] }, executable };
  });
  for (const directory of ['debug', 'debug/deps']) for (const name of runtimeNames) fs.writeFileSync(path.join(root, 'src-tauri/target', directory, name), 'compiled runtime bytes');
  fs.writeFileSync(path.join(root, 'src-tauri/target/debug/deps', 'unrelated.pdb'), 'excluded debug symbols');
  fs.writeFileSync(path.join(root, 'src-tauri/target/ci-test-artifacts.jsonl'), records.concat({ reason: 'build-finished', success: true }).map(r => JSON.stringify(r)).join('\n'));
  const bundle = path.join(root, 'src-tauri/target/ci-rust-bundle');
  fs.mkdirSync(path.join(bundle, 'logs'), { recursive: true });
  fs.writeFileSync(path.join(bundle, 'logs/doctests.log'), 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n');
  const options = { root, bundle, environment, inJob: true, elevated };
  const plan = createPlan(options);
  return { root, sourceSha, environment, bundle, runtimeNames, options, plan, runShard, aggregateResults, verifyBundle, bundleArtifactName };
}

// 编译包和分片对同一实际提升状态作精确绑定，四条正向既不执行也不忽略。
test('RustRunner_ElevatedArtifact_007', { skip: process.platform === 'win32' && 'Unix executable fixture; production binaries are Windows PE files' }, async t => {
  const { root, options, plan, runShard } = await runnerFixture(t, null, true);
  const scope = JSON.parse(fs.readFileSync(new URL('../../scripts/windows-native-scope.json', import.meta.url), 'utf8'));
  const library = plan.harnesses[0];
  assert.equal(library.excluded.length, 22);
  assert.ok(scope.unelevatedTests.every(n => library.full.some(t => t.name === n) && !library.selected.includes(n) && !library.ignored.includes(n)));
  assert.ok(scope.ordinaryRequiredSelectedTests.every(n => library.selected.includes(n)), 'denial and cleanup contracts remain selected on the elevated host');
  const result = await runShard({ ...options, index: 0, output: path.join(root, 'elevated-shard'), artifactName: plan.artifactName });
  assert.equal(result.completed, true);
  assert.equal(result.host.elevationQuerySucceeded, true);
  assert.equal(result.host.elevated, true);
  assert.ok(result.harnesses.every(h => h.names.every(n => !scope.unelevatedTests.includes(n))), 'elevated-host positives cannot appear in any executed shard selection');
});

// 超时分片保存失败回执与完整部分日志，不能进入成功覆盖聚合。
test('RustRunner_TimeoutReceipt_005', { skip: process.platform === 'win32' && 'Unix executable fixture; production binaries are Windows PE files' }, async t => {
  const { root, options, plan, runShard, aggregateResults } = await runnerFixture(t);
  const shards = path.join(root, 'timeout-shards'), output = path.join(shards, '0'), phases = [];
  const result = await runShard({ ...options, environment: { ...options.environment, RUST_SHARD_FIXTURE_HANG: '1' }, index: 0, output, artifactName: plan.artifactName, timeoutMs: 500, onPhase: phase => phases.push(phase) });
  assert.equal(result.completed, false);
  assert.notEqual(result.exitCode, 0);
  const harness = result.harnesses[0];
  assert.equal(harness.watchdog.timedOut, true);
  assert.equal(harness.watchdog.outputIncomplete, true);
  assert.deepEqual(harness.watchdog.pendingNames, harness.names);
  const raw = fs.readFileSync(path.join(output, harness.logs.execution), 'utf8');
  assert.ok(raw.includes('PRIVATE pending fixture assertion'), 'the raw failure evidence remains on disk');
  assert.equal(JSON.stringify(phases).includes('PRIVATE'), false);
  assert.deepEqual(phases.find(p => p.phase === 'execute-start').assignedNames, harness.names);
  assert.equal(phases.find(p => p.phase === 'execute-end').failureDiagnostics.complete, false);
  assert.equal(JSON.parse(fs.readFileSync(path.join(output, 'shard-result.json'), 'utf8')).completed, false);
  for (let index = 1; index < plan.shardCount; index++) fs.mkdirSync(path.join(shards, String(index)), { recursive: true });
  assert.throws(() => aggregateResults({ ...options, shards, coverage: path.join(root, 'timeout-coverage') }), /missing|incomplete/, 'a timed-out receipt cannot publish complete coverage');
});

// 使用真实文件、git checkout 和可执行进程检查十六个分片及旧版覆盖归档。
test('RustRunner_ArtifactBinding_001', { skip: process.platform === 'win32' && 'Unix executable fixture; production binaries are Windows PE files' }, async t => {
  const { root, sourceSha, environment, bundle, runtimeNames, options, plan, runShard, aggregateResults, verifyBundle, bundleArtifactName } = await runnerFixture(t);
  assert.equal(plan.shardCount, 16, 'the fixed workflow must assign sixteen shards');
  assert.equal(plan.artifactName, bundleArtifactName(sourceSha, '1234', 2));
  assert.equal(plan.harnesses[0].excluded.length, 18, 'only the reviewed Job-free inventory is excluded');
  assert.ok(plan.files.every(f => !f.path.endsWith('.pdb')), 'debug symbols must not enter the executable bundle');
  assert.ok(plan.files.some(f => f.path === 'src-tauri/target/debug/deps/OpenConsole.exe'), 'the ConPTY host must be bundled beside the libtest executable');
  assert.ok(runtimeNames.every(n => plan.files.some(f => f.path === `src-tauri/target/debug/deps/${n}`)), 'all embedded runtime manifest entries must be bundled beside the executable');
  assert.throws(() => verifyBundle({ ...options, environment: { ...environment, GITHUB_RUN_ATTEMPT: '3' } }), /binding/, 'another CI attempt must reject the compiler artifact');
  assert.throws(() => verifyBundle({ ...options, environment: { ...environment, GITHUB_SHA: 'a'.repeat(40) } }), /binding/, 'another source SHA must reject the compiler artifact');
  assert.throws(() => verifyBundle({ ...options, inJob: false }), /Job/, 'the observed runner Job state must agree with compilation');
  assert.throws(() => verifyBundle({ ...options, elevated: true }), /elevation/, 'the actual runner token must agree with compilation');
  const executable = plan.files.find(f => f.path.endsWith('harness-0.exe'));
  const filename = path.join(bundle, executable.path), original = fs.readFileSync(filename);
  fs.appendFileSync(filename, '\nchanged bytes');
  assert.throws(() => verifyBundle(options), /hash|size/, 'tampered executable bytes must reject before any listing');
  fs.writeFileSync(filename, original, { mode: 0o755 });
  fs.rmSync(path.join(root, 'src-tauri/target/debug'), { recursive: true });
  const shards = path.join(root, 'shards');
  for (let index = 0; index < 16; index++) {
    const result = await runShard({ ...options, index, output: path.join(shards, String(index)), artifactName: plan.artifactName });
    assert.equal(result.completed, true);
    assert.equal(result.harnesses[1].executed, index === 0, 'an originally empty harness executes once on shard zero');
    if (index > 0) assert.equal(result.harnesses[2].executed, false, 'empty assignment must not execute a nonempty harness');
  }
  const coverage = path.join(root, 'coverage');
  const report = aggregateResults({ ...options, shards, coverage });
  assert.equal(report.completed, true);
  assert.equal(report.harnesses[0].result.ignored, 1, 'original ignored worker is retained once');
  assert.equal(report.harnesses[0].defaultIgnored[0].reason, 'supervised only');
  assert.equal(report.nativeAll.status, 'unverified');
  assert.equal(report.nativeAcceptanceProven, false);
  assert.equal(report.rustShardRun.shards.length, 16, 'archived run proof retains every empty shard receipt');
  assert.equal(fs.readdirSync(path.join(coverage, 'logs')).length, 17, 'legacy release archive keeps sixteen harness logs and the doctest log');
  const firstPath = path.join(shards, '0/shard-result.json'), first = JSON.parse(fs.readFileSync(firstPath, 'utf8'));
  fs.copyFileSync(firstPath, path.join(shards, 'duplicate-result.json'));
  fs.mkdirSync(path.join(shards, 'duplicate'));
  fs.renameSync(path.join(shards, 'duplicate-result.json'), path.join(shards, 'duplicate/shard-result.json'));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /duplicate/, 'duplicate top-level shard indices reject even empty assignments');
  fs.rmSync(path.join(shards, 'duplicate'), { recursive: true });
  fs.renameSync(path.join(shards, '7/shard-result.json'), path.join(shards, '7/saved.json'));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /missing|indices/, 'all sixteen shard receipts are mandatory');
  fs.renameSync(path.join(shards, '7/saved.json'), path.join(shards, '7/shard-result.json'));
  fs.writeFileSync(firstPath, JSON.stringify({ ...first, runAttempt: 3 }));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /binding/, 'a shard from another attempt cannot complete this run');
  fs.writeFileSync(firstPath, JSON.stringify({ ...first, completed: false }));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /failed|incomplete/, 'failed shards never produce successful native coverage');
});

// 失败必须从同一不可变执行包的真实进程进入正常日志，原失败与原始字节均保留。
test('RustRunner_FailureDiagnostics_002', { skip: process.platform === 'win32' && 'Unix executable fixture; production binaries are Windows PE files' }, async t => {
  const failedName = 'ordinary::new_test';
  const { root, options, plan, runShard, aggregateResults } = await runnerFixture(t, failedName);
  const index = plan.harnesses[0].partitions.findIndex(names => names.includes(failedName));
  const phases = [], shards = path.join(root, 'failure-shards');
  const result = await runShard({ ...options, index, output: path.join(shards, String(index)), artifactName: plan.artifactName, onPhase: phase => phases.push(phase) });
  const end = phases.find(phase => phase.phase === 'execute-end' && phase.identity.kind === 'lib');
  assert.deepEqual(end.failureDiagnostics?.names, [failedName], 'normal phase output must identify the exact failed assigned test');
  const diagnostic = end.failureDiagnostics;
  assert.deepEqual(Object.keys(diagnostic).sort(), ['complete', 'executionLog', 'names', 'rejected', 'truncated']);
  assert.equal(diagnostic.complete, true);
  assert.equal(diagnostic.rejected, 0);
  assert.equal(diagnostic.truncated, 0);
  assert.ok(Buffer.byteLength(JSON.stringify(diagnostic)) <= 8192);
  assert.equal(JSON.stringify(diagnostic).includes('PRIVATE'), false, 'assertion and Debug bodies remain artifact-only');
  const executionPath = plan.harnesses[0].logs.execution;
  const raw = fs.readFileSync(path.join(shards, String(index), executionPath));
  assert.deepEqual(diagnostic.executionLog, { path: executionPath, sha256: createHash('sha256').update(raw).digest('hex'), bytes: raw.length });
  assert.ok(raw.includes(Buffer.from('PRIVATE fixture assertion and Debug value')), 'the raw execution artifact must remain unchanged');
  assert.equal(result.sourceSha, plan.sourceSha);
  assert.equal(result.runId, plan.runId);
  assert.equal(result.runAttempt, plan.runAttempt);
  assert.equal(result.harnesses[0].result.exitCode, 101);
  assert.equal(result.harnesses[0].result.failed, 1);
  assert.equal(result.completed, false);
  assert.equal(result.exitCode, 1);
  const nextHarness = result.harnesses[2];
  assert.equal(nextHarness.executed, true, 'a failed library must not prevent the next assigned harness from executing');
  assert.equal(nextHarness.names.length, 1);
  assert.equal(nextHarness.result.exitCode, 0);
  assert.equal(nextHarness.result.passed, 1);
  assert.ok(phases.indexOf(end) < phases.findIndex(phase => phase.phase === 'execute-end' && phase.identity.name === nextHarness.identity.name));
  for (let other = 0; other < 16; ++other) if (other !== index) {
    assert.equal((await runShard({ ...options, index: other, output: path.join(shards, String(other)), artifactName: plan.artifactName })).completed, true);
  }
  assert.throws(() => aggregateResults({ ...options, shards, coverage: path.join(root, 'failed-coverage') }), /failed|incomplete/, 'diagnostic completeness cannot make a failed run pass');
});
const executionPath = 'logs/cc_desk-execution.log';
function failureLog(names, { nested = '', count = names.length } = {}) {
  return ['running tests', ...names.map(name => `test ${name} ... FAILED`), '', 'failures:', '',
    `---- ${names[0]} stdout ----`, 'PRIVATE assertion, custom panic and Debug value', nested, '', 'failures:',
    ...names.map(name => `    ${name}`), '',
    `test result: FAILED. 0 passed; ${count} failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`, ''].join('\n');
}
const diagnose = (output, assigned = ['owned::one'], incomplete = false) => shardFailureDiagnostics(output, assigned, executionPath, incomplete);

test('RustDiagnostics_OnlyFinalAssignedNames_001', () => {
  const output = failureLog(['owned::one', 'owned::two']);
  const diagnostic = diagnose(output, ['owned::one', 'owned::two']);
  assert.deepEqual(diagnostic.names, ['owned::one', 'owned::two']);
  assert.equal(diagnostic.complete, true);
  assert.equal(diagnostic.rejected, 0);
  assert.equal(diagnostic.truncated, 0);
  assert.equal(JSON.stringify(diagnostic).includes('PRIVATE'), false);
  assert.deepEqual(diagnostic.executionLog, { path: executionPath, sha256: createHash('sha256').update(output).digest('hex'), bytes: Buffer.byteLength(output) });
});

test('RustDiagnostics_NestedWorkerIsIncomplete_002', () => {
  const nested = failureLog(['nested::worker']);
  const diagnostic = diagnose(failureLog(['owned::one'], { nested }), ['owned::one', 'nested::worker']);
  assert.deepEqual(diagnostic.names, ['owned::one'], 'nested assigned names cannot escape the outer final list');
  assert.equal(diagnostic.complete, false);
  assert.equal(diagnostic.rejected, 0);
});

test('RustDiagnostics_RejectsUnknownDuplicateAndControlNames_003', () => {
  const diagnostic = diagnose(failureLog(['owned::one', 'outside::name', 'owned::one', 'owned::\u001b[31m']), ['owned::one', 'owned::\u001b[31m']);
  assert.deepEqual(diagnostic.names, ['owned::one']);
  assert.equal(diagnostic.rejected, 3);
  assert.equal(diagnostic.complete, false);
  assert.equal(JSON.stringify(diagnostic).includes('outside'), false);
  assert.equal(JSON.stringify(diagnostic).includes('[31m'), false);
});

test('RustDiagnostics_MissingOrPartialSummaryIsIncomplete_004', () => {
  const output = failureLog(['owned::one']);
  for (const broken of [output.slice(0, output.indexOf('test result:')), output.slice(0, -6)]) {
    const diagnostic = diagnose(broken);
    assert.deepEqual(diagnostic.names, []);
    assert.equal(diagnostic.complete, false);
    assert.equal(diagnostic.executionLog.bytes, Buffer.byteLength(broken));
  }
});

test('RustDiagnostics_TrailingBodyCannotQualifyNestedSummary_005', () => {
  const diagnostic = diagnose(failureLog(['owned::one']) + 'PRIVATE trailing worker stdout');
  assert.deepEqual(diagnostic.names, []);
  assert.equal(diagnostic.complete, false);
});

test('RustDiagnostics_CountMismatchAndCaptureTruncationAreIncomplete_006', () => {
  assert.equal(diagnose(failureLog(['owned::one'], { count: 2 })).complete, false);
  assert.equal(diagnose(failureLog(['owned::one']), ['owned::one'], true).complete, false);
});

test('RustDiagnostics_NameAndByteCapsAreExplicit_007', () => {
  const names = Array.from({ length: 17 }, (_, index) => `owned::failure_${index}`);
  const capped = diagnose(failureLog(names), names);
  assert.deepEqual(capped.names, names.slice(0, 16));
  assert.equal(capped.truncated, 1);
  assert.equal(capped.complete, false);
  const long = Array.from({ length: 16 }, (_, index) => `owned_${index}::` + 'x'.repeat(500 - `owned_${index}::`.length));
  const bounded = diagnose(failureLog(long), long);
  assert.ok(bounded.names.length < 16);
  assert.ok(bounded.truncated > 0);
  assert.equal(bounded.complete, false);
  assert.ok(Buffer.byteLength(JSON.stringify(bounded)) <= 8192);
});

test('RustDiagnostics_CRLFPreservesOriginalByteProof_008', () => {
  const output = failureLog(['owned::one']).replaceAll('\n', '\r\n');
  const diagnostic = diagnose(output);
  assert.deepEqual(diagnostic.names, ['owned::one']);
  assert.equal(diagnostic.complete, true);
  assert.equal(diagnostic.executionLog.bytes, Buffer.byteLength(output));
  assert.equal(diagnostic.executionLog.sha256, createHash('sha256').update(output).digest('hex'));
});

test('RustDiagnostics_PassingOuterSummaryNeverReportsWorkerFailure_009', () => {
  const pass = 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n';
  assert.equal(diagnose(pass).complete, true);
  const nested = diagnose(failureLog(['owned::one']) + pass);
  assert.deepEqual(nested.names, []);
  assert.equal(nested.complete, false);
});

test('RustDiagnostics_MissingFailureListBoundaryIsIncomplete_010', () => {
  const diagnostic = diagnose(failureLog(['owned::one']).replace('failures:\n', ''));
  assert.deepEqual(diagnostic.names, []);
  assert.equal(diagnostic.complete, false);
});
