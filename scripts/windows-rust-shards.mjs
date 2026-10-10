// Pure deterministic partitioning and fail-closed raw libtest reconciliation.
export const RUST_SHARD_COUNT = 16;
function requireThat(ok, message) { if (!ok) throw new Error(`Rust shards: ${message}`); }
function unique(names) {
  requireThat(Array.isArray(names) && names.every(n => typeof n === 'string' && n.length > 0 && n.length <= 500 && !/[\r\n\0]/.test(n)), 'invalid inventory');
  requireThat(new Set(names).size === names.length, 'duplicate inventory name');
}
function equal(a, b) { return a.length === b.length && a.every(n => b.includes(n)); }
// Weights only influence placement, never admission. Baseline run 37894886145
// observed retained custody at ~311s; first sharded run put it in the 831s tail.
// Include the real nested :: namespace as well as top-level test modules.
export function estimatedWeight(name, measuredWeights = {}) {
  if (Object.hasOwn(measuredWeights, name)) return measuredWeights[name];
  if (name.endsWith('::HistoryPreinstallCustody_RetainedContextReturn_002')) return 300;
  if (name.endsWith('::HistoryContextWindows_LaterCompletionPublication_040')) return 210;
  if (name.endsWith('::HistoryContextWindows_ReturnConflicts_038')) return 240;
  if (/version_history_context_windows|version_history_bundle_restore_windows|version_history_scope_context_windows|version_history::windows::context::(?:bundle_restore|switching::preinstall_custody_tests)/.test(name)) return 120;
  if (/version_history.*(?:windows|registration|restart|journal|preinstall)/.test(name)) return 30;
  if (/Webview|WebView|Channel_Native|Launch_Native/.test(name)) return 60;
  return 1;
}
export function partitionNames(names, count, measuredWeights = {}) {
  unique(names);
  requireThat(Number.isInteger(count) && count >= 1 && count <= 16, 'invalid shard count');
  requireThat(measuredWeights && typeof measuredWeights === 'object' && !Array.isArray(measuredWeights), 'invalid measured weight table');
  const entries = Object.entries(measuredWeights);
  requireThat(entries.length <= 16 && entries.every(([name, seconds]) => /^[A-Za-z0-9_]+(?:::[A-Za-z0-9_]+)+$/.test(name) && name.length <= 500 && Number.isFinite(seconds) && seconds > 0 && seconds <= 480), 'invalid bounded measured weight');
  const weight = name => estimatedWeight(name, measuredWeights);
  const shards = Array.from({ length: count }, () => []), loads = Array(count).fill(0);
  const sorted = [...names].sort((a, b) => weight(b) - weight(a) || (a < b ? -1 : a > b ? 1 : 0));
  for (const name of sorted) {
    const index = loads.indexOf(Math.min(...loads));
    shards[index].push(name); loads[index] += weight(name);
  }
  shards.forEach(s => s.sort());
  validatePartition(names, shards);
  return shards;
}
export function validatePartition(names, shards) {
  unique(names);
  requireThat(Array.isArray(shards) && shards.length > 0, 'missing shards');
  shards.forEach(unique); unique(shards.flat());
  requireThat(equal(names, shards.flat()), 'partition differs from exact inventory');
}
function checkSlice(h, s, parseListing, parseResult) {
  unique(s.names);
  requireThat(Number.isInteger(s.index) && s.index >= 0, 'invalid shard index');
  requireThat(Number.isFinite(s.durationSeconds) && s.durationSeconds >= 0, 'invalid duration');
  const rows = parseListing(s.listing);
  requireThat(equal(rows.map(t => t.name), s.names) && rows.every(t => h.full.some(f => f.name === t.name && f.type === t.type)), 'raw exact listing differs from assigned names');
  const actual = parseResult(s.output);
  requireThat(s.result && Object.entries(actual).every(([key, value]) => s.result[key] === value), 'raw result differs from shard result');
  requireThat(s.result.exitCode === 0 && actual.failed === 0, 'executed tests failed');
  requireThat(actual.passed + actual.failed + actual.ignored + actual.measured === s.names.length && actual.filteredOut === h.full.length - s.names.length, 'shard outer count mismatch');
  requireThat(actual.ignored === s.names.filter(n => h.ignored.includes(n)).length, 'shard original ignored count changed');
}
const START = 'CCDESK_SHARD_START ', LIST = '\nCCDESK_SHARD_LIST\n', BODY = '\nCCDESK_SHARD_BODY\n', END = '\nCCDESK_SHARD_END\n';
export function aggregateHarness(h, slices, parseListing, parseResult) {
  requireThat(Array.isArray(slices) && slices.length > 0, 'missing harness shards');
  requireThat(new Set(slices.map(s => s.index)).size === slices.length, 'duplicate shard index');
  validatePartition(h.selected, slices.map(s => s.names));
  const result = { exitCode: 0, passed: 0, failed: 0, ignored: 0, measured: 0, filteredOut: h.excluded.length };
  let execution = '';
  const ignored = new Map();
  for (const s of [...slices].sort((a, b) => a.index - b.index)) {
    checkSlice(h, s, parseListing, parseResult);
    const descriptor = { index: s.index, names: s.names, result: s.result, durationSeconds: s.durationSeconds };
    execution += START + JSON.stringify(descriptor) + LIST + s.listing.trimEnd() + BODY + s.output.trimEnd() + END;
    for (const key of ['passed', 'failed', 'ignored', 'measured']) result[key] += s.result[key];
    for (const m of s.output.matchAll(/^test (.+) \.\.\. ignored(?:, (.*))?$/gm)) {
      if (s.names.includes(m[1]) && h.ignored.includes(m[1])) ignored.set(m[1], m[2] ?? null);
    }
  }
  requireThat(h.ignored.every(name => ignored.has(name)), 'missing original ignored execution record');
  execution += `test result: ok. ${result.passed} passed; ${result.failed} failed; ${result.ignored} ignored; ${result.measured} measured; ${result.filteredOut} filtered out; finished in 0.00s\n`;
  return { ...h, result, defaultIgnored: h.ignored.map(name => ({ name, reason: ignored.get(name), classification: 'original-default-ignore' })), shards: slices.map(s => ({ index: s.index, names: s.names, result: s.result, durationSeconds: s.durationSeconds })), execution };
}
export function validateShardExecution(h, execution, parseListing, parseResult) {
  requireThat(Array.isArray(h.shards) && h.shards.length > 0, 'missing shard proof');
  const slices = [];
  let rest = execution;
  while (rest.startsWith(START)) {
    const listAt = rest.indexOf(LIST), bodyAt = rest.indexOf(BODY), endAt = rest.indexOf(END);
    requireThat(listAt > START.length && bodyAt > listAt && endAt > bodyAt, 'incomplete shard proof');
    const descriptor = JSON.parse(rest.slice(START.length, listAt));
    slices.push({ ...descriptor, listing: rest.slice(listAt + LIST.length, bodyAt), output: rest.slice(bodyAt + BODY.length, endAt) });
    rest = rest.slice(endAt + END.length);
  }
  const merged = aggregateHarness(h, slices, parseListing, parseResult);
  requireThat(JSON.stringify(merged.shards) === JSON.stringify(h.shards), 'shard descriptors differ from report');
  requireThat(Object.entries(merged.result).every(([key, value]) => h.result[key] === value), 'aggregate count differs from shards');
  requireThat(JSON.stringify(merged.defaultIgnored) === JSON.stringify(h.defaultIgnored), 'aggregate ignored reasons differ from shards');
  requireThat(rest === merged.execution.slice(merged.execution.lastIndexOf('test result:')), 'unexplained aggregate execution data');
}

export function validateRustShardRun(report, context) {
  const run = report.rustShardRun;
  const artifact = `windows-rust-bundle-${context.sourceSha}-${context.runId}-${context.runAttempt}`;
  requireThat(run?.policy === 'same-source-compiled-rust-shards-v1' && run.shardCount === RUST_SHARD_COUNT && /^[a-f0-9]{64}$/.test(run.planHash ?? ''), 'missing complete shard run/compiler proof');
  const bound = value => value?.sourceSha === context.sourceSha && value.runId === String(context.runId) && value.runAttempt === context.runAttempt && value.planHash === run.planHash && value.artifactName === artifact;
  requireThat(bound(run), 'shard run source/run/attempt/artifact binding mismatch');
  requireThat(typeof run.compiler?.rustcVerbose === 'string' && /^rustc 1\.98\.1(?: |\n)/.test(run.compiler.rustcVerbose), 'unexpected pinned compiler');
  requireThat(Array.isArray(run.bundleFiles) && run.bundleFiles.length > 0 && new Set(run.bundleFiles.map(f => f.path)).size === run.bundleFiles.length && run.bundleFiles.every(f => typeof f.path === 'string' && !f.path.includes('..') && !f.path.includes('\\') && Number.isSafeInteger(f.bytes) && f.bytes > 0 && /^[a-f0-9]{64}$/.test(f.sha256)), 'invalid compiler bundle commitment');
  requireThat(Array.isArray(run.shards) && run.shards.length === RUST_SHARD_COUNT && new Set(run.shards.map(s => s.index)).size === RUST_SHARD_COUNT && run.shards.every(s => Number.isInteger(s.index) && s.index >= 0 && s.index < RUST_SHARD_COUNT), 'missing or duplicate shard receipt index');
  const identities = report.harnesses.map(h => `${h.identity.kind}:${h.identity.name}`);
  for (const s of run.shards) {
    requireThat(bound(s), 'shard receipt source/run/attempt/compiler binding mismatch');
    requireThat(s.completed === true && s.exitCode === 0 && Number.isFinite(s.durationSeconds) && s.durationSeconds >= 0, 'failed or incomplete shard receipt');
    requireThat(s.host?.jobQuerySucceeded === true && s.host.inJob === report.host.inJob, 'shard host observation mismatch');
    if (report.host.elevationQuerySucceeded !== undefined || report.host.elevated !== undefined) requireThat(s.host.elevationQuerySucceeded === true && s.host.elevated === report.host.elevated, 'shard elevation observation mismatch');
    requireThat(Array.isArray(s.harnesses) && s.harnesses.length === identities.length && new Set(s.harnesses.map(h => `${h.identity?.kind}:${h.identity?.name}`)).size === identities.length && equal(s.harnesses.map(h => `${h.identity?.kind}:${h.identity?.name}`), identities), 'shard receipt harness identities mismatch');
    for (const h of report.harnesses) {
      const receipt = s.harnesses.find(r => r.identity.kind === h.identity.kind && r.identity.name === h.identity.name);
      unique(receipt.names);
      const actual = h.shards?.find(r => r.index === s.index);
      const execute = receipt.names.length > 0 || (h.full.length === 0 && s.index === 0);
      requireThat(receipt.executed === execute && (execute ? actual && equal(actual.names, receipt.names) : actual === undefined), 'shard receipt execution/names differ from raw evidence');
    }
  }
  for (const h of report.harnesses) requireThat(h.shards?.every(s => Number.isInteger(s.index) && s.index >= 0 && s.index < RUST_SHARD_COUNT), 'unexpected raw shard index');
}
