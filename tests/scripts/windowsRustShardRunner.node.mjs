import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const moduleUrl = new URL('../../scripts/windows-rust-shard-runner.mjs', import.meta.url);
const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

// 使用真实文件、git checkout 和可执行进程检查十六个分片及旧版覆盖归档。
test('RustRunner_ArtifactBinding_001', { skip: process.platform === 'win32' && 'Unix executable fixture; production binaries are Windows PE files' }, async t => {
  assert.equal(fs.existsSync(moduleUrl), true, 'the compiled-artifact runner must exist');
  const { createPlan, runShard, aggregateResults, verifyBundle, bundleArtifactName } = await import(moduleUrl);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rust-shard-runner-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const files = ['src-tauri/Cargo.lock', '.github/workflows/ci.yml', 'scripts/windows-rust-tests.ps1', 'scripts/windows-rust-shard-runner.mjs', 'scripts/windows-rust-shards.mjs', 'scripts/windows-native-validation.mjs', 'scripts/windows-native-scope.json', 'scripts/prepare-conpty.mjs', 'src-tauri/conpty/manifest.json'];
  for (const name of files) {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.copyFileSync(path.join(repository, name), path.join(root, name));
  }
  fs.writeFileSync(path.join(root, 'src-tauri/Cargo.toml'), '[package]\nname="fixture"\nversion="1.0.0"\n');
  execFileSync('git', ['init', '--quiet', root]);
  execFileSync('git', ['-C', root, 'add', '.']);
  execFileSync('git', ['-C', root, '-c', 'user.name=Runner Test', '-c', 'user.email=runner@example.invalid', 'commit', '--quiet', '-m', 'fixture']);
  const sourceSha = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const environment = { ...process.env, GITHUB_SHA: sourceSha, GITHUB_RUN_ID: '1234', GITHUB_RUN_ATTEMPT: '2', CARGO_PROFILE_DEV_DEBUG: '0', CARGO_PROFILE_TEST_DEBUG: '0' };
  fs.mkdirSync(path.join(root, 'tools'));
  fs.writeFileSync(path.join(root, 'tools/rustc'), '#!/usr/bin/env node\nconsole.log("rustc 1.98.1\\nrelease: 1.98.1\\nhost: x86_64-pc-windows-msvc");\n', { mode: 0o755 });
  environment.PATH = path.join(root, 'tools') + path.delimiter + process.env.PATH;
  const scope = JSON.parse(fs.readFileSync(path.join(root, 'scripts/windows-native-scope.json'), 'utf8'));
  const runtimeNames = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/conpty/manifest.json'), 'utf8')).files.map(f => f.name);
  const worker = 'tests::fixture::ignored_worker';
  const inventories = [scope.jobFreeTests.concat(scope.requiredSelectedTests, worker, 'ordinary::new_test'), [], ['integration::one'], ['integration::two']];
  const records = scope.harnesses.map((identity, index) => {
    const executable = path.join(root, 'src-tauri/target/debug/deps', `harness-${index}.exe`);
    fs.mkdirSync(path.dirname(executable), { recursive: true });
    const program = `#!/usr/bin/env node\nif(process.cwd()!==${JSON.stringify(path.join(root, 'src-tauri'))}) { console.error('wrong original Cargo working directory'); process.exit(99); }\nconst fs=require('node:fs'), path=require('node:path'); for(const dir of [__dirname,path.dirname(__dirname)]) for(const name of ${JSON.stringify(runtimeNames)}) if(!fs.existsSync(path.join(dir,name))) { console.error('missing runtime beside test executable: '+dir+'/'+name); process.exit(98); }\nconst full=${JSON.stringify(inventories[index])}, ignored=${JSON.stringify(index === 0 ? [worker] : [])}, args=process.argv.slice(2);\nlet selected=full; if(args.includes('--exact')) selected=full.filter(n=>args.includes(n)); else for(let i=0;i<args.length;i++) if(args[i]==='--skip') { const skip=args[++i]; selected=selected.filter(n=>n!==skip); }\nif(args.includes('--ignored')) selected=selected.filter(n=>ignored.includes(n));\nif(args.includes('--list')) { for(const n of selected) console.log(n+': test'); console.log(selected.length+' tests, 0 benchmarks'); } else { for(const n of selected) console.log('test '+n+' ... '+(ignored.includes(n)?'ignored, supervised only':'ok')); const count=selected.filter(n=>ignored.includes(n)).length; console.log('test result: ok. '+(selected.length-count)+' passed; 0 failed; '+count+' ignored; 0 measured; '+(full.length-selected.length)+' filtered out; finished in 0.01s'); }\n`;
    fs.writeFileSync(executable, program, { mode: 0o755 });
    return { reason: 'compiler-artifact', profile: { test: true }, manifest_path: path.join(root, 'src-tauri/Cargo.toml'), target: { name: identity.name, kind: [identity.kind] }, executable };
  });
  for (const directory of ['debug', 'debug/deps']) for (const name of runtimeNames) fs.writeFileSync(path.join(root, 'src-tauri/target', directory, name), 'compiled runtime bytes');
  fs.writeFileSync(path.join(root, 'src-tauri/target/debug/deps', 'unrelated.pdb'), 'excluded debug symbols');
  fs.writeFileSync(path.join(root, 'src-tauri/target/ci-test-artifacts.jsonl'), records.concat({ reason: 'build-finished', success: true }).map(r => JSON.stringify(r)).join('\n'));
  const bundle = path.join(root, 'src-tauri/target/ci-rust-bundle');
  fs.mkdirSync(path.join(bundle, 'logs'), { recursive: true });
  fs.writeFileSync(path.join(bundle, 'logs/doctests.log'), 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n');
  const options = { root, bundle, environment, inJob: true };
  const plan = createPlan(options);
  assert.equal(plan.shardCount, 16, 'the fixed workflow must assign sixteen shards');
  assert.equal(plan.artifactName, bundleArtifactName(sourceSha, '1234', 2));
  assert.equal(plan.harnesses[0].excluded.length, 18, 'only the reviewed Job-free inventory is excluded');
  assert.ok(plan.files.every(f => !f.path.endsWith('.pdb')), 'debug symbols must not enter the executable bundle');
  assert.ok(plan.files.some(f => f.path === 'src-tauri/target/debug/deps/OpenConsole.exe'), 'the ConPTY host must be bundled beside the libtest executable');
  assert.ok(runtimeNames.every(n => plan.files.some(f => f.path === `src-tauri/target/debug/deps/${n}`)), 'all embedded runtime manifest entries must be bundled beside the executable');
  assert.throws(() => verifyBundle({ ...options, environment: { ...environment, GITHUB_RUN_ATTEMPT: '3' } }), /binding/, 'another CI attempt must reject the compiler artifact');
  assert.throws(() => verifyBundle({ ...options, environment: { ...environment, GITHUB_SHA: 'a'.repeat(40) } }), /binding/, 'another source SHA must reject the compiler artifact');
  assert.throws(() => verifyBundle({ ...options, inJob: false }), /Job/, 'the observed runner Job state must agree with compilation');
  const executable = plan.files.find(f => f.path.endsWith('harness-0.exe'));
  const filename = path.join(bundle, executable.path), original = fs.readFileSync(filename);
  fs.appendFileSync(filename, '\nchanged bytes');
  assert.throws(() => verifyBundle(options), /hash|size/, 'tampered executable bytes must reject before any listing');
  fs.writeFileSync(filename, original, { mode: 0o755 });
  fs.rmSync(path.join(root, 'src-tauri/target/debug'), { recursive: true });
  const shards = path.join(root, 'shards');
  for (let index = 0; index < 16; index++) {
    const result = runShard({ ...options, index, output: path.join(shards, String(index)), artifactName: plan.artifactName });
    assert.equal(result.completed, true);
    assert.equal(result.harnesses[1].executed, index === 0, 'an originally empty harness executes once on shard zero');
    if (index > 0) assert.equal(result.harnesses[2].executed, false, 'empty assignment must not execute a nonempty harness');
  }
  const coverage = path.join(root, 'coverage');
  const report = aggregateResults({ ...options, shards, coverage });
  assert.equal(report.completed, true);
  assert.equal(report.harnesses[0].result.ignored, 1, 'original ignored worker is retained once');
  assert.equal(report.harnesses[0].defaultIgnored[0].reason, 'supervised only');
  assert.equal(report.nativeAll.status, 'unverified');
  assert.equal(report.nativeAcceptanceProven, false);
  assert.equal(report.rustShardRun.shards.length, 16, 'archived run proof retains every empty shard receipt');
  assert.equal(fs.readdirSync(path.join(coverage, 'logs')).length, 17, 'legacy release archive keeps sixteen harness logs and the doctest log');
  const firstPath = path.join(shards, '0/shard-result.json'), first = JSON.parse(fs.readFileSync(firstPath, 'utf8'));
  fs.copyFileSync(firstPath, path.join(shards, 'duplicate-result.json'));
  fs.mkdirSync(path.join(shards, 'duplicate'));
  fs.renameSync(path.join(shards, 'duplicate-result.json'), path.join(shards, 'duplicate/shard-result.json'));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /duplicate/, 'duplicate top-level shard indices reject even empty assignments');
  fs.rmSync(path.join(shards, 'duplicate'), { recursive: true });
  fs.renameSync(path.join(shards, '7/shard-result.json'), path.join(shards, '7/saved.json'));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /missing|indices/, 'all sixteen shard receipts are mandatory');
  fs.renameSync(path.join(shards, '7/saved.json'), path.join(shards, '7/shard-result.json'));
  fs.writeFileSync(firstPath, JSON.stringify({ ...first, runAttempt: 3 }));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /binding/, 'a shard from another attempt cannot complete this run');
  fs.writeFileSync(firstPath, JSON.stringify({ ...first, completed: false }));
  assert.throws(() => aggregateResults({ ...options, shards, coverage }), /failed|incomplete/, 'failed shards never produce successful native coverage');
});
