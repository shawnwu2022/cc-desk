const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');

const repo = path.resolve(__dirname, '../..');
const releaseGateNames = [
  'devtools-json-big-integer',
  'console-multiline-over-128k',
  'devtools-json-nested-64',
  'devtools-json-nested-256',
  'devtools-json-nested-800',
  'win10-19045-user-reported-shape',
  'field-shape-28037-no-tabs',
  'consecutive-three-pastes',
];

function generate(t, launchMode = 'direct') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-paste-release-gate-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.cpSync(path.join(repo, '.github/scripts'), path.join(root, '.github/scripts'), { recursive: true });
  fs.mkdirSync(path.join(root, 'src/utils'), { recursive: true });
  fs.copyFileSync(path.join(repo, 'src/utils/pasteText.ts'), path.join(root, 'src/utils/pasteText.ts'));
  fs.symlinkSync(path.join(repo, 'node_modules'), path.join(root, 'node_modules'), process.platform === 'win32' ? 'junction' : 'dir');
  const fixtureDir = path.join(root, 'src-tauri/tests/fixtures');
  fs.mkdirSync(fixtureDir, { recursive: true });
  fs.copyFileSync(
    path.join(repo, 'src-tauri/tests/fixtures/devtools-paste-framing.json'),
    path.join(fixtureDir, 'devtools-paste-framing.json'),
  );
  const pkg = path.join(root, '.ci-claude/node_modules/@anthropic-ai/claude-code');
  fs.mkdirSync(pkg, { recursive: true });
  fs.writeFileSync(path.join(pkg, 'package.json'), JSON.stringify({ version: '0.0.0-test', bin: { claude: 'cli.js' } }));
  fs.writeFileSync(path.join(pkg, 'cli.js'), 'throw new Error("not a real CLI");');
  const run = spawnSync(process.execPath, [path.join(root, '.github/scripts/generate-bundled-paste.cjs')], {
    cwd: root,
    encoding: 'utf8',
    timeout: 20_000,
    env: {
      ...process.env,
      CC_PASTE_CLI_KIND: 'npm',
      CC_PASTE_NATIVE_CLI: '',
      CC_PASTE_LAUNCH_MODE: launchMode,
      GITHUB_ENV: path.join(root, 'env'),
      GITHUB_STEP_SUMMARY: path.join(root, 'summary'),
    },
  });
  assert.equal(run.status, 0, run.stderr);
  const all = JSON.parse(fs.readFileSync(path.join(root, '.ci-claude/payloads.json'), 'utf8'));
  const gate = JSON.parse(fs.readFileSync(path.join(root, '.ci-claude/payloads-gate.json'), 'utf8'));
  return { all, gate };
}

test('PasteReleaseGate_ContainsOnlyExactReleaseBlockers_001', t => {
  const { all, gate } = generate(t);
  assert.deepEqual(gate.map(entry => entry.name), releaseGateNames);
  assert.ok(gate.every(entry => entry.launchMode === 'direct'));
  for (const entry of gate) {
    assert.deepEqual(entry, all.find(candidate => candidate.name === entry.name));
  }
});

test('PasteReleaseGate_ExcludesCharacterizationOnlyCases_002', t => {
  const { gate } = generate(t, 'production-shell');
  assert.ok(gate.every(entry => entry.launchMode === 'production-shell'));
  for (const name of [
    'devtools-console-unicode',
    'devtools-non-json-object',
    'console-ansi-literals',
    'devtools-text-shape-1341',
  ]) {
    assert.equal(gate.some(entry => entry.name === name), false, `${name} is characterization, not the release gate`);
  }
});

test('PasteReleaseGate_WorkflowRunsGateBeforeCharacterization_003', () => {
  const workflow = fs.readFileSync(path.join(repo, '.github/workflows/paste-cli-acceptance.yml'), 'utf8');
  const gate = workflow.indexOf('Run exact release gate');
  const characterization = workflow.indexOf('Characterize historical strict differences');
  assert.ok(gate >= 0, 'missing exact release gate step');
  assert.ok(characterization > gate, 'historical characterization must run after the blocking release gate');
  assert.match(workflow, /payloads-gate\.json/);
  assert.match(
    workflow,
    /if \(\$infraFailed\) \{ exit 1 \}\r?\n\s+exit 0/,
    'expected strict characterization failures must not leak a native cargo exit code into the job result',
  );
});
