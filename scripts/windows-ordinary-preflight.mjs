// Diagnostic gate on the existing immutable compiler bundle, never coverage.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { executeHarness, verifyBundle } from './windows-rust-shard-runner.mjs';
import { parseLibtestResult } from './windows-native-validation.mjs';
const PREFIX = 'version_history::windows::process::cancel_tests::';
export const ORDINARY_NATIVE_NAMES = [
  'OrdinaryInstaller_DurableHandoffAndExactOnceResume_020',
  'OrdinaryInstaller_LiveWorkerSurvivesOwnerDrop_022',
  'OrdinaryInstaller_MissingLifetimeSpendsResumeWithoutRearming_024',
  'OrdinaryInstaller_PriorAppliedObservationBindsExactLaunch_025',
].map(name => PREFIX + name);
export const ORDINARY_CONTRACT_NAMES = [
  'OrdinaryInstaller_AdmissionFailureKeepsArmedCleanupAndSpendsAttempt_027',
  'OrdinaryInstaller_ElevatedTokenRejectsBeforeDisarm_028',
  'OrdinaryInstaller_FixturePanicCleansExactDisarmedChild_029',
].map(name => PREFIX + name);
export const FIXTURE_CLEANUP_NAMES = [
  'CancelBeforeResume_RecordFailureCannotRetryOrResume_005',
  'OrdinaryInstaller_FixtureCleanupRequiresExactTerminalHandle_030',
].map(name => PREFIX + name);
export function ordinaryPreflightNames(elevated) {
  if (typeof elevated !== 'boolean') throw new Error('Actual elevation observation required');
  return [...ORDINARY_CONTRACT_NAMES, ...FIXTURE_CLEANUP_NAMES, ...(elevated ? [] : ORDINARY_NATIVE_NAMES)];
}
export async function preflight(options) {
  const { plan, context: c } = verifyBundle({ ...options, requireCheckoutPath: true });
  if (plan.host.elevationQuerySucceeded !== true || plan.host.elevated !== options.elevated) throw new Error('Compiler/host elevation mismatch');
  const h = plan.harnesses.find(h => h.identity.kind === 'lib');
  const names = ordinaryPreflightNames(options.elevated);
  if (!h || [...ORDINARY_NATIVE_NAMES, ...ORDINARY_CONTRACT_NAMES, ...FIXTURE_CLEANUP_NAMES].some(name => !h.full.some(row => row.name === name) || h.ignored.includes(name))) throw new Error('Complete ordinary preflight inventory required');
  const output = path.join(c.root, 'src-tauri/target/ci-rust-ordinary-preflight');
  fs.mkdirSync(output, { recursive: true });
  const result = await executeHarness(path.join(c.root, h.executable), ['--exact', ...names], {
    root: c.testCwd, environment: c.environment, assignedNames: names,
    executionLog: path.join(output, 'execution.log'), timeoutMs: 120000,
    onProgress: progress => console.log(JSON.stringify({ phase: 'ordinary-preflight', ...progress })),
  });
  console.log(JSON.stringify({ phase: 'ordinary-preflight-end', sourceSha: c.sourceSha, runId: c.runId, runAttempt: c.runAttempt, elevated: options.elevated, nativeUnverifiedNames: options.elevated ? ORDINARY_NATIVE_NAMES : [], exitCode: result.exitCode, timedOut: result.timedOut, pendingNames: result.pendingNames }));
  let summary;
  try { summary = parseLibtestResult(result.output.replaceAll('\r\n', '\n')); }
  catch (error) { console.error(result.output.slice(-65536)); throw error; }
  if (result.exitCode !== 0 || result.outputIncomplete || summary.failed !== 0 || summary.passed !== names.length || summary.ignored !== 0 || summary.filteredOut !== h.full.length - names.length) {
    console.error(result.output.slice(-65536));
    throw new Error('Ordinary exact preflight failed; full inventory cannot proceed');
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [inJob, elevated] = process.argv.slice(2);
    if (!['true', 'false'].includes(inJob) || !['true', 'false'].includes(elevated) || process.argv.length !== 4) throw new Error('Actual host Job and elevation observations required');
    await preflight({ inJob: inJob === 'true', elevated: elevated === 'true' });
  } catch (error) { console.error(String(error.message)); process.exitCode = 1; }
}
