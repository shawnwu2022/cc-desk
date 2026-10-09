import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateShardExecution, validateRustShardRun } from './windows-rust-shards.mjs';

export const VALIDATION_POLICY = 'required-checks-and-disclosed-host-unverified-v1';
export const REPORT_FILENAME = 'windows-native-coverage.json';
export const MAX_REPORT_BYTES = 4 * 1024 * 1024;
const MAX_LOG_BYTES = 64 * 1024 * 1024;
const scope = JSON.parse(fs.readFileSync(new URL('./windows-native-scope.json', import.meta.url), 'utf8'));
if (scope.schema !== 1 || scope.reason !== 'external_job' || scope.jobFreeTests.length !== 18 || new Set(scope.jobFreeTests).size !== 18) {
  throw new Error('Invalid checked-in Windows native scope');
}
export const JOB_FREE_TESTS = Object.freeze([...scope.jobFreeTests]);

function requireThat(condition, message) { if (!condition) throw new Error(`Native coverage: ${message}`); }
function binding(sourceSha, runId, runAttempt) {
  requireThat(typeof sourceSha === 'string' && /^[a-f0-9]{40}$/.test(sourceSha), 'invalid source binding');
  requireThat(/^[1-9][0-9]{0,19}$/.test(String(runId)), 'invalid run binding');
  requireThat(Number.isSafeInteger(runAttempt) && runAttempt > 0 && runAttempt <= 10000, 'invalid attempt binding');
  return { sourceSha, runId: String(runId), runAttempt };
}
export function coverageArtifactName(sourceSha, runId, runAttempt) {
  const b = binding(sourceSha, runId, runAttempt);
  return `windows-native-coverage-${b.sourceSha}-${b.runId}-${b.runAttempt}`;
}
function names(values, label) {
  requireThat(Array.isArray(values) && values.length <= 20000, `${label} inventory missing or oversized`);
  requireThat(values.every(n => typeof n === 'string' && n.length > 0 && n.length <= 500 && !/[\r\n\x00]/.test(n)), `${label} invalid name`);
  requireThat(new Set(values).size === values.length, `${label} duplicate name`);
  return values;
}
function same(a, b) { return a.length === b.length && a.every(n => b.includes(n)); }
function logName(value) {
  requireThat(typeof value === 'string' && /^logs\/[A-Za-z0-9_.-]+\.log$/.test(value) && !value.includes('..'), 'invalid or missing log');
  return value;
}
function resultCounts(result, label) {
  requireThat(result && Number.isSafeInteger(result.exitCode), `${label} result/exit missing`);
  for (const key of ['passed', 'failed', 'ignored', 'measured', 'filteredOut']) {
    requireThat(Number.isSafeInteger(result[key]) && result[key] >= 0, `${label} invalid count ${key}`);
  }
  requireThat(result.exitCode === 0 && result.failed === 0, `${label} executed tests failed`);
  return result.passed + result.failed + result.ignored + result.measured;
}

export function validateNativeCoverage(report, context) {
  const expected = binding(context.sourceSha, context.runId, context.runAttempt);
  requireThat(report && Buffer.byteLength(JSON.stringify(report), 'utf8') <= MAX_REPORT_BYTES, 'report oversized or missing');
  requireThat(report.schema === 1 && report.policy === VALIDATION_POLICY, 'unsupported report policy/schema');
  requireThat(report.completed === true, 'incomplete report');
  requireThat(report.sourceSha === expected.sourceSha && report.runId === expected.runId && report.runAttempt === expected.runAttempt, 'source/run/attempt binding mismatch');
  requireThat(report.host?.jobQuerySucceeded === true, 'host Job query did not succeed');
  requireThat(typeof report.host.inJob === 'boolean', 'invalid observed host Job state');
  requireThat(Array.isArray(report.harnesses) && report.harnesses.length === scope.harnesses.length, 'default harness coverage missing');
  const identities = report.harnesses.map(h => `${h.identity?.kind}:${h.identity?.name}`);
  requireThat(new Set(identities).size === identities.length && same(identities, scope.harnesses.map(h => `${h.kind}:${h.name}`)), 'default harness identity mismatch');
  if (report.rustShardRun !== undefined || report.harnesses.some(h => h.shards !== undefined)) validateRustShardRun(report, expected);
  const counts = { passed: 0, failed: 0, ignored: 0, measured: 0, filteredOut: 0, totalInventory: 0, selected: 0, executed: 0, excluded: 0 };
  const allLogs = [];
  for (const h of report.harnesses) {
    requireThat(Array.isArray(h.full) && h.full.every(t => t && ['test', 'benchmark'].includes(t.type)), 'invalid full inventory records');
    const full = names(h.full.map(t => t.name), 'full');
    const ignored = names(h.ignored, 'ignored');
    const selected = names(h.selected, 'selected');
    const excluded = names(h.excluded, 'excluded');
    requireThat(ignored.every(n => full.includes(n)), 'ignored inventory not in full');
    const library = h.identity.kind === 'lib';
    const requiredExclusion = library && report.host.inJob ? JOB_FREE_TESTS : [];
    requireThat(same(excluded, requiredExclusion), 'unexpected exclusion set');
    if (library) {
      requireThat(JOB_FREE_TESTS.every(n => full.includes(n)), 'reviewed Job-free policy drift');
      requireThat(JOB_FREE_TESTS.every(n => !ignored.includes(n)), 'reviewed Job-free policy entry became ignored');
      requireThat(scope.requiredSelectedTests.every(n => full.includes(n) && selected.includes(n) && !ignored.includes(n)), 'required ordinary/Wry test not selected');
      requireThat(selected.some(n => !ignored.includes(n) && h.full.find(t => t.name === n).type === 'test'), 'library selected nonignored inventory is empty');
    } else {
      requireThat(!full.some(n => JOB_FREE_TESTS.includes(n)), 'Job-free policy test moved outside library');
    }
    requireThat(excluded.every(n => full.includes(n) && !ignored.includes(n)), 'exclusion missing or ignored');
    for (const excludedName of excluded) {
      requireThat(!full.some(n => n !== excludedName && n.includes(excludedName)), 'libtest full-name skip substring collision');
    }
    requireThat(same(selected, full.filter(n => !excluded.includes(n))), 'selection inventory differs from full minus exclusions');
    requireThat(Array.isArray(h.defaultIgnored) && h.defaultIgnored.length === ignored.length && same(names(h.defaultIgnored.map(i => i.name), 'ignored classification'), ignored), 'ignored classification mismatch');
    requireThat(h.defaultIgnored.every(i => i.classification === 'original-default-ignore' && (i.reason === null || typeof i.reason === 'string')), 'ignored reason/classification missing');
    for (const phase of ['full', 'ignored', 'selected', 'execution']) allLogs.push(logName(h.logs?.[phase]));
    const executedCount = resultCounts(h.result, `harness ${h.identity.name}`);
    requireThat(executedCount === selected.length, 'outer result/inventory count mismatch');
    requireThat(h.result.ignored === ignored.length, 'original ignored count changed');
    requireThat(h.result.filteredOut === excluded.length, 'filtered count differs from exact exclusions');
    for (const key of ['passed', 'failed', 'ignored', 'measured', 'filteredOut']) counts[key] += h.result[key];
    counts.totalInventory += full.length; counts.selected += selected.length;
    counts.executed += selected.length - ignored.length; counts.excluded += excluded.length;
  }
  const doc = report.doctests;
  requireThat(doc?.command === 'cargo test --locked --doc' && doc.exitCode === 0, 'doctest command/outcome missing or failed');
  resultCounts(doc.result, 'doctests');
  requireThat(doc.result.filteredOut === 0, 'doctest filtering unsupported');
  requireThat(Array.isArray(doc.logs) && doc.logs.length === 1, 'doctest log missing');
  allLogs.push(logName(doc.logs[0]));
  requireThat(new Set(allLogs).size === allLogs.length, 'duplicate log reference');
  for (const key of ['passed', 'failed', 'ignored', 'measured', 'filteredOut']) counts[key] += doc.result[key];
  counts.executed += doc.result.passed + doc.result.failed + doc.result.measured;
  const unavailable = report.host.inJob ? JOB_FREE_TESTS : [];
  requireThat(report.nativeJobSuite?.status === (report.host.inJob ? 'unverified' : 'executed') && report.nativeJobSuite.reason === (report.host.inJob ? 'external_job' : null), 'specialist disclosure mismatch');
  requireThat(same(names(report.nativeJobSuite.unverifiedNames, 'disclosure'), unavailable), 'specialist disclosure names mismatch');
  requireThat(report.nativeAll?.status === 'unverified' && report.nativeAll.reason === 'original_all_not_run', 'original All claim is unsupported');
  requireThat(report.nativeAcceptanceProven === false, 'actual roundtrip acceptance is not proven');
  return {
    policy: VALIDATION_POLICY, ...expected, counts, harnessCount: report.harnesses.length,
    unverifiedNames: [...unavailable], nativeAllStatus: 'unverified', nativeAcceptanceProven: false,
    executed: counts.executed, ignored: counts.ignored, unverified: unavailable.length,
  };
}

export function parseLibtestListing(text) {
  text = text.replaceAll('\r\n', '\n');
  const rows = text.split(/\r?\n/).flatMap(line => {
    const m = /^(.+): (test|benchmark)$/.exec(line); return m ? [{ name: m[1], type: m[2] }] : [];
  });
  names(rows.map(r => r.name), 'raw listing');
  const footers = [...text.matchAll(/^(\d+) tests?, (\d+) benchmarks?$/gm)];
  requireThat(footers.length === 1 && Number(footers[0][1]) === rows.filter(t => t.type === 'test').length && Number(footers[0][2]) === rows.filter(t => t.type === 'benchmark').length, 'raw listing count/footer mismatch');
  return rows;
}
export function parseLibtestResult(text) {
  const summaries = [...text.matchAll(/^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;/gm)];
  requireThat(summaries.length > 0, 'raw execution summary missing');
  const m = summaries.at(-1);
  return Object.fromEntries(['passed', 'failed', 'ignored', 'measured', 'filteredOut'].map((key, i) => [key, Number(m[i + 1])]));
}
function readBounded(filename, maximum, label) {
  const stat = fs.lstatSync(filename);
  requireThat(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0 && stat.size <= maximum, `${label} missing, empty, linked or oversized`);
  return fs.readFileSync(filename, 'utf8');
}
export function readNativeCoverageArtifact(directory, context) {
  const root = fs.realpathSync(directory);
  const report = JSON.parse(readBounded(path.join(root, REPORT_FILENAME), MAX_REPORT_BYTES, 'report'));
  const summary = validateNativeCoverage(report, context);
  const readLog = relative => {
    const filename = path.join(root, logName(relative));
    requireThat(path.dirname(fs.realpathSync(filename)) === path.join(root, 'logs'), 'log escaped artifact');
    return readBounded(filename, MAX_LOG_BYTES, 'log');
  };
  for (const h of report.harnesses) {
    const full = parseLibtestListing(readLog(h.logs.full));
    requireThat(JSON.stringify(full) === JSON.stringify(h.full), 'raw full log differs from report');
    const ignored = parseLibtestListing(readLog(h.logs.ignored));
    const selected = parseLibtestListing(readLog(h.logs.selected));
    requireThat(same(ignored.map(t => t.name), h.ignored) && same(selected.map(t => t.name), h.selected), 'raw selected/ignored log differs from report');
    requireThat([...ignored, ...selected].every(t => full.some(original => original.name === t.name && original.type === t.type)), 'raw inventory type changed');
    const execution = readLog(h.logs.execution).replaceAll('\r\n', '\n');
    if (h.shards !== undefined || execution.startsWith('CCDESK_SHARD_START ')) {
      validateShardExecution(h, execution, parseLibtestListing, parseLibtestResult);
    }
    const actual = parseLibtestResult(execution);
    requireThat(Object.entries(actual).every(([k, v]) => h.result[k] === v), 'raw outer result differs from report');
    const observedIgnored = new Map([...execution.matchAll(/^test (.+) \.\.\. ignored(?:, (.*))?$/gm)].map(m => [m[1], m[2] ?? null]));
    requireThat(h.defaultIgnored.every(i => observedIgnored.has(i.name) && observedIgnored.get(i.name) === i.reason), 'raw original ignored reasons differ from report');
  }
  const actualDoc = parseLibtestResult(readLog(report.doctests.logs[0]));
  requireThat(Object.entries(actualDoc).every(([k, v]) => report.doctests.result[k] === v), 'raw doctest result differs from report');
  return { report, summary };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [action, ...args] = process.argv.slice(2);
  if (action === 'artifact-name' && args.length === 3) {
    console.log(coverageArtifactName(args[0], args[1], Number(args[2])));
  } else if (action === 'validate' && args.length === 4) {
    console.log(JSON.stringify(readNativeCoverageArtifact(args[0], { sourceSha: args[1], runId: args[2], runAttempt: Number(args[3]) }).summary));
  } else {
    throw new Error('Expected artifact-name SHA RUN ATTEMPT or validate DIRECTORY SHA RUN ATTEMPT');
  }
}
