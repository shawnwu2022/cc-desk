const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');

const repo = path.resolve(__dirname, '../..');
const fixtures = [
  { name: 'plain-stack', source: "Error: missing field\r\n\tat Widget.refresh (eval at <anonymous> (bundle.js:1:2))\r\n(匿名)\t@\tassets\\core\\c…nt\\Item.ts:128", repeat: 1 },
  { name: 'json-control', source: '{\r\n  "id": 18446744073709551615\r\n}', repeat: 1, isJson: true },
  { name: 'ansi-transport-only', source: 'head\x1b[31mred\x1b[0m', repeat: 1,
    cliAcceptance: false, cliSkipReason: 'Transport preservation test, not a literal CLI editor contract' },
];

function generate(t, customFixtures = fixtures, launchMode = '') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-paste-generator-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.cpSync(path.join(repo, '.github/scripts'), path.join(root, '.github/scripts'), { recursive: true });
  fs.mkdirSync(path.join(root, 'src/utils'), { recursive: true });
  fs.copyFileSync(path.join(repo, 'src/utils/pasteText.ts'), path.join(root, 'src/utils/pasteText.ts'));
  fs.symlinkSync(path.join(repo, 'node_modules'), path.join(root, 'node_modules'), process.platform === 'win32' ? 'junction' : 'dir');
  const fixtureDir = path.join(root, 'src-tauri/tests/fixtures');
  fs.mkdirSync(fixtureDir, { recursive: true });
  fs.writeFileSync(path.join(fixtureDir, 'devtools-paste-framing.json'), JSON.stringify(customFixtures));
  // Package-entry resolution only; this fake entry is never executed.
  const pkg = path.join(root, '.ci-claude/node_modules/@anthropic-ai/claude-code');
  fs.mkdirSync(pkg, { recursive: true });
  fs.writeFileSync(path.join(pkg, 'package.json'), JSON.stringify({ version: '0.0.0-test', bin: { claude: 'cli.js' } }));
  fs.writeFileSync(path.join(pkg, 'cli.js'), 'throw new Error("not a real CLI");');
  const run = spawnSync(process.execPath, [path.join(root, '.github/scripts/generate_paste_acceptance.cjs')], {
    cwd: root, encoding: 'utf8', timeout: 20000,
    env: { ...process.env, CC_PASTE_CLI_KIND: 'npm', CC_PASTE_NATIVE_CLI: '', CC_PASTE_LAUNCH_MODE: launchMode, GITHUB_ENV: path.join(root, 'env'), GITHUB_STEP_SUMMARY: path.join(root, 'summary') },
  });
  assert.equal(run.status, 0, run.stderr);
  return JSON.parse(fs.readFileSync(path.join(root, '.ci-claude/payloads.json'), 'utf8'));
}

test('PasteAcceptance_NonJsonReachesCli_001', t => {
  const cases = generate(t);
  const actual = cases.find(c => c.name === 'plain-stack');
  assert.ok(actual, 'Non-JSON error stacks must not be filtered out of actual CLI acceptance');
  const expected = fixtures[0].source.replace(/\r\n?/g, '\n');
  assert.equal(actual.expected, expected);
  assert.equal(actual.wire, '\x1b[200~' + expected + '\x1b[201~');
  assert.equal(actual.isJson, false);
});

test('PasteAcceptance_JsonValidationIsExplicit_002', t => {
  const cases = generate(t);
  assert.equal(cases.find(c => c.name === 'json-control').isJson, true);
  assert.equal(cases.some(c => c.name === 'ansi-transport-only'), false);
  assert.equal(cases.find(c => c.name === 'win10-19045-user-reported-shape').expected.length, 106002);
});

test('PasteAcceptance_ShortReportedMetrics_003', t => {
  const cases = generate(t);
  for (const [name, wireBytes, chars, lf] of [
    ['devtools-text-shape-1341', 1341, 1281, 17],
    ['devtools-text-shape-1326', 1326, 1266, 16],
  ]) {
    const actual = cases.find(c => c.name === name);
    assert.ok(actual, `Missing sanitized shape fixture: ${name}`);
    assert.equal(Buffer.byteLength(actual.wire), wireBytes);
    assert.equal([...actual.wire].length, chars);
    assert.equal(actual.expected.split('\n').length - 1, lf);
    assert.equal(actual.isJson, false);
    assert.equal(actual.wire, '\x1b[200~' + actual.expected + '\x1b[201~');
  }
});

test('PasteAcceptance_ChunkBoundaryOffsets_004', t => {
  const cases = generate(t);
  for (const boundary of [255, 256, 257, 4095, 4096, 4097]) {
    const actual = cases.find(c => c.name === `devtools-text-boundary-${boundary}`);
    assert.ok(actual, `Missing boundary fixture: ${boundary}`);
    assert.equal(Buffer.byteLength(actual.wire.split('__BOUNDARY__')[0]), boundary);
  }
});

// 启动方式可以变化，但送给 writer 的正文和帧不能随之变化。
test('PasteAcceptance_LaunchModePreservesWire_005', t => {
  const direct = generate(t, fixtures, 'direct');
  const shell = generate(t, fixtures, 'production-shell');
  assert.equal(shell.length, direct.length);
  for (let index = 0; index < direct.length; index += 1) {
    assert.equal(direct[index].launchMode, 'direct');
    assert.equal(shell[index].launchMode, 'production-shell');
    assert.deepEqual({ ...shell[index], launchMode: 'direct' }, direct[index]);
  }
});

test('PasteAcceptance_DefaultLaunchIsDirect_006', t => {
  const cases = generate(t);
  assert.ok(cases.every(c => c.launchMode === 'direct'));
});

test('PasteAcceptance_RejectUnknownLaunch_007', t => {
  assert.throws(() => generate(t, fixtures, 'not-a-launch-mode'), /Invalid CC_PASTE_LAUNCH_MODE/);
});

// 只上传合成验收的指标 JSON；隐藏的 .ci-claude 目录必须显式允许。
test('PasteAcceptance_MetadataArtifactIsNotSilentlyOmitted_008', () => {
  const workflow = fs.readFileSync(path.join(repo, '.github/workflows/paste-cli-acceptance.yml'), 'utf8');
  const step = workflow.split('- name: Upload result metadata only')[1]?.split('\n      - name:')[0];
  assert.ok(step, 'Expected the dedicated metadata-only artifact step');
  assert.match(step, /path: \.ci-claude\/acceptance-\*\.json/);
  assert.match(step, /include-hidden-files: true/, 'The metadata directory is hidden and upload-artifact excludes it by default');
});
