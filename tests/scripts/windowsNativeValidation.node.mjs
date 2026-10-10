import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  VALIDATION_POLICY, REPORT_FILENAME, JOB_FREE_TESTS, coverageArtifactName,
  validateNativeCoverage, readNativeCoverageArtifact,
} from '../../scripts/windows-native-validation.mjs';

const scope = JSON.parse(fs.readFileSync(new URL('../../scripts/windows-native-scope.json', import.meta.url)));
const context = { sourceSha: 'a'.repeat(40), runId: '12345', runAttempt: 2 };
const zero = { exitCode: 0, passed: 0, failed: 0, ignored: 0, measured: 0, filteredOut: 0 };
const ordinaryUnelevated = [
  'version_history::windows::process::cancel_tests::OrdinaryInstaller_DurableHandoffAndExactOnceResume_020',
  'version_history::windows::process::cancel_tests::OrdinaryInstaller_LiveWorkerSurvivesOwnerDrop_022',
  'version_history::windows::process::cancel_tests::OrdinaryInstaller_MissingLifetimeSpendsResumeWithoutRearming_024',
  'version_history::windows::process::cancel_tests::OrdinaryInstaller_PriorAppliedObservationBindsExactLaunch_025',
];

// 仅真实提升令牌主机将四条普通正向集成列为未验证，不能算作执行或忽略。
test('NativeElevation_ExactFourUnverified_015', () => {
  const report = validCoverageFixture(), lib = report.harnesses[0];
  report.host.elevationQuerySucceeded = true; report.host.elevated = true;
  lib.full.push(...ordinaryUnelevated.map(name => ({ name, type: 'test' })));
  lib.full.push(...scope.ordinaryRequiredSelectedTests.map(name => ({ name, type: 'test' })));
  lib.selected.push(...scope.ordinaryRequiredSelectedTests); lib.result.passed += 3;
  lib.excluded.push(...ordinaryUnelevated); lib.result.filteredOut += 4;
  report.nativeUnelevatedSuite = { status: 'unverified', reason: 'elevated_host', unverifiedNames: ordinaryUnelevated };
  const result = validateNativeCoverage(report, context);
  assert.equal(result.unverified, 22, 'the four elevated-host tests are independent of the original eighteen Job-free tests');
  assert.equal(result.executed, 11, 'only the three mandatory denial/cleanup tests increase execution; unavailable positives cannot');
  assert.equal(result.ignored, 3, 'host limitations cannot become ignored tests');
  assert.deepEqual(report.nativeJobSuite.unverifiedNames, JOB_FREE_TESTS);
  lib.excluded.push('ordinary_new');
  assert.throws(() => validateNativeCoverage(report, context), /exclusion/, 'an elevated host grants no arbitrary exclusion');
});

// 缺失或失败的提升检查不能替代真实主机限制证明。
test('NativeElevation_UnknownQueryBlocks_016', () => {
  const report = validCoverageFixture(), lib = report.harnesses[0];
  lib.full.push(...ordinaryUnelevated.map(name => ({ name, type: 'test' })));
  lib.full.push(...scope.ordinaryRequiredSelectedTests.map(name => ({ name, type: 'test' })));
  lib.selected.push(...scope.ordinaryRequiredSelectedTests); lib.result.passed += 3;
  lib.selected.push(...ordinaryUnelevated); lib.result.passed += 4;
  report.nativeUnelevatedSuite = { status: 'executed', reason: null, unverifiedNames: [] };
  assert.throws(() => validateNativeCoverage(report, context), /elevation/, 'current ordinary inventory requires an actual elevation query');
  report.host.elevated = false; report.host.elevationQuerySucceeded = false;
  assert.throws(() => validateNativeCoverage(report, context), /elevation/, 'a failed elevation query is never unavailable evidence');
});

// 自然非提升主机执行所有四条普通测试，Job 外主机也保持独立完整选择。
test('NativeElevation_UnelevatedKeepsAll_017', () => {
  const report = validCoverageFixture(false), lib = report.harnesses[0];
  report.host.elevationQuerySucceeded = true; report.host.elevated = false;
  lib.full.push(...ordinaryUnelevated.map(name => ({ name, type: 'test' })));
  lib.full.push(...scope.ordinaryRequiredSelectedTests.map(name => ({ name, type: 'test' })));
  lib.selected.push(...scope.ordinaryRequiredSelectedTests); lib.result.passed += 3;
  lib.selected.push(...ordinaryUnelevated); lib.result.passed += 4;
  report.nativeUnelevatedSuite = { status: 'executed', reason: null, unverifiedNames: [] };
  assert.equal(validateNativeCoverage(report, context).unverified, 0);
  lib.excluded.push(ordinaryUnelevated[0]); lib.selected = lib.selected.filter(n => n !== ordinaryUnelevated[0]);
  lib.result.passed--; lib.result.filteredOut++;
  assert.throws(() => validateNativeCoverage(report, context), /exclusion/, 'unelevated hosts cannot omit any ordinary positive integration');
});

// 部分普通库存或新提升字段不能冒充无提升字段的历史报告。
test('NativeElevation_PartialInventory_018', () => {
  const report = validCoverageFixture(), lib = report.harnesses[0];
  report.host.elevationQuerySucceeded = true; report.host.elevated = false;
  report.nativeUnelevatedSuite = { status: 'executed', reason: null, unverifiedNames: [] };
  assert.throws(() => validateNativeCoverage(report, context), /inventory drift/, 'new host fields require the complete four-test inventory');
  lib.full.push(...ordinaryUnelevated.slice(0, 3).map(name => ({ name, type: 'test' })));
  lib.selected.push(...ordinaryUnelevated.slice(0, 3)); lib.result.passed += 3;
  assert.throws(() => validateNativeCoverage(report, context), /inventory drift/, 'a partially present ordinary inventory is never legacy');
});

// 提升主机仍必须执行真实令牌拒绝、失败承诺和精确清理契约。
test('NativeElevation_RequiredDenials_019', () => {
  for (const missing of scope.ordinaryRequiredSelectedTests) {
    const report = validCoverageFixture(), lib = report.harnesses[0];
    report.host.elevationQuerySucceeded = true; report.host.elevated = true;
    lib.full.push(...ordinaryUnelevated.concat(scope.ordinaryRequiredSelectedTests).map(name => ({ name, type: 'test' })));
    lib.excluded.push(...ordinaryUnelevated); lib.result.filteredOut += 4;
    lib.selected.push(...scope.ordinaryRequiredSelectedTests.filter(n => n !== missing)); lib.result.passed += 2;
    report.nativeUnelevatedSuite = { status: 'unverified', reason: 'elevated_host', unverifiedNames: ordinaryUnelevated };
    assert.throws(() => validateNativeCoverage(report, context), /required ordinary/, `mandatory test ${missing} cannot become host-unavailable`);
  }
});
function harness(identity, names, ignored, excluded = []) {
  const selected = names.filter(name => !excluded.includes(name));
  return {
    identity: { ...identity },
    full: names.map(name => ({ name, type: 'test' })), ignored, selected, excluded,
    defaultIgnored: ignored.map(name => ({ name, reason: null, classification: 'original-default-ignore' })),
    logs: Object.fromEntries(['full', 'ignored', 'selected', 'execution'].map(phase => [phase, `logs/${identity.name}-${phase}.log`])),
    result: { ...zero, passed: selected.length - ignored.length, ignored: ignored.length, filteredOut: excluded.length },
  };
}
export function validCoverageFixture(inJob = true) {
  const names = [...scope.jobFreeTests, ...scope.requiredSelectedTests, 'ordinary_new', 'worker'];
  return {
    schema: 1, policy: VALIDATION_POLICY, completed: true, ...context,
    host: { jobQuerySucceeded: true, inJob },
    harnesses: [
      harness(scope.harnesses[0], names, ['worker'], inJob ? [...scope.jobFreeTests] : []),
      harness(scope.harnesses[1], [], []),
      harness(scope.harnesses[2], ['integration_worker'], ['integration_worker']),
      harness(scope.harnesses[3], ['real_cli_worker'], ['real_cli_worker']),
    ],
    doctests: { command: 'cargo test --locked --doc', exitCode: 0, logs: ['logs/doctests.log'], result: { ...zero } },
    nativeJobSuite: { status: inJob ? 'unverified' : 'executed', reason: inJob ? 'external_job' : null, unverifiedNames: inJob ? [...scope.jobFreeTests] : [] },
    nativeAll: { status: 'unverified', reason: 'original_all_not_run' }, nativeAcceptanceProven: false,
  };
}
function rejects(change, pattern) {
  const report = validCoverageFixture(); change(report);
  assert.throws(() => validateNativeCoverage(report, context), pattern);
}

test('WindowsNativeCoverage_Contained_001: exact eighteen disclosed; ordinary and Wry coverage retained', () => {
  const result = validateNativeCoverage(validCoverageFixture(), context);
  assert.equal(result.unverified, 18);
  assert.equal(result.harnessCount, 4);
  assert.equal(result.executed, 8);
  assert.equal(result.ignored, 3);
  assert.deepEqual(new Set(result.unverifiedNames), new Set(JOB_FREE_TESTS));
  assert.equal(result.nativeAllStatus, 'unverified');
  assert.equal(result.nativeAcceptanceProven, false);
});
test('WindowsNativeCoverage_JobFree_002: qualified host includes all names without claiming original All', () => {
  const result = validateNativeCoverage(validCoverageFixture(false), context);
  assert.equal(result.unverified, 0);
  assert.equal(result.executed, 26);
  assert.equal(result.nativeAllStatus, 'unverified');
});
test('WindowsNativeCoverage_Binding_003: source, run and attempt are exact', () => {
  rejects(r => { r.sourceSha = 'b'.repeat(40); }, /binding/);
  rejects(r => { r.runId = '999'; }, /binding/);
  rejects(r => { r.runAttempt = 3; }, /binding/);
  assert.equal(coverageArtifactName(context.sourceSha, context.runId, 2), `windows-native-coverage-${context.sourceSha}-12345-2`);
  assert.notEqual(coverageArtifactName(context.sourceSha, context.runId, 2), coverageArtifactName(context.sourceSha, context.runId, 3));
});
test('WindowsNativeCoverage_UnknownIncluded_004: newly discovered names remain selected', () => {
  const report = validCoverageFixture(); const lib = report.harnesses[0];
  lib.full.push({ name: 'new::unclassified_test', type: 'test' });
  lib.selected.push('new::unclassified_test'); lib.result.passed++;
  assert.equal(validateNativeCoverage(report, context).executed, 9);
  lib.selected.pop(); lib.result.passed--;
  assert.throws(() => validateNativeCoverage(report, context), /selection/);
});
test('WindowsNativeCoverage_ArbitrarySkip_005: caller exclusions and omitted targets refused', () => {
  rejects(r => { r.harnesses[0].excluded.push('ordinary_new'); }, /exclusion/);
  rejects(r => { r.harnesses.pop(); }, /harness/);
  rejects(r => { r.harnesses[1].identity.name = 'renamed'; }, /harness/);
});
test('WindowsNativeCoverage_PolicyDrift_006: reviewed tests missing, duplicated or ignored fail closed', () => {
  rejects(r => { r.harnesses[0].full.shift(); }, /policy|exclusion/);
  rejects(r => { r.harnesses[0].full.push(r.harnesses[0].full[0]); }, /duplicate/);
  rejects(r => { r.harnesses[0].ignored.push(JOB_FREE_TESTS[0]); }, /ignored|policy/);
  rejects(r => { r.harnesses[2].full.push({ name: JOB_FREE_TESTS[0], type: 'test' }); }, /library|policy/);
});
test('WindowsNativeCoverage_PrefixCollision_007: full-name libtest substring collisions refused', () => {
  rejects(r => {
    const lib = r.harnesses[0]; const name = JOB_FREE_TESTS[0] + '_suffix';
    lib.full.push({ name, type: 'test' }); lib.selected.push(name); lib.result.passed++;
  }, /collision/);
});
test('WindowsNativeCoverage_RequiredSelected_008: negative manager and all five real Wry supervisors stay selected', () => {
  for (const name of scope.requiredSelectedTests) {
    rejects(r => { r.harnesses[0].full = r.harnesses[0].full.filter(t => t.name !== name); r.harnesses[0].selected = r.harnesses[0].selected.filter(t => t !== name); r.harnesses[0].result.passed--; }, /required/);
  }
});
test('WindowsNativeCoverage_Counts_009: ignored, filtered and all outer counts reconcile', () => {
  rejects(r => { r.harnesses[0].result.filteredOut--; }, /count/);
  rejects(r => { r.harnesses[0].result.ignored--; r.harnesses[0].result.passed++; }, /ignored/);
  rejects(r => { r.harnesses[0].result.passed++; }, /count/);
  rejects(r => { r.harnesses[0].result.measured = -1; }, /count/);
});
test('WindowsNativeCoverage_Failure_010: selected native failures remain blocking', () => {
  rejects(r => { r.harnesses[0].result.exitCode = 101; r.harnesses[0].result.passed--; r.harnesses[0].result.failed++; }, /failed/);
  rejects(r => { r.doctests.exitCode = 1; }, /doctest/);
  rejects(r => { r.completed = false; }, /incomplete/);
});
test('WindowsNativeCoverage_QueryFailure_011: a failed query is never unavailable evidence', () => {
  rejects(r => { r.host.jobQuerySucceeded = false; }, /query/);
  rejects(r => { delete r.host.inJob; }, /host/);
  rejects(r => { r.host.inJob = false; }, /exclusion/);
});
test('WindowsNativeCoverage_HonestClaims_012: excluded tests are neither ignored nor native-All proof', () => {
  rejects(r => { r.nativeAll.status = 'passed'; }, /All/);
  rejects(r => { r.nativeAcceptanceProven = true; }, /acceptance/);
  rejects(r => { r.nativeJobSuite.unverifiedNames.push('worker'); }, /disclosure/);
  rejects(r => { r.nativeJobSuite.status = 'passed'; }, /disclosure/);
});
test('WindowsNativeCoverage_Logs_013: missing logs or traversal rejected', () => {
  rejects(r => { delete r.harnesses[0].logs.execution; }, /log/);
  rejects(r => { r.harnesses[0].logs.full = '../elsewhere.log'; }, /log/);
  rejects(r => { r.harnesses[0].logs.full = 'C:/elsewhere.log'; }, /log/);
});
test('WindowsNativeCoverage_Artifact_014: actual artifact must contain every log and fixed report', () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'ccdesk-coverage-contract-'));
  const report = validCoverageFixture(); fs.mkdirSync(`${temp}/logs`);
  fs.writeFileSync(`${temp}/${REPORT_FILENAME}`, JSON.stringify(report));
  assert.throws(() => readNativeCoverageArtifact(temp, context), /log/);
  const listing = rows => rows.map(t => `${t.name}: ${t.type}`).join('\n') + `\n${rows.filter(t => t.type === 'test').length} tests, ${rows.filter(t => t.type === 'benchmark').length} benchmarks\n`;
  const summaryLine = r => `test result: ok. ${r.passed} passed; ${r.failed} failed; ${r.ignored} ignored; ${r.measured} measured; ${r.filteredOut} filtered out; finished in 0.00s\n`;
  for (const h of report.harnesses) {
    fs.writeFileSync(`${temp}/${h.logs.full}`, listing(h.full).replaceAll('\n', '\r\n'));
    fs.writeFileSync(`${temp}/${h.logs.ignored}`, listing(h.full.filter(t => h.ignored.includes(t.name))));
    fs.writeFileSync(`${temp}/${h.logs.selected}`, listing(h.full.filter(t => h.selected.includes(t.name))));
    fs.writeFileSync(`${temp}/${h.logs.execution}`, h.ignored.map(name => `test ${name} ... ignored\n`).join('') + summaryLine(h.result));
  }
  fs.writeFileSync(`${temp}/logs/doctests.log`, 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n');
  assert.equal(readNativeCoverageArtifact(temp, context).summary.unverified, 18);
  fs.writeFileSync(`${temp}/${report.harnesses[0].logs.execution}`, summaryLine({ ...report.harnesses[0].result, passed: 1000 }));
  assert.throws(() => readNativeCoverageArtifact(temp, context), /raw outer result/);
});
