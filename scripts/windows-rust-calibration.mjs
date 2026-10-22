// 从 PR43 复用有界测量思路；回执仅供候选比较，绝不成为原生覆盖或发布准入。
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { verifyBundle, executeHarness } from './windows-rust-shard-runner.mjs';
import { parseLibtestListing, parseLibtestResult } from './windows-native-validation.mjs';

export const CALIBRATION_GROUP_COUNT = 8;
export const CALIBRATION_BUDGET_MS = 8 * 60 * 1000;
const TARGET_FILE = 'scripts/windows-rust-calibration-targets.json';
function requireThat(ok, message) { if (!ok) throw new Error(`Rust calibration: ${message}`); }
export function calibrationEnabled(value) {
  requireThat([undefined, '', 'false', 'true'].includes(value), 'invalid explicit opt-in');
  return value === 'true';
}
export function calibrationGroups(value) {
  requireThat(value?.schema === 1, 'invalid calibration schema');
  const names = value.targets;
  requireThat(Array.isArray(names) && names.length > 0 && names.length <= 16 && names.every(n => typeof n === 'string' && /^[A-Za-z0-9_]+(?:::[A-Za-z0-9_]+)+$/.test(n) && n.length <= 500), 'invalid bounded exact targets');
  requireThat(new Set(names).size === names.length, 'duplicate calibration target');
  const groups = Array.from({ length: CALIBRATION_GROUP_COUNT }, () => []);
  [...names].sort().forEach((name, index) => groups[index % groups.length].push(name));
  return groups;
}
export function calibrationWeights(plan, receipts, targets, targetHash) {
  const groups = calibrationGroups(targets), weights = Object.create(null);
  requireThat(/^[a-f0-9]{64}$/.test(targetHash ?? ''), 'target file hash required');
  requireThat(Array.isArray(receipts) && receipts.length === groups.length
    && new Set(receipts.map(r => r.index)).size === groups.length, 'complete unique calibration groups required');
  for (const r of receipts) {
    requireThat(Number.isInteger(r.index) && r.index >= 0 && r.index < groups.length
      && r.schema === 1 && r.kind === 'rust-exact-name-calibration-v1' && r.groupCount === groups.length
      && r.sourceSha === plan.sourceSha && r.runId === plan.runId && r.runAttempt === plan.runAttempt
      && r.planHash === plan.planHash && r.artifactName === plan.artifactName && r.targetFileHash === targetHash,
    'calibration source/run/attempt/plan/targets differ');
    requireThat(r.completed === true && r.exitCode === 0 && r.error == null && r.nativeAcceptanceProven === false
      && JSON.stringify(r.compiler) === JSON.stringify(plan.compiler) && JSON.stringify(r.host) === JSON.stringify(plan.host),
    'incomplete calibration or compiler/host mismatch');
    requireThat(JSON.stringify(r.targets) === JSON.stringify(groups[r.index]) && Array.isArray(r.measurements)
      && r.measurements.length === r.targets.length, 'measurement inventory differs');
    for (const [ordinal, m] of r.measurements.entries()) {
      const owners = plan.harnesses.filter(h => h.selected.includes(m.name) && !h.ignored.includes(m.name)
        && h.full.some(t => t.name === m.name && t.type === 'test'));
      requireThat(m.name === r.targets[ordinal] && owners.length === 1 && !Object.hasOwn(weights, m.name), 'invented/ignored/duplicate measurement');
      const h = owners[0], executable = plan.files.find(f => f.path === h.executable);
      requireThat(executable && m.executable === h.executable && m.executableSha256 === executable.sha256
        && JSON.stringify(m.identity) === JSON.stringify(h.identity) && JSON.stringify(m.argv) === JSON.stringify(['--exact', m.name]),
      'measurement executable/selector differs');
      requireThat(m.completed === true && m.exitCode === 0 && m.watchdog?.timedOut === false && m.watchdog.outputIncomplete === false
        && typeof m.durationSeconds === 'number' && Number.isFinite(m.durationSeconds) && m.durationSeconds > 0 && m.durationSeconds <= CALIBRATION_BUDGET_MS / 1000
        && /^[a-f0-9]{64}$/.test(m.logHash ?? '') && /^[a-f0-9]{64}$/.test(m.listingHash ?? ''), 'incomplete or invalid measurement');
      const result = m.result;
      requireThat(result?.passed === 1 && result.failed === 0 && result.ignored === 0 && result.measured === 0
        && result.filteredOut === h.full.length - 1, 'measurement is not one passed original test');
      weights[m.name] = m.durationSeconds;
    }
  }
  requireThat(Object.keys(weights).length === targets.targets.length, 'target measurements missing');
  return weights;
}

// 结构回执不能代替原始日志；从已验证 plan 的同一组目录重读并核对正文。
export function readCalibrationWeights(plan, directories, targets, targetHash) {
  requireThat(Array.isArray(directories) && directories.length === CALIBRATION_GROUP_COUNT, 'complete receipt directories required');
  const read = (directory, relative, maximum) => {
    requireThat(/^(?:calibration-result\.json|logs\/[0-9]+-(?:selected|execution)\.log)$/.test(relative ?? ''), 'invalid calibration log path');
    const root = fs.realpathSync(directory), filename = path.join(root, relative), stat = fs.lstatSync(filename);
    requireThat(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0 && stat.size <= maximum
      && fs.realpathSync(filename).startsWith(root + path.sep), 'linked/empty/oversized calibration log');
    return fs.readFileSync(filename);
  };
  const receipts = directories.map(directory => {
    const r = JSON.parse(read(directory, 'calibration-result.json', 1024 * 1024).toString('utf8'));
    requireThat(Array.isArray(r.measurements), 'measurement inventory missing');
    for (const m of r.measurements) {
      const listed = read(directory, m.logs?.selected, 64 * 1024), output = read(directory, m.logs?.execution, 64 * 1024 * 1024);
      requireThat(digest(listed) === m.listingHash && digest(output) === m.logHash, 'raw measurement logs changed');
      const inventory = parseLibtestListing(listed.toString('utf8')), result = parseLibtestResult(output.toString('utf8'));
      requireThat(inventory.length === 1 && inventory[0].type === 'test' && inventory[0].name === m.name
        && Object.entries(result).every(([key, value]) => m.result?.[key] === value), 'raw original name/result differs');
    }
    return r;
  });
  return calibrationWeights(plan, receipts, targets, targetHash);
}
function writeReceipt(filename, value) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename + '.tmp', JSON.stringify(value, null, 2) + '\n'); fs.renameSync(filename + '.tmp', filename);
}
const digest = data => createHash('sha256').update(data).digest('hex');

export async function runCalibration(options = {}) {
  const admittedAt = Date.now(), started = process.hrtime.bigint();
  const jobStartMs = options.jobStartMs ?? admittedAt, budgetMs = options.budgetMs ?? CALIBRATION_BUDGET_MS;
  requireThat(Number.isSafeInteger(jobStartMs) && jobStartMs > 0 && jobStartMs <= admittedAt, 'invalid first-step clock');
  requireThat(Number.isSafeInteger(budgetMs) && budgetMs > 0 && budgetMs <= CALIBRATION_BUDGET_MS, 'invalid bounded measurement window');
  requireThat(Number.isInteger(options.index) && options.index >= 0 && options.index < CALIBRATION_GROUP_COUNT, 'invalid calibration group');
  requireThat(typeof options.inJob === 'boolean' && typeof options.elevated === 'boolean' && typeof options.artifactName === 'string', 'explicit host/artifact identity required');
  const { plan, bundle, context: c } = verifyBundle({ ...options, requireCheckoutPath: true });
  const calibrationInputs = new Map();
  const calibrationContentHashes = [TARGET_FILE, 'scripts/windows-rust-calibration.mjs'].map(relative => {
    const local = fs.readFileSync(path.join(c.root, relative));
    const committed = spawnSync('git', ['show', `${c.sourceSha}:${relative}`], { cwd: c.root, env: c.environment,
      windowsHide: true, timeout: 10000, maxBuffer: 1024 * 1024 });
    requireThat(committed.status === 0 && committed.stdout && digest(committed.stdout) === digest(local), 'calibration code/targets are not exact committed source');
    calibrationInputs.set(relative, local);
    return { path: relative, sha256: digest(local) };
  });
  const groups = calibrationGroups(JSON.parse(calibrationInputs.get(TARGET_FILE).toString('utf8')));
  requireThat(groups[options.index].length > 0, 'calibration group must contain an original target');
  for (const name of groups.flat()) {
    const owners = plan.harnesses.filter(h => h.full.some(t => t.name === name && t.type === 'test'));
    requireThat(owners.length === 1 && owners[0].selected.includes(name) && !owners[0].ignored.includes(name), 'target must be one selected nonignored original test');
  }
  const consumer = {
    runnerOs: c.environment.RUNNER_OS ?? process.platform, runnerArch: c.environment.RUNNER_ARCH ?? process.arch,
    profileDevDebug: c.environment.CARGO_PROFILE_DEV_DEBUG ?? null, profileTestDebug: c.environment.CARGO_PROFILE_TEST_DEBUG ?? null,
  };
  requireThat(Object.entries(consumer).every(([key, value]) => plan.compiler[key] === value), 'consumer OS/architecture/profile differs from producer');
  const output = path.resolve(options.output ?? path.join(c.root, `src-tauri/target/ci-rust-calibration-${options.index}`));
  fs.mkdirSync(path.join(output, 'logs'), { recursive: true });
  const resultPath = path.join(output, 'calibration-result.json');
  const remainingMs = () => Math.floor(budgetMs - (admittedAt - jobStartMs) - Number(process.hrtime.bigint() - started) / 1e6);
  requireThat(!fs.existsSync(resultPath), 'calibration receipt already exists');
  const result = {
    schema: 1, kind: 'rust-exact-name-calibration-v1', sourceSha: c.sourceSha, runId: c.runId, runAttempt: c.runAttempt,
    planHash: plan.planHash, artifactName: plan.artifactName, targetFileHash: digest(calibrationInputs.get(TARGET_FILE)),
    index: options.index, groupCount: groups.length, targets: groups[options.index], calibrationContentHashes, compiler: plan.compiler, consumer, host: plan.host,
    durationSemantics: 'one exact original libtest process invocation; includes launch and complete output/child shutdown',
    nativeAcceptanceProven: false, completed: false, exitCode: 1, measurements: [], error: null,
  };
  const persist = () => writeReceipt(resultPath, result);
  persist();
  try {
    // 只恢复同一源码/run/attempt 的已核验产物，不另加编译或 Job 包装。
    for (const file of plan.files.filter(f => f.path.startsWith('src-tauri/target/debug/'))) {
      requireThat(remainingMs() > 0, 'shared calibration window exhausted');
      const source = path.join(bundle, file.path), destination = path.join(c.root, file.path);
      fs.mkdirSync(path.dirname(destination), { recursive: true }); fs.copyFileSync(source, destination);
      fs.chmodSync(destination, fs.statSync(source).mode & 0o777);
      requireThat(digest(fs.readFileSync(destination)) === file.sha256, 'restored payload hash mismatch');
    }
    for (const [ordinal, name] of result.targets.entries()) {
      const h = plan.harnesses.find(h => h.selected.includes(name)), argv = ['--exact', name];
      const listingPath = `logs/${ordinal}-selected.log`, executionPath = `logs/${ordinal}-execution.log`;
      const executable = path.join(c.root, h.executable);
      const listingBudgetMs = remainingMs();
      requireThat(listingBudgetMs > 0, 'shared calibration window exhausted before exact listing');
      const listed = spawnSync(executable, [...argv, '--list', '--format', 'pretty'], { cwd: c.testCwd, env: c.environment, encoding: 'utf8', windowsHide: true, timeout: Math.min(30000, listingBudgetMs), maxBuffer: 64 * 1024 });
      fs.writeFileSync(path.join(output, listingPath), listed.stdout ?? '');
      const inventory = parseLibtestListing(listed.stdout ?? '');
      requireThat(listed.status === 0 && inventory.length === 1 && inventory[0].name === name && inventory[0].type === 'test', 'exact listing must contain one original test');
      const executionBudgetMs = remainingMs();
      requireThat(executionBudgetMs > 0, 'shared calibration window exhausted');
      const measurement = { name, identity: h.identity, executable: h.executable, executableSha256: plan.files.find(f => f.path === h.executable).sha256,
        listingHash: digest(fs.readFileSync(path.join(output, listingPath))), argv, cwd: 'src-tauri', logs: { selected: listingPath, execution: executionPath }, completed: false, exitCode: 1, durationSeconds: null, result: null };
      result.measurements.push(measurement); persist();
      const invocation = await executeHarness(executable, argv, {
        root: c.testCwd, environment: c.environment, assignedNames: [name], executionLog: path.join(output, executionPath), timeoutMs: executionBudgetMs,
        onProgress: progress => options.onPhase?.({ phase: 'calibration-progress', index: options.index, name, ...progress }),
      });
      measurement.exitCode = invocation.exitCode; measurement.durationSeconds = invocation.durationSeconds;
      measurement.watchdog = { timedOut: invocation.timedOut, outputIncomplete: invocation.outputIncomplete };
      measurement.result = parseLibtestResult(invocation.output);
      measurement.logHash = digest(fs.readFileSync(path.join(output, executionPath)));
      const r = measurement.result;
      requireThat(invocation.exitCode === 0 && !invocation.error && !invocation.outputIncomplete && r.passed === 1 && r.failed === 0 && r.ignored === 0 && r.measured === 0 && r.filteredOut === h.full.length - 1, 'calibration requires one actual passed original test and complete exit');
      measurement.completed = true; persist();
      options.onPhase?.({ phase: 'calibration-complete', index: options.index, name, durationSeconds: measurement.durationSeconds, result: measurement.result });
    }
    result.completed = true; result.exitCode = 0;
  } catch (error) {
    result.error = String(error.message).startsWith('Rust calibration: ') ? String(error.message) : 'Rust calibration: raw receipt/log recording failed or output incomplete';
  }
  persist(); return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [action, ...args] = process.argv.slice(2);
    if (action === 'enabled' && args.length === 0) console.log(String(calibrationEnabled(process.env.CALIBRATION_OPT_IN)));
    else {
      requireThat(action === 'run', 'expected enabled or run');
      const accepted = ['index', 'job-start-ms', 'in-job', 'elevated', 'artifact-name', 'output'], values = {};
      for (let i = 0; i < args.length; i += 2) {
        const key = args[i]?.slice(2); requireThat(args[i]?.startsWith('--') && accepted.includes(key) && args[i + 1] !== undefined && !Object.hasOwn(values, key), 'invalid/duplicate option'); values[key] = args[i + 1];
      }
      requireThat(['true', 'false'].includes(values['in-job']) && ['true', 'false'].includes(values.elevated), 'invalid host observation');
      const value = await runCalibration({ index: Number(values.index), jobStartMs: Number(values['job-start-ms']), inJob: values['in-job'] === 'true', elevated: values.elevated === 'true', artifactName: values['artifact-name'], output: values.output, onPhase: phase => console.log(JSON.stringify(phase)) });
      console.log(JSON.stringify(value)); if (!value.completed) process.exitCode = 1;
    }
  } catch { console.error('Rust calibration: recording rejected; inspect retained receipt and raw logs'); process.exitCode = 1; }
}
