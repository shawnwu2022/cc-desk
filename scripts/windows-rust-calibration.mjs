// Bounded advisory measurement. A calibration receipt is never native coverage.
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
  const groups = calibrationGroups(JSON.parse(fs.readFileSync(path.join(c.root, TARGET_FILE), 'utf8')));
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
    planHash: plan.planHash, artifactName: plan.artifactName, targetFileHash: digest(fs.readFileSync(path.join(c.root, TARGET_FILE))),
    index: options.index, groupCount: groups.length, targets: groups[options.index], compiler: plan.compiler, consumer, host: plan.host,
    durationSemantics: 'one exact original libtest process invocation; includes launch and complete output/child shutdown',
    nativeAcceptanceProven: false, completed: false, exitCode: 1, measurements: [], error: null,
  };
  const persist = () => writeReceipt(resultPath, result);
  persist();
  try {
    // Restore the exact source/run/attempt payload. No compiler or extra Job wrapper.
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
      const measurement = { name, identity: h.identity, executable: h.executable, argv, cwd: 'src-tauri', logs: { selected: listingPath, execution: executionPath }, completed: false, exitCode: 1, durationSeconds: null, result: null };
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
      const accepted = ['index', 'job-start-ms', 'in-job', 'elevated', 'artifact-name'], values = {};
      for (let i = 0; i < args.length; i += 2) {
        const key = args[i]?.slice(2); requireThat(args[i]?.startsWith('--') && accepted.includes(key) && args[i + 1] !== undefined && !Object.hasOwn(values, key), 'invalid/duplicate option'); values[key] = args[i + 1];
      }
      requireThat(['true', 'false'].includes(values['in-job']) && ['true', 'false'].includes(values.elevated), 'invalid host observation');
      const value = await runCalibration({ index: Number(values.index), jobStartMs: Number(values['job-start-ms']), inJob: values['in-job'] === 'true', elevated: values.elevated === 'true', artifactName: values['artifact-name'], onPhase: phase => console.log(JSON.stringify(phase)) });
      console.log(JSON.stringify(value)); if (!value.completed) process.exitCode = 1;
    }
  } catch { console.error('Rust calibration: recording rejected; inspect retained receipt and raw logs'); process.exitCode = 1; }
}
