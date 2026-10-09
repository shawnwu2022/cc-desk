// A single Cargo compilation feeds immutable, source-bound executable shards.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { partitionNames, validatePartition, aggregateHarness } from './windows-rust-shards.mjs';
import { coverageArtifactName, JOB_FREE_TESTS, VALIDATION_POLICY, REPORT_FILENAME, parseLibtestListing, parseLibtestResult, validateNativeCoverage, readNativeCoverageArtifact } from './windows-native-validation.mjs';

const POLICY = 'same-source-compiled-rust-shards-v1';
const SHARD_COUNT = 8;
const MAX_LOG = 64 * 1024 * 1024;
const MAX_JSON = 8 * 1024 * 1024;
const BOUND_FILES = ['src-tauri/Cargo.lock', '.github/workflows/ci.yml', 'scripts/windows-rust-tests.ps1', 'scripts/windows-rust-shard-runner.mjs', 'scripts/windows-rust-shards.mjs', 'scripts/windows-native-validation.mjs', 'scripts/windows-native-scope.json', 'scripts/prepare-conpty.mjs', 'src-tauri/conpty/manifest.json'];
const scope = JSON.parse(fs.readFileSync(new URL('./windows-native-scope.json', import.meta.url), 'utf8'));
const hash = data => createHash('sha256').update(data).digest('hex');
function requireThat(ok, message) { if (!ok) throw new Error(`Rust runner: ${message}`); }
function equal(a, b) { return a.length === b.length && a.every(n => b.includes(n)); }
function identity(h) { return `${h.identity.kind}:${h.identity.name}`; }
function readFile(filename, maximum = MAX_LOG) {
  const stat = fs.lstatSync(filename);
  requireThat(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0 && stat.size <= maximum, `file missing, linked, empty or oversized: ${filename}`);
  return fs.readFileSync(filename);
}
function readJson(filename) { return JSON.parse(readFile(filename, MAX_JSON).toString('utf8').replace(/^\uFEFF/, '')); }
function writeJson(filename, value) { fs.mkdirSync(path.dirname(filename), { recursive: true }); fs.writeFileSync(filename, JSON.stringify(value, null, 2) + '\n'); }
function relativeFile(root, relative) {
  requireThat(typeof relative === 'string' && relative.length < 1000 && !relative.includes('\\') && !relative.includes('\0') && !path.posix.isAbsolute(relative) && relative.split('/').every(s => s && s !== '.' && s !== '..'), 'unsafe relative file');
  const filename = path.join(root, ...relative.split('/'));
  requireThat(fs.realpathSync(filename).startsWith(fs.realpathSync(root) + path.sep), 'file escaped artifact');
  return filename;
}
function fileRecord(root, relative) { const data = readFile(relativeFile(root, relative), 2 * 1024 * 1024 * 1024); return { path: relative, bytes: data.length, sha256: hash(data) }; }
function verifyFile(root, file) {
  requireThat(file && Number.isSafeInteger(file.bytes) && file.bytes > 0 && /^[a-f0-9]{64}$/.test(file.sha256), 'invalid bundle file descriptor');
  const actual = fileRecord(root, file.path);
  requireThat(actual.bytes === file.bytes && actual.sha256 === file.sha256, `bundle file hash/size mismatch: ${file.path}`);
}
function invoke(command, args, root, environment) {
  const start = process.hrtime.bigint();
  const child = spawnSync(command, args, { cwd: root, env: environment, encoding: 'utf8', maxBuffer: MAX_LOG, windowsHide: true });
  const output = ((child.stdout ?? '') + (child.stderr ?? '')).replaceAll('\r\n', '\n');
  return { output, exitCode: child.status ?? 1, durationSeconds: Number(process.hrtime.bigint() - start) / 1e9, error: child.error ? String(child.error.message) : child.signal ? `terminated by ${child.signal}` : null };
}
function context(options) {
  const root = fs.realpathSync(options.root ?? process.cwd());
  const environment = options.environment ?? process.env;
  const sourceSha = environment.GITHUB_SHA, runId = environment.GITHUB_RUN_ID, runAttempt = Number(environment.GITHUB_RUN_ATTEMPT);
  coverageArtifactName(sourceSha, runId, runAttempt);
  const checkout = invoke('git', ['rev-parse', 'HEAD'], root, environment);
  requireThat(checkout.exitCode === 0 && checkout.output.trim() === sourceSha, 'checkout source binding mismatch');
  return { root, testCwd: path.join(root, 'src-tauri'), environment, sourceSha, runId: String(runId), runAttempt };
}
function sameBinding(value, expected) {
  requireThat(value?.sourceSha === expected.sourceSha && value.runId === expected.runId && value.runAttempt === expected.runAttempt, 'source/run/attempt binding mismatch');
}
export function bundleArtifactName(sourceSha, runId, runAttempt) { coverageArtifactName(sourceSha, runId, runAttempt); return `windows-rust-bundle-${sourceSha}-${runId}-${runAttempt}`; }
export function shardArtifactName(sourceSha, runId, runAttempt, index) {
  requireThat(Number.isInteger(index) && index >= 0 && index < SHARD_COUNT, 'invalid shard index');
  coverageArtifactName(sourceSha, runId, runAttempt);
  return `windows-rust-shard-${index}-${sourceSha}-${runId}-${runAttempt}`;
}
function planDigest(plan) { const { planHash, ...body } = plan; return hash(JSON.stringify(body)); }
function selection(full, ignored, kind, inJob) {
  const names = full.map(t => t.name);
  requireThat(ignored.every(n => names.includes(n)), 'ignored inventory outside full inventory');
  if (kind === 'lib') {
    requireThat(JOB_FREE_TESTS.every(n => names.includes(n) && !ignored.includes(n)), 'reviewed Job-free policy drift');
  } else requireThat(!names.some(n => JOB_FREE_TESTS.includes(n)), 'Job-free test outside library');
  const excluded = kind === 'lib' && inJob ? [...JOB_FREE_TESTS] : [];
  requireThat(excluded.every(n => !names.some(other => other !== n && other.includes(n))), 'full-name skip substring collision');
  const selected = names.filter(n => !excluded.includes(n));
  if (kind === 'lib') {
    requireThat(scope.requiredSelectedTests.every(n => selected.includes(n) && !ignored.includes(n)), 'required ordinary/Wry test not selected');
    requireThat(full.some(t => t.type === 'test' && selected.includes(t.name) && !ignored.includes(t.name)), 'library selected nonignored inventory empty');
  }
  return { selected, excluded };
}
function logNames(name) { return Object.fromEntries(['full', 'ignored', 'selected', 'execution'].map(phase => [phase, `logs/${name}-${phase}.log`])); }
function inventory(executable, args, context, filename) {
  const result = invoke(executable, [...args, '--list', '--format', 'pretty'], context.testCwd, context.environment);
  fs.writeFileSync(filename, result.output);
  requireThat(result.exitCode === 0 && !result.error, `inventory failed: ${executable}`);
  return parseLibtestListing(result.output);
}
function targetsFromCargo(filename, root) {
  const records = readFile(filename).toString('utf8').replace(/^\uFEFF/, '').split(/\r?\n/).filter(l => l.trim()).map(l => JSON.parse(l));
  const finished = records.filter(r => r.reason === 'build-finished');
  requireThat(finished.length === 1 && finished[0].success === true, 'successful Cargo build-finished record missing');
  const manifest = path.join(root, 'src-tauri/Cargo.toml');
  const targets = records.filter(r => r.reason === 'compiler-artifact' && r.profile?.test === true && r.executable && path.resolve(r.manifest_path) === manifest);
  const distinct = [...new Map(targets.map(r => [path.resolve(r.executable), r])).values()];
  return distinct.map(r => {
    const kinds = r.target.kind.filter(k => ['lib', 'bin', 'test'].includes(k));
    requireThat(kinds.length === 1, 'unsupported default Cargo test target');
    const executable = path.relative(root, path.resolve(r.executable)).split(path.sep).join('/');
    requireThat(/^src-tauri\/target\/debug\/deps\/[^/]+\.exe$/i.test(executable), 'compiled executable outside original debug/deps location');
    return { identity: { name: r.target.name, kind: kinds[0] }, executable };
  });
}
function assertHarnessIdentities(harnesses) {
  requireThat(Array.isArray(harnesses) && harnesses.length === scope.harnesses.length && new Set(harnesses.map(identity)).size === scope.harnesses.length && equal(harnesses.map(identity), scope.harnesses.map(h => `${h.kind}:${h.name}`)), 'original four-harness identity mismatch');
}
function bundleLocation(options, root) { return path.resolve(options.bundle ?? path.join(root, 'src-tauri/target/ci-rust-bundle')); }
function conptyRuntimeNames(root) {
  const files = readJson(path.join(root, 'src-tauri/conpty/manifest.json')).files;
  requireThat(Array.isArray(files) && files.length > 0 && files.length <= 16 && files.every(f => typeof f.name === 'string' && /^[A-Za-z0-9][A-Za-z0-9_.-]*$/.test(f.name)) && new Set(files.map(f => f.name.toLowerCase())).size === files.length, 'invalid checked-in ConPTY runtime inventory');
  return files.map(f => f.name);
}
function conptyRuntimePaths(root) { return ['debug', 'debug/deps'].flatMap(directory => conptyRuntimeNames(root).map(name => `src-tauri/target/${directory}/${name}`)); }

export function createPlan(options = {}) {
  requireThat(typeof options.inJob === 'boolean', 'explicit successful Job observation required');
  const c = context(options), bundle = bundleLocation(options, c.root);
  fs.mkdirSync(path.join(bundle, 'logs'), { recursive: true });
  requireThat(!fs.existsSync(path.join(bundle, 'plan.json')), 'compile plan already exists');
  const cargoJson = path.resolve(options.cargoJson ?? path.join(c.root, 'src-tauri/target/ci-test-artifacts.jsonl'));
  const targets = targetsFromCargo(cargoJson, c.root); assertHarnessIdentities(targets);
  const rustc = invoke('rustc', ['-Vv'], c.root, c.environment);
  requireThat(rustc.exitCode === 0 && /^rustc \S+/m.test(rustc.output), 'compiler version observation failed');
  const docExitCode = options.docExitCode ?? 0;
  requireThat(Number.isSafeInteger(docExitCode), 'invalid doctest exit code');
  const docResult = { exitCode: docExitCode, ...parseLibtestResult(readFile(path.join(bundle, 'logs/doctests.log')).toString('utf8')) };
  requireThat(docExitCode === 0 && docResult.failed === 0 && docResult.filteredOut === 0, 'compile doctests failed or filtered');
  const doctests = { command: 'cargo test --locked --doc', exitCode: docExitCode, result: docResult, logs: ['logs/doctests.log'] };
  writeJson(path.join(bundle, 'doc.result.json'), doctests);
  const plan = {
    schema: 1, policy: POLICY, sourceSha: c.sourceSha, runId: c.runId, runAttempt: c.runAttempt,
    artifactName: bundleArtifactName(c.sourceSha, c.runId, c.runAttempt), checkoutRoot: c.root, shardCount: SHARD_COUNT,
    host: { jobQuerySucceeded: true, inJob: options.inJob },
    compiler: { rustcVerbose: rustc.output.trim(), rustupToolchain: c.environment.RUSTUP_TOOLCHAIN ?? null, profileDevDebug: c.environment.CARGO_PROFILE_DEV_DEBUG ?? null, profileTestDebug: c.environment.CARGO_PROFILE_TEST_DEBUG ?? null, runnerOs: c.environment.RUNNER_OS ?? process.platform, runnerArch: c.environment.RUNNER_ARCH ?? process.arch, imageOs: c.environment.ImageOS ?? null, imageVersion: c.environment.ImageVersion ?? null },
    contentHashes: BOUND_FILES.map(relative => fileRecord(c.root, relative)), cargoJsonHash: hash(readFile(cargoJson)),
    harnesses: [], doctests, files: [],
  };
  for (const target of targets.sort((a, b) => scope.harnesses.findIndex(h => h.name === a.identity.name) - scope.harnesses.findIndex(h => h.name === b.identity.name))) {
    const executable = relativeFile(c.root, target.executable), logs = logNames(target.identity.name);
    const full = inventory(executable, [], c, path.join(bundle, logs.full));
    const ignored = inventory(executable, ['--ignored'], c, path.join(bundle, logs.ignored)).map(t => t.name);
    const policy = selection(full, ignored, target.identity.kind, options.inJob);
    const selected = inventory(executable, policy.excluded.flatMap(n => ['--skip', n]), c, path.join(bundle, logs.selected));
    requireThat(equal(selected.map(t => t.name), policy.selected) && selected.every(t => full.some(f => f.name === t.name && f.type === t.type)), 'compile selection differs from exact policy');
    plan.harnesses.push({ ...target, full, ignored, ...policy, logs, partitions: partitionNames(policy.selected, SHARD_COUNT) });
  }
  const payload = new Set(plan.harnesses.map(h => h.executable));
  const runtimeNames = conptyRuntimeNames(c.root);
  for (const relative of ['src-tauri/target/debug', 'src-tauri/target/debug/deps']) {
    for (const entry of fs.readdirSync(path.join(c.root, relative), { withFileTypes: true })) {
      if (/\.dll$/i.test(entry.name) || runtimeNames.includes(entry.name)) {
        requireThat(entry.isFile() && !entry.isSymbolicLink(), 'runtime dependency is linked or not a file');
        payload.add(`${relative}/${entry.name}`);
      }
    }
  }
  for (const required of conptyRuntimePaths(c.root)) requireThat([...payload].some(p => p.toLowerCase() === required.toLowerCase()), `required ConPTY runtime missing: ${required}`);
  for (const relative of payload) {
    const source = relativeFile(c.root, relative), destination = path.join(bundle, relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true }); fs.copyFileSync(source, destination);
    fs.chmodSync(destination, fs.statSync(source).mode & 0o777);
  }
  const metadata = ['doc.result.json', 'logs/doctests.log', ...plan.harnesses.flatMap(h => [h.logs.full, h.logs.ignored, h.logs.selected])];
  plan.files = [...payload, ...metadata].sort().map(relative => fileRecord(bundle, relative));
  requireThat(equal(walkFiles(bundle), plan.files.map(f => f.path)), 'unlisted file in compile bundle');
  plan.planHash = planDigest(plan); writeJson(path.join(bundle, 'plan.json'), plan);
  return plan;
}

export function verifyBundle(options = {}) {
  const c = context(options), bundle = bundleLocation(options, c.root), plan = readJson(path.join(bundle, 'plan.json'));
  sameBinding(plan, c);
  requireThat(plan.schema === 1 && plan.policy === POLICY && plan.shardCount === SHARD_COUNT && plan.planHash === planDigest(plan), 'plan hash/policy mismatch');
  requireThat(plan.artifactName === bundleArtifactName(c.sourceSha, c.runId, c.runAttempt), 'compiler artifact name binding mismatch');
  if (options.artifactName !== undefined) requireThat(options.artifactName === plan.artifactName, 'downloaded compiler artifact name binding mismatch');
  requireThat(plan.host?.jobQuerySucceeded === true && typeof plan.host.inJob === 'boolean', 'compile Job observation missing');
  if (options.inJob !== undefined) requireThat(typeof options.inJob === 'boolean' && options.inJob === plan.host.inJob, 'observed runner Job state differs from compilation');
  if (options.requireCheckoutPath) requireThat(c.root === plan.checkoutRoot, 'compiled checkout path binding mismatch');
  requireThat(typeof plan.compiler?.rustcVerbose === 'string' && /^rustc \S+/m.test(plan.compiler.rustcVerbose), 'compiler identity missing');
  requireThat(Array.isArray(plan.contentHashes) && new Set(plan.contentHashes.map(f => f.path)).size === BOUND_FILES.length && equal(plan.contentHashes.map(f => f.path), BOUND_FILES), 'source content hash binding incomplete');
  plan.contentHashes.forEach(file => verifyFile(c.root, file));
  assertHarnessIdentities(plan.harnesses);
  requireThat(Array.isArray(plan.files) && new Set(plan.files.map(f => f.path)).size === plan.files.length, 'duplicate/missing bundle file inventory');
  requireThat(equal(walkFiles(bundle), ['plan.json', ...plan.files.map(f => f.path)]), 'unlisted file in compiler artifact');
  const runtimePaths = conptyRuntimePaths(c.root);
  for (const file of plan.files) {
    requireThat(/^src-tauri\/target\/debug\/(?:deps\/)?[^/]+\.dll$/i.test(file.path) || plan.harnesses.some(h => h.executable === file.path) || runtimePaths.some(p => p.toLowerCase() === file.path.toLowerCase()) || /^logs\/[A-Za-z0-9_.-]+\.log$/.test(file.path) || file.path === 'doc.result.json', 'unexpected bundle file');
    verifyFile(bundle, file);
  }
  const required = ['doc.result.json', 'logs/doctests.log'];
  for (const h of plan.harnesses) {
    requireThat(/^src-tauri\/target\/debug\/deps\/[^/]+\.exe$/i.test(h.executable), 'invalid executable path');
    requireThat(JSON.stringify(h.logs) === JSON.stringify(logNames(h.identity.name)), 'invalid compile log paths');
    const full = parseLibtestListing(readFile(relativeFile(bundle, h.logs.full)).toString('utf8'));
    const ignored = parseLibtestListing(readFile(relativeFile(bundle, h.logs.ignored)).toString('utf8'));
    const selected = parseLibtestListing(readFile(relativeFile(bundle, h.logs.selected)).toString('utf8'));
    requireThat(JSON.stringify(full) === JSON.stringify(h.full) && equal(ignored.map(t => t.name), h.ignored) && equal(selected.map(t => t.name), h.selected), 'original raw compile inventory changed');
    requireThat([...ignored, ...selected].every(t => full.some(f => f.name === t.name && f.type === t.type)), 'compile inventory type changed');
    const expected = selection(full, h.ignored, h.identity.kind, plan.host.inJob);
    requireThat(equal(expected.selected, h.selected) && equal(expected.excluded, h.excluded), 'compile selection policy changed');
    requireThat(Array.isArray(h.partitions) && h.partitions.length === SHARD_COUNT, 'missing shard partitions');
    validatePartition(h.selected, h.partitions);
    requireThat(JSON.stringify(h.partitions) === JSON.stringify(partitionNames(h.selected, SHARD_COUNT)), 'deterministic shard partition changed');
    required.push(h.executable, h.logs.full, h.logs.ignored, h.logs.selected);
  }
  for (const required of runtimePaths) requireThat(plan.files.some(f => f.path.toLowerCase() === required.toLowerCase()), 'ConPTY runtime binding missing');
  requireThat(required.every(p => plan.files.some(f => f.path === p)), 'bundle file hash inventory incomplete');
  requireThat(JSON.stringify(readJson(path.join(bundle, 'doc.result.json'))) === JSON.stringify(plan.doctests), 'compile doctest evidence changed');
  const doc = parseLibtestResult(readFile(path.join(bundle, 'logs/doctests.log')).toString('utf8'));
  requireThat(plan.doctests.command === 'cargo test --locked --doc' && plan.doctests.exitCode === 0 && plan.doctests.result.exitCode === 0 && doc.failed === 0 && doc.filteredOut === 0 && Object.entries(doc).every(([k, v]) => plan.doctests.result[k] === v), 'compile doctests failed or differ from log');
  return { plan, bundle, context: c };
}

export function runShard(options = {}) {
  requireThat(Number.isInteger(options.index) && options.index >= 0 && options.index < SHARD_COUNT, 'invalid shard index');
  requireThat(typeof options.inJob === 'boolean', 'explicit successful runner Job observation required');
  requireThat(typeof options.artifactName === 'string', 'downloaded compiler artifact name required');
  const { plan, bundle, context: c } = verifyBundle({ ...options, requireCheckoutPath: true });
  const output = path.resolve(options.output ?? path.join(c.root, `src-tauri/target/ci-rust-shard-${options.index}`));
  fs.mkdirSync(path.join(output, 'logs'), { recursive: true });
  requireThat(!fs.existsSync(path.join(output, 'shard-result.json')), 'shard result already exists');
  const result = { schema: 1, policy: POLICY, sourceSha: c.sourceSha, runId: c.runId, runAttempt: c.runAttempt, artifactName: plan.artifactName, planHash: plan.planHash, index: options.index, shardCount: SHARD_COUNT, host: { jobQuerySucceeded: true, inJob: options.inJob }, completed: false, exitCode: 1, durationSeconds: 0, harnesses: [], error: null };
  const started = process.hrtime.bigint();
  try {
    // Restore only hashed runtime payload, at its original source-relative location.
    for (const file of plan.files.filter(f => f.path.startsWith('src-tauri/target/debug/'))) {
      const source = relativeFile(bundle, file.path), destination = path.join(c.root, file.path);
      fs.mkdirSync(path.dirname(destination), { recursive: true }); fs.copyFileSync(source, destination);
      fs.chmodSync(destination, fs.statSync(source).mode & 0o777); verifyFile(c.root, file);
    }
    // Every assigned list is admitted before this shard executes any test body.
    for (const h of plan.harnesses) {
      const names = h.partitions[options.index], executed = names.length > 0 || (h.full.length === 0 && options.index === 0);
      const receipt = { identity: h.identity, names, executed, durationSeconds: 0, logs: executed ? { selected: h.logs.selected, execution: h.logs.execution } : null, result: null };
      result.harnesses.push(receipt);
      if (!executed) continue;
      const rows = inventory(relativeFile(c.root, h.executable), ['--exact', ...names], c, path.join(output, receipt.logs.selected));
      requireThat(equal(rows.map(t => t.name), names) && rows.every(t => h.full.some(f => f.name === t.name && f.type === t.type)), 'raw shard listing differs from exact assignment');
    }
    let failed = false;
    for (const receipt of result.harnesses) {
      if (!receipt.executed) continue;
      const h = plan.harnesses.find(h => identity(h) === identity(receipt));
      const invocation = invoke(relativeFile(c.root, h.executable), ['--exact', ...receipt.names], c.testCwd, c.environment);
      fs.writeFileSync(path.join(output, receipt.logs.execution), invocation.output);
      receipt.durationSeconds = invocation.durationSeconds;
      try {
        receipt.result = { exitCode: invocation.exitCode, ...parseLibtestResult(invocation.output) };
        aggregateHarness({ ...h, selected: receipt.names, ignored: h.ignored.filter(n => receipt.names.includes(n)) }, [{ index: options.index, names: receipt.names, listing: readFile(path.join(output, receipt.logs.selected)).toString('utf8'), output: invocation.output, result: receipt.result, durationSeconds: invocation.durationSeconds }], parseLibtestListing, parseLibtestResult);
      } catch (error) { receipt.error = String(error.message); failed = true; }
      if (invocation.error || invocation.exitCode !== 0) { receipt.error ??= invocation.error ?? `exit ${invocation.exitCode}`; failed = true; }
    }
    result.completed = !failed; result.exitCode = failed ? 1 : 0;
  } catch (error) { result.error = String(error.message); }
  result.durationSeconds = Number(process.hrtime.bigint() - started) / 1e9;
  writeJson(path.join(output, 'shard-result.json'), result);
  return result;
}

function walkFiles(directory, prefix = '') {
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    requireThat(!entry.isSymbolicLink(), 'linked artifact evidence');
    const relative = prefix + entry.name, filename = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...walkFiles(filename, relative + '/'));
    else { requireThat(entry.isFile(), 'non-file artifact evidence'); files.push(relative); }
  }
  return files;
}
function findResults(directory) {
  const results = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    requireThat(!entry.isSymbolicLink(), 'linked shard evidence');
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) results.push(...findResults(filename));
    else if (entry.isFile() && entry.name === 'shard-result.json') results.push(filename);
  }
  return results;
}
export function aggregateResults(options = {}) {
  const { plan, bundle, context: c } = verifyBundle(options);
  const shards = path.resolve(options.shards ?? path.join(c.root, 'src-tauri/target/ci-rust-shards'));
  const coverage = path.resolve(options.coverage ?? path.join(c.root, 'src-tauri/target/windows-native-coverage'));
  const receipts = findResults(shards).map(filename => ({ filename, result: readJson(filename) }));
  requireThat(new Set(receipts.map(r => r.result.index)).size === receipts.length, 'duplicate shard indices');
  requireThat(receipts.length === SHARD_COUNT && equal(receipts.map(r => r.result.index), Array.from({ length: SHARD_COUNT }, (_, i) => i)), 'missing shard indices');
  for (const { result } of receipts) {
    sameBinding(result, c);
    requireThat(result.schema === 1 && result.policy === POLICY && result.shardCount === SHARD_COUNT && result.planHash === plan.planHash && result.artifactName === plan.artifactName, 'shard compiler artifact/plan binding mismatch');
    requireThat(result.host?.jobQuerySucceeded === true && result.host.inJob === plan.host.inJob, 'shard Job observation differs from compilation');
    requireThat(result.completed === true && result.exitCode === 0, 'failed or incomplete shard');
    requireThat(Number.isFinite(result.durationSeconds) && result.durationSeconds >= 0, 'invalid shard duration');
    assertHarnessIdentities(result.harnesses);
  }
  const report = {
    schema: 1, policy: VALIDATION_POLICY, completed: true, sourceSha: c.sourceSha, runId: c.runId, runAttempt: c.runAttempt,
    host: plan.host, harnesses: [], doctests: plan.doctests,
    nativeJobSuite: { status: plan.host.inJob ? 'unverified' : 'executed', reason: plan.host.inJob ? 'external_job' : null, unverifiedNames: plan.host.inJob ? [...JOB_FREE_TESTS] : [] },
    nativeAll: { status: 'unverified', reason: 'original_all_not_run' }, nativeAcceptanceProven: false,
    rustShardRun: {
      policy: POLICY, sourceSha: c.sourceSha, runId: c.runId, runAttempt: c.runAttempt, planHash: plan.planHash, artifactName: plan.artifactName, shardCount: SHARD_COUNT,
      compiler: plan.compiler, bundleFiles: plan.files,
      shards: receipts.map(({ result }) => ({ index: result.index, sourceSha: result.sourceSha, runId: result.runId, runAttempt: result.runAttempt, planHash: result.planHash, artifactName: result.artifactName, host: result.host, completed: result.completed, exitCode: result.exitCode, durationSeconds: result.durationSeconds, harnesses: result.harnesses.map(h => ({ identity: h.identity, names: h.names, executed: h.executed })) })).sort((a, b) => a.index - b.index),
    },
  };
  const mergedLogs = [];
  for (const h of plan.harnesses) {
    const slices = [];
    for (const { filename, result } of receipts.sort((a, b) => a.result.index - b.result.index)) {
      const receipt = result.harnesses.find(r => identity(r) === identity(h)), names = h.partitions[result.index];
      requireThat(equal(receipt.names, names), 'shard names differ from compiled partition');
      const execute = names.length > 0 || (h.full.length === 0 && result.index === 0);
      requireThat(receipt.executed === execute, 'empty shard execution policy changed');
      if (!execute) { requireThat(receipt.result === null && receipt.logs === null && receipt.durationSeconds === 0, 'skipped shard contains execution claims'); continue; }
      requireThat(JSON.stringify(receipt.logs) === JSON.stringify({ selected: h.logs.selected, execution: h.logs.execution }), 'shard raw log paths changed');
      const directory = path.dirname(filename);
      slices.push({ index: result.index, names: receipt.names, listing: readFile(relativeFile(directory, receipt.logs.selected)).toString('utf8'), output: readFile(relativeFile(directory, receipt.logs.execution)).toString('utf8'), result: receipt.result, durationSeconds: receipt.durationSeconds });
    }
    const { execution, executable, partitions, ...merged } = aggregateHarness(h, slices, parseLibtestListing, parseLibtestResult);
    report.harnesses.push(merged); mergedLogs.push({ filename: h.logs.execution, content: execution });
  }
  validateNativeCoverage(report, c);
  fs.mkdirSync(path.join(coverage, 'logs'), { recursive: true });
  for (const h of plan.harnesses) for (const phase of ['full', 'ignored', 'selected']) fs.copyFileSync(relativeFile(bundle, h.logs[phase]), path.join(coverage, h.logs[phase]));
  fs.copyFileSync(path.join(bundle, 'logs/doctests.log'), path.join(coverage, 'logs/doctests.log'));
  for (const log of mergedLogs) fs.writeFileSync(path.join(coverage, log.filename), log.content);
  writeJson(path.join(coverage, REPORT_FILENAME), report);
  readNativeCoverageArtifact(coverage, c);
  return report;
}

function cliOptions(args) {
  const accepted = new Set(['root', 'cargo-json', 'bundle', 'in-job', 'doc-exit-code', 'output', 'index', 'artifact-name', 'shards', 'coverage']);
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i]?.replace(/^--/, '');
    requireThat(args[i]?.startsWith('--') && accepted.has(key) && args[i + 1] !== undefined && !Object.hasOwn(options, key), 'invalid/duplicate CLI option');
    options[key] = args[i + 1];
  }
  if (options['in-job'] !== undefined) requireThat(['true', 'false'].includes(options['in-job']), '--in-job must be true or false');
  return Object.fromEntries(Object.entries(options).map(([key, value]) => [key.replace(/-([a-z])/g, (_, c) => c.toUpperCase()), key === 'in-job' ? value === 'true' : ['index', 'doc-exit-code'].includes(key) ? Number(value) : value]));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [action, ...args] = process.argv.slice(2);
    if (action === 'artifact-name' && args.length === 3) console.log(bundleArtifactName(args[0], args[1], Number(args[2])));
    else if (action === 'shard-artifact-name' && args.length === 4) console.log(shardArtifactName(args[0], args[1], Number(args[2]), Number(args[3])));
    else {
      const options = cliOptions(args);
      let value;
      if (action === 'plan') value = createPlan(options);
      else if (action === 'run') value = runShard(options);
      else if (action === 'aggregate') value = aggregateResults(options);
      else throw new Error('Expected artifact-name, shard-artifact-name, plan, run or aggregate');
      console.log(JSON.stringify({ action, planHash: value.planHash, artifactName: value.artifactName, completed: value.completed, exitCode: value.exitCode }));
      if (action === 'run' && !value.completed) process.exitCode = 1;
    }
  } catch (error) { console.error(String(error.message)); process.exitCode = 1; }
}
