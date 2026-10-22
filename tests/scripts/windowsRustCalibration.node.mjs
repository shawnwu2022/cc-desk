import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { calibrationGroups, calibrationEnabled, calibrationWeights, readCalibrationWeights } from '../../scripts/windows-rust-calibration.mjs';

test('RustCalibration_CheckoutPreservesExactSource_004', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rust-calibration-checkout-'));
  t.after(() => { assert.equal(path.dirname(root), os.tmpdir()); fs.rmSync(root, { recursive: true, force: true }); });
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.autocrlf=true', ...args], { windowsHide: true, maxBuffer: 1024 * 1024 });
    assert.equal(result.status, 0, result.stderr?.toString());
    return result.stdout;
  };
  git('init', '--quiet');
  const files = ['scripts/windows-rust-calibration.mjs', 'scripts/windows-rust-calibration-targets.json'];
  fs.mkdirSync(path.join(root, 'scripts'));
  for (const file of ['.gitattributes', ...files]) fs.copyFileSync(new URL(`../../${file}`, import.meta.url), path.join(root, file));
  git('add', '--', '.gitattributes', ...files);
  for (const file of files) fs.unlinkSync(path.join(root, file));
  git('checkout-index', '--all');
  // 真正经过 Windows 换行规则后仍须匹配 Git blob，不能把合法检出错判为源码篡改。
  for (const file of files) assert.deepEqual(fs.readFileSync(path.join(root, file)), git('show', `:${file}`));
});

test('RustCalibration_OptInKeepsOriginalGate_003', () => {
  const yaml = fs.readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  assert.match(yaml, /calibrate_rust:\n\s+type: boolean\n\s+default: false/);
  assert.doesNotMatch(yaml, /^  rust-calibration:/m, 'skipped jobs would fail the existing all-jobs-success release boundary');
  const job = yaml.match(/^  rust-tests:\n([\s\S]*?)(?=^  [a-z][a-z-]*:)/m)?.[1];
  assert.ok(job, 'calibration follows original proof in the existing shards');
  assert.match(job, /workflow_dispatch.*inputs\.calibrate_rust == true/);
  assert.match(job, /pull_request.*\[calibrate-rust\]/);
  assert.match(job, /needs: rust-compile/);
  assert.match(job, /matrix.shard < 4/);
  assert.match(job, /foreach \(\$group in @\(\[int\]\$\{\{ matrix.shard \}\}, \(\[int\]\$\{\{ matrix.shard \}\} \+ 4\)\)\)/);
  assert.match(job, /timeout-minutes: 30/);
  assert.doesNotMatch(job, /continue-on-error/);
  assert.ok(job.indexOf('path: src-tauri/target/ci-rust-shard-') < job.indexOf('name: Measure optional exact-name calibration'), 'original completed proof must be retained first');
  assert.match(job, /uses: \.\/\.github\/actions\/upload-evidence/);
  assert.match(job, /windows-rust-bundle-\$\{\{ github.sha \}\}-\$\{\{ github.run_id \}\}-\$\{\{ github.run_attempt \}\}/);
  assert.match(yaml, /needs: \[rust-compile, rust-static, rust-tests\]/);
  assert.match(yaml, /node --test[^\n]*tests\/scripts\/windowsRustCalibration.node.mjs/);
});

test('RustCalibration_ExactTargets_001', () => {
  const targets = Array.from({ length: 16 }, (_, i) => `fixture::Original_${i}`);
  const groups = calibrationGroups({ schema: 1, targets });
  assert.equal(groups.length, 8);
  assert.deepEqual(new Set(groups.flat()), new Set(targets));
  assert.deepEqual(calibrationGroups({ schema: 1, targets: [...targets].reverse() }), groups);
  for (const bad of [[], ['--ignored'], ['x\ny'], ['a::b', 'a::b'], [...targets, 'fixture::Extra']])
    assert.throws(() => calibrationGroups({ schema: 1, targets: bad }));
  for (const value of [undefined, '', 'false']) assert.equal(calibrationEnabled(value), false);
  assert.equal(calibrationEnabled('true'), true);
  assert.throws(() => calibrationEnabled('yes'));
});

test('RustCalibration_RejectIncompleteOrForeignMeasurements_002', t => {
  const names = Array.from({ length: 8 }, (_, i) => `fixture::Original_${i}`);
  const compiler = { rustcVerbose: 'rustc 1.98.1', runnerOs: 'Windows', runnerArch: 'X64', profileDevDebug: '0', profileTestDebug: '0' };
  const plan = { sourceSha: 'a'.repeat(40), runId: '42', runAttempt: 1, planHash: 'b'.repeat(64), artifactName: 'same-run-bundle', compiler,
    host: { inJob: true, elevated: false }, files: [{ path: 'src-tauri/target/debug/deps/fixture.exe', sha256: 'c'.repeat(64) }],
    harnesses: [{ identity: { kind: 'lib', name: 'fixture' }, executable: 'src-tauri/target/debug/deps/fixture.exe', full: names.map(name => ({ name, type: 'test' })), selected: names, ignored: [] }] };
  const receipts = calibrationGroups({ schema: 1, targets: names }).map((targets, index) => ({ schema: 1, kind: 'rust-exact-name-calibration-v1',
    sourceSha: plan.sourceSha, runId: plan.runId, runAttempt: 1, planHash: plan.planHash, artifactName: plan.artifactName, compiler,
    targetFileHash: 'd'.repeat(64), host: plan.host, index, groupCount: 8, targets, completed: true, exitCode: 0, nativeAcceptanceProven: false,
    measurements: targets.map(name => ({ name, identity: plan.harnesses[0].identity, executable: plan.harnesses[0].executable,
      executableSha256: 'c'.repeat(64), argv: ['--exact', name], completed: true, exitCode: 0, durationSeconds: 1.5,
      logHash: 'e'.repeat(64), listingHash: 'f'.repeat(64), watchdog: { timedOut: false, outputIncomplete: false },
      result: { passed: 1, failed: 0, ignored: 0, measured: 0, filteredOut: 7 } })) }));
  const weights = calibrationWeights(plan, receipts, { schema: 1, targets: names }, 'd'.repeat(64));
  assert.deepEqual(Object.keys(weights).sort(), names.sort());
  for (const mutate of [r => r.pop(), r => r.push(r[0]), r => r[0].completed = false, r => r[0].sourceSha = '0'.repeat(40),
    r => r[0].runAttempt++, r => r[0].planHash = '0'.repeat(64), r => r[0].targetFileHash = '0'.repeat(64),
    r => r[0].compiler.runnerArch = 'arm64', r => r[0].measurements[0].executableSha256 = '0'.repeat(64),
    r => r[0].measurements[0].result.ignored = 1, r => r[0].measurements[0].watchdog.outputIncomplete = true,
    r => r[0].measurements[0].durationSeconds = 0, r => r[0].measurements[0].durationSeconds = Infinity,
    r => r[0].measurements[0].name = 'fixture::Invented', r => r[0].measurements[0].argv.push('--ignored')]) {
    const altered = structuredClone(receipts); mutate(altered);
    assert.throws(() => calibrationWeights(plan, altered, { schema: 1, targets: names }, 'd'.repeat(64)));
  }
  const ignored = structuredClone(plan); ignored.harnesses[0].ignored.push(names[0]);
  assert.throws(() => calibrationWeights(ignored, receipts, { schema: 1, targets: names }, 'd'.repeat(64)));

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rust-calibration-test-'));
  t.after(() => {
    assert.equal(path.dirname(root), os.tmpdir());
    fs.rmSync(root, { recursive: true, force: true });
  });
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  const directories = receipts.map(r => {
    const directory = path.join(root, String(r.index));
    fs.mkdirSync(path.join(directory, 'logs'), { recursive: true });
    const m = r.measurements[0];
    const listed = `${m.name}: test\n\n1 test, 0 benchmarks\n`;
    const output = `test ${m.name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 1.50s\n`;
    m.logs = { selected: 'logs/0-selected.log', execution: 'logs/0-execution.log' };
    m.listingHash = hash(listed); m.logHash = hash(output);
    fs.writeFileSync(path.join(directory, m.logs.selected), listed);
    fs.writeFileSync(path.join(directory, m.logs.execution), output);
    fs.writeFileSync(path.join(directory, 'calibration-result.json'), JSON.stringify(r));
    return directory;
  });
  assert.deepEqual(readCalibrationWeights(plan, directories, { schema: 1, targets: names }, 'd'.repeat(64)), weights);
  const first = receipts[0], measurement = first.measurements[0];
  const originalOutput = fs.readFileSync(path.join(directories[0], measurement.logs.execution), 'utf8');
  // 真实 CrashRecovery harness 会把子进程 libtest 输出继承到父 stdout，父结果仍在末尾。
  const nestedOutput = 'running 1 test\ntest fixture::ProcessWorker ... ok\n'
    + 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.06s\n\n'
    + originalOutput;
  fs.writeFileSync(path.join(directories[0], measurement.logs.execution), nestedOutput);
  measurement.logHash = hash(nestedOutput);
  fs.writeFileSync(path.join(directories[0], 'calibration-result.json'), JSON.stringify(first));
  assert.deepEqual(readCalibrationWeights(plan, directories, { schema: 1, targets: names }, 'd'.repeat(64)), weights);
  for (const malformed of [
    originalOutput.replace(measurement.name, 'fixture::Unrelated'),
    originalOutput + 'TRAILING PARTIAL OUTPUT\n',
    originalOutput.replace('finished in 1.50s', 'finished in'),
    originalOutput + originalOutput,
    nestedOutput.replace(measurement.name, 'fixture::Unrelated'),
    originalOutput + nestedOutput,
    originalOutput + 'test fixture::ProcessWorker ... ok\n'
      + 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.06s\n',
  ]) {
    // 同时更新摘要 hash，避免仅证明既有字节校验；实际执行名称和终止记录也必须成立。
    fs.writeFileSync(path.join(directories[0], measurement.logs.execution), malformed);
    measurement.logHash = hash(malformed);
    fs.writeFileSync(path.join(directories[0], 'calibration-result.json'), JSON.stringify(first));
    assert.throws(() => readCalibrationWeights(plan, directories, { schema: 1, targets: names }, 'd'.repeat(64)), /exact original completion|terminal outer summary/);
  }
  fs.writeFileSync(path.join(directories[0], measurement.logs.execution), originalOutput);
  measurement.logHash = hash(originalOutput);
  fs.writeFileSync(path.join(directories[0], 'calibration-result.json'), JSON.stringify(first));
  fs.appendFileSync(path.join(directories[0], 'logs/0-execution.log'), 'changed');
  assert.throws(() => readCalibrationWeights(plan, directories, { schema: 1, targets: names }, 'd'.repeat(64)), /logs changed/);
});
