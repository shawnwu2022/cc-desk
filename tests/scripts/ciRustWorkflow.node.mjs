import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import path from 'node:path';

const workflow = fs.readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const job = id => workflow.match(new RegExp(`^  ${id}:\\n([\\s\\S]*?)(?=^  [a-z][a-z-]*:|$(?![\\s\\S]))`, 'm'))?.[1];
const gateUrl = new URL('../../scripts/ci-rust-job-gate.mjs', import.meta.url);

const packageWorkflow = fs.readFileSync(new URL('../../.github/workflows/conpty-integration.yml', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const packageJob = id => packageWorkflow.match(new RegExp(`^  ${id}:\\n([\\s\\S]*?)(?=^  [a-z][a-z-]*:|$(?![\\s\\S]))`, 'm'))?.[1];

// 上传回执未知时只能读回相同字节；差异、缺失、额外文件或空证据均不能恢复成功。
test('EvidenceUpload_ExactBytes_010', async t => {
  const { verifyEvidenceCopy } = await import('../../.github/actions/upload-evidence/verify.mjs');
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'ci-evidence-copy-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const original = path.join(directory, 'original'), downloaded = path.join(directory, 'downloaded');
  fs.mkdirSync(path.join(original, 'logs'), { recursive: true });
  fs.mkdirSync(path.join(downloaded, 'logs'), { recursive: true });
  await assert.rejects(verifyEvidenceCopy(original, downloaded), /empty/);
  for (const root of [original, downloaded]) {
    fs.writeFileSync(path.join(root, 'shard-result.json'), '{"completed":false,"exitCode":1}');
    fs.writeFileSync(path.join(root, 'logs/raw.log'), Buffer.from([0, 13, 10, 255]));
  }
  await assert.doesNotReject(verifyEvidenceCopy(original, downloaded), 'transport equality must not reinterpret a failing receipt as passing coverage');
  fs.writeFileSync(path.join(downloaded, 'logs/raw.log'), Buffer.from([0, 10, 255]));
  await assert.rejects(verifyEvidenceCopy(original, downloaded), /bytes/);
  fs.writeFileSync(path.join(downloaded, 'logs/raw.log'), Buffer.from([0, 13, 10, 254]));
  await assert.rejects(verifyEvidenceCopy(original, downloaded), /bytes/, 'equal size with different bytes must still reject');
  fs.writeFileSync(path.join(downloaded, 'logs/raw.log'), Buffer.from([0, 13, 10, 255]));
  fs.writeFileSync(path.join(downloaded, 'extra.json'), '{}');
  await assert.rejects(verifyEvidenceCopy(original, downloaded), /inventory/);
  fs.unlinkSync(path.join(downloaded, 'extra.json'));
  fs.unlinkSync(path.join(downloaded, 'shard-result.json'));
  await assert.rejects(verifyEvidenceCopy(original, downloaded), /inventory/);
  fs.writeFileSync(path.join(downloaded, 'shard-result.json'), '{"completed":false,"exitCode":1}');
  fs.symlinkSync(original, path.join(downloaded, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(verifyEvidenceCopy(original, downloaded), /linked/);
});

// 运行真实恢复入口，合法输入先成功，再检查拒绝信息不会泄露宿主路径或证据正文。
test('EvidenceUpload_CliPrivacy_012', t => {
  const script = fileURLToPath(new URL('../../.github/actions/upload-evidence/verify.mjs', import.meta.url));
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'ci-evidence-cli-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const source = path.join(directory, 'source'), copy = path.join(directory, 'copy');
  for (const root of [source, copy]) { fs.mkdirSync(root); fs.writeFileSync(path.join(root, 'raw.log'), 'PRIVATE captured body'); }
  for (const [changed, expected] of [[false, 0], [true, 1]]) {
    if (changed) fs.writeFileSync(path.join(copy, 'raw.log'), 'different bytes');
    const child = spawnSync(process.execPath, [script], { encoding: 'utf8', env: { ...process.env, EVIDENCE_ROOT: source, EVIDENCE_COPY: copy } });
    assert.ifError(child.error);
    assert.equal(child.status, expected);
    assert.ok(!(child.stdout + child.stderr).includes(directory));
    assert.ok(!(child.stdout + child.stderr).includes('PRIVATE'));
  }
});

// 执行真实上传汇总脚本；原上传失败不能在未知、取消或坏字节回读时被清成成功。
test('EvidenceUpload_ConfirmedOnly_011', () => {
  const action = fs.readFileSync(new URL('../../.github/actions/upload-evidence/action.yml', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
  assert.doesNotMatch(action, /overwrite: true|delete-artifact|github-token:/);
  assert.doesNotMatch(action, /timeout-minutes:/, 'composite action steps do not support workflow timeout metadata');
  assert.match(action, /steps\.readback\.outcome == 'failure'/);
  assert.match(action, /steps\.readback\.outcome == 'success'/);
  assert.equal((action.match(/overwrite: false/g) ?? []).length, 2);
  assert.equal((action.match(/working-directory: \$\{\{ github\.workspace \}\}/g) ?? []).length, 2, 'producer job run defaults must not relocate relative evidence paths');
  const script = action.match(/<<'NODE'\n([\s\S]*?)\n\s*NODE/)?.[1];
  assert.ok(script);
  for (const [original, readback, reconcile, retry, accepted] of [
    ['success', 'skipped', 'skipped', 'skipped', true],
    ['failure', 'success', 'success', 'skipped', true],
    ['failure', 'failure', 'skipped', 'success', true],
    ['failure', 'success', 'failure', 'skipped', false],
    ['failure', 'failure', 'skipped', 'failure', false],
    ['cancelled', 'skipped', 'skipped', 'skipped', false],
    ['failure', 'cancelled', 'skipped', 'skipped', false],
    ['', '', '', '', false],
  ]) {
    const child = spawnSync(process.execPath, ['--input-type=module', '-e', script], { encoding: 'utf8', env: { ...process.env, ORIGINAL_RESULT: original, READBACK_RESULT: readback, RECONCILE_RESULT: reconcile, RETRY_RESULT: retry } });
    assert.ifError(child.error);
    assert.equal(child.status, accepted ? 0 : 1, `${original}/${readback}/${reconcile}/${retry}`);
  }
  for (const id of ['rust-compile', 'rust-tests', 'rust']) {
    assert.match(job(id), /timeout-minutes: 5\n        uses: \.\/\.github\/actions\/upload-evidence/);
  }
  assert.doesNotMatch(job('rust-tests').split('name: Execute assigned full names')[1].split('name: Preserve original selection')[0], /continue-on-error/, 'transport recovery cannot erase an executed failure');
});

// 完整 Windows 验证与真实安装包构建并行，原 installer 名称只在全部成功后通过。
test('ConptyDag_ParallelVerification_007', () => {
  const validation = packageJob('validation'), build = packageJob('package'), gate = packageJob('installer');
  assert.ok(validation && build && gate, 'separate Windows verification/package jobs and the original installer gate are required');
  for (const text of [validation, build]) {
    assert.match(text, /runs-on: windows-2022/);
    assert.doesNotMatch(text, /^    needs:/m, 'both branches must be runnable together');
    assert.match(text, /cache-workspace-crates: false/);
    assert.match(text, /npm ci/);
    assert.match(text, /node scripts\/prepare-conpty.mjs/);
  }
  for (const command of ['npm run typecheck', 'npm test', 'tests/scripts/nativeReleasePolicy.node.mjs', 'cargo test --locked --bin cc-desk ConptyRuntime_ -- --nocapture', 'cargo clippy --locked --all-targets -- -D warnings']) assert.ok(validation.includes(command), `retain Windows verification: ${command}`);
  for (const command of ['npm run tauri -- build --bundles nsis', 'createUpdaterArtifacts":false', './scripts/test-conpty-install.ps1', 'git diff --exit-code']) assert.ok(build.includes(command), `retain actual package verification: ${command}`);
  assert.match(gate, /needs: \[formatting, validation, package\]/);
  assert.match(gate, /if: \$\{\{ always\(\) \}\}/);
  assert.ok(gate.indexOf('Require all Windows checks') < gate.indexOf('actions/download-artifact'));
  assert.doesNotMatch(build, /name: cc-desk-native-v3-test-/);
  assert.match(build, /name: conpty-package-candidate-\$\{\{ github\.sha \}\}-\$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}/);
  assert.match(gate, /name: cc-desk-native-v3-test-\$\{\{ github\.sha \}\}/);
});

// 执行 workflow 中的真实汇总脚本，失败、取消、跳过或缺失均不能发布已验证测试包。
test('ConptyDag_FailClosedGate_008', () => {
  const gate = packageJob('installer');
  assert.ok(gate);
  const script = gate.match(/<<'NODE'\n([\s\S]*?)\n\s*NODE/)?.[1];
  assert.ok(script, 'the final package gate must execute an explicit result check');
  const accepted = { FORMAT_RESULT: 'success', VALIDATION_RESULT: 'success', PACKAGE_RESULT: 'success' };
  for (const values of [accepted, ...Object.keys(accepted).flatMap(key => ['failure', 'cancelled', 'skipped', '', 'Success'].map(value => ({ ...accepted, [key]: value })))]) {
    const child = spawnSync(process.execPath, ['--input-type=module', '-e', script], { encoding: 'utf8', env: { ...process.env, ...values } });
    assert.ifError(child.error);
    assert.equal(child.status, Object.values(values).every(value => value === 'success') ? 0 : 1, JSON.stringify(values));
  }
});

// 执行下载后的实际字节/来源检查；其他 attempt、篡改正文和路径逃逸都必须拒绝。
test('ConptyDag_CandidateBinding_009', () => {
  const gate = packageJob('installer');
  const script = gate?.split('name: Recheck candidate identity and installer bytes')[1]?.match(/<<'NODE'\n([\s\S]*?)\n\s*NODE/)?.[1];
  assert.ok(script);
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'conpty-candidate-'));
  const root = path.join(directory, 'verified-package/test-package');
  fs.mkdirSync(root, { recursive: true });
  const name = 'CC-Desk-0.23.1-test-37fef8b73066-x64-setup.exe';
  fs.writeFileSync(path.join(root, name), 'original installer bytes');
  const manifest = { sourceCommit: '1'.repeat(40), workflowRunId: '123', workflowRunAttempt: '2', channel: 'test-only', publishable: false, updaterPublication: false,
    installerFile: name, sha256: '1e951b647ae87c4f6add5d85a8b33b3ef9dba60931ed1c681531499a95875d42' };
  try {
    for (const [key, value] of [[null, null], ['sourceCommit', '2'.repeat(40)], ['workflowRunId', '124'], ['workflowRunAttempt', '1'], ['publishable', true], ['updaterPublication', true], ['channel', 'stable'], ['installerFile', '../escape.exe'], ['installerFile', '..\\escape.exe'], ['sha256', '0'.repeat(64)]]) {
      fs.writeFileSync(path.join(root, 'test-package-manifest.json'), '\uFEFF' + JSON.stringify(key === null ? manifest : { ...manifest, [key]: value }));
      const child = spawnSync(process.execPath, ['--input-type=module', '-e', script], { cwd: directory, encoding: 'utf8', env: { ...process.env, SOURCE_SHA: manifest.sourceCommit, RUN_ID: manifest.workflowRunId, RUN_ATTEMPT: manifest.workflowRunAttempt } });
      assert.ifError(child.error);
      assert.equal(child.status, key === null ? 0 : 1, `${key}: ${child.stderr}`);
    }
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

// 生产首屏门禁始终重建正常入口；普通 CI 不得在它之前再构建同一份前端。
test('FrontendBuild_ProductionOwnsBuild_006', () => {
  const frontend = job('frontend');
  assert.ok(frontend);
  assert.doesNotMatch(frontend, /run: npm run build(?:\n|$)/);
  assert.match(frontend, /run: npm run test:ci/);
  assert.match(frontend, /run: npm run typecheck/);
  assert.match(frontend, /run: npm run test:production/);
  const config = fs.readFileSync(new URL('../../playwright.production.config.ts', import.meta.url), 'utf8');
  assert.match(config, /command: 'node tests\/production\/serve\.mjs'/);
  assert.match(config, /reuseExistingServer: false/);
  const server = fs.readFileSync(new URL('../../tests/production/serve.mjs', import.meta.url), 'utf8');
  assert.match(server, /await build\(\{ root \}\)/);
  assert.ok(server.indexOf('await build({ root })') < server.indexOf('await preview('), 'build failure must reject before serving');
  assert.match(server, /Content-Security-Policy/);
});

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
  // 先证明真实入口能接受完整成功，不能把模块未找到误当作门禁拒绝。
  const accepted = spawnSync(process.execPath, [fileURLToPath(gateUrl)], { encoding: 'utf8', env: { ...process.env, COMPILE_RESULT: 'success', STATIC_RESULT: 'success', SHARD_RESULT: 'success' } });
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.equal(accepted.stdout, '');
  assert.equal(accepted.stderr, '');
  for (const key of ['compile', 'static', 'shards']) for (const result of ['failure', 'cancelled', 'skipped', '', undefined, 'Success']) {
    const values = { compile: 'success', static: 'success', shards: 'success', [key]: result };
    assert.throws(() => requireRustJobSuccess(values), new RegExp(key));
    const child = spawnSync(process.execPath, [fileURLToPath(gateUrl)], { encoding: 'utf8', env: { ...process.env, COMPILE_RESULT: values.compile ?? '', STATIC_RESULT: values.static ?? '', SHARD_RESULT: values.shards ?? '' } });
    assert.equal(child.status, 1);
    assert.equal(child.stdout, '');
    assert.equal(child.stderr, `Rust checks: ${key} job result must be success\n`);
  }
});

test('RustDag_Cache_005: same compatible defaults share dependency cache and clear all source-bound evidence', () => {
  for (const id of ['rust-compile', 'rust-static']) {
    const text = job(id);
    assert.match(text, /shared-key: rust-compile/);
    assert.match(text, /node-version: 22\n          cache: npm/);
    assert.match(text, /cache-workspace-crates: false/);
    assert.match(text, /ci-build-metrics-\$\{\{ github\.job \}\}-\$\{\{ github\.sha \}\}-\$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}/);
    assert.match(text, /'src-tauri\/target\/ci-build-metrics'/);
  }
  assert.match(job('rust-static'), /save-if: false/);
  assert.match(workflow, /CARGO_PROFILE_DEV_DEBUG: '0'/);
  assert.match(workflow, /CARGO_PROFILE_TEST_DEBUG: '0'/);
});
