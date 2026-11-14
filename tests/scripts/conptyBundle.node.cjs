const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { createHash } = require('node:crypto');
const { test } = require('node:test');
const http = require('node:http');
const { execFile, spawnSync } = require('node:child_process');
const { promisify } = require('node:util');
const execFileAsync = promisify(execFile);
const root = path.resolve(__dirname, '../..');
const configPath = path.join(root, 'src-tauri/tauri.windows.conf.json');
const config = fs.existsSync(configPath) ? JSON.parse(fs.readFileSync(configPath, 'utf8')) : {};

test('ConptyBundle_WindowsResources_001', () => {
  for (const name of ['conpty.dll', 'OpenConsole.exe', 'LICENSE-Microsoft-ConPTY.txt']) {
    assert.equal(config.bundle?.resources?.[`conpty/runtime/${name}`], name,
      `Windows installer must carry ${name} next to the application`);
  }
});
test('ConptyBundle_PrepareBothBuildAndDev_002', () => {
  for (const key of ['beforeBuildCommand', 'beforeDevCommand']) {
    assert.match(config.build?.[key] || '', /node scripts\/prepare-conpty\.mjs/);
  }
});
test('ConptyBundle_FailClosedBeforeApplication_003', () => {
  const p = path.join(root, 'src-tauri/src/main.rs');
  const main = fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : '';
  assert.ok(main.includes('conpty_runtime::initialize()'), 'Production entry must verify the bundled runtime');
  assert.ok(main.indexOf('conpty_runtime::initialize()') < main.indexOf('cc_desk::run('));
});
async function fixture(t) {
  const mod = await import(pathToFileURL(path.join(root, 'scripts/prepare-conpty.mjs')));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-conpty-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const pe = Buffer.alloc(128); pe.write('MZ'); pe.writeUInt32LE(64, 60);
  pe.write('PE\0\0', 64); pe.writeUInt16LE(0x8664, 68);
  const files = ['conpty.dll', 'OpenConsole.exe'].map(name => ({
    name, bytes: pe.length, sha256: createHash('sha256').update(pe).digest('hex'),
  }));
  for (const f of files) fs.writeFileSync(path.join(dir, f.name), pe);
  return { mod, dir, files, pe };
}
test('ConptyBundle_RejectPartialPair_004', async t => {
  const { mod, dir, files } = await fixture(t);
  await mod.verifyFiles(dir, files);
  fs.unlinkSync(path.join(dir, 'OpenConsole.exe'));
  await assert.rejects(mod.verifyFiles(dir, files), /OpenConsole/);
});
test('ConptyBundle_RejectChangedBytes_005', async t => {
  const { mod, dir, files, pe } = await fixture(t);
  pe[127] = 1; fs.writeFileSync(path.join(dir, 'conpty.dll'), pe);
  await assert.rejects(mod.verifyFiles(dir, files), /SHA-256/);
});
test('ConptyBundle_RejectWrongArchitecture_006', async t => {
  const { mod, dir, files, pe } = await fixture(t);
  pe.writeUInt16LE(0x14c, 68); fs.writeFileSync(path.join(dir, 'conpty.dll'), pe);
  files[0].sha256 = createHash('sha256').update(pe).digest('hex');
  await assert.rejects(mod.verifyFiles(dir, files), /x64/);
});
test('ConptyBundle_RejectEscapingManifest_007', async t => {
  const { mod, dir, files } = await fixture(t);
  files[0].name = '../outside.dll';
  await assert.rejects(mod.verifyFiles(dir, files), /name/);
});

async function prepareFixture(t, { corruptZip = false, wrongPackageHash = false, wrongFileHash = false } = {}) {
  const { dir, files, pe } = await fixture(t);
  const checkout = path.join(dir, "checkout [literal] ' 中文");
  const source = path.join(dir, 'zip-source', 'x64');
  const zip = path.join(dir, "runtime [literal] ' 中文.zip");
  fs.mkdirSync(source, { recursive: true });
  for (const file of files) fs.writeFileSync(path.join(source, file.name), pe);
  const packed = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
    '$ErrorActionPreference="Stop"; Add-Type -AssemblyName System.IO.Compression.FileSystem; [System.IO.Compression.ZipFile]::CreateFromDirectory($env:CC_TEST_ZIP_SOURCE, $env:CC_TEST_ZIP)'],
    { encoding: 'utf8', timeout: 30_000, env: { ...process.env, CC_TEST_ZIP_SOURCE: path.dirname(source), CC_TEST_ZIP: zip } });
  assert.equal(packed.status, 0, packed.error?.message || packed.stderr);
  const archive = corruptZip ? Buffer.from('not a ZIP archive') : fs.readFileSync(zip);
  const license = Buffer.from('isolated ConPTY fixture license');
  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push(req.url);
    if (req.url === '/runtime.zip') res.end(archive);
    else if (req.url === '/license') res.end(license);
    else { res.statusCode = 404; res.end(); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => { server.close(resolve); server.closeAllConnections(); }));
  const url = 'http://127.0.0.1:' + server.address().port;
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  const manifest = {
    version: 'isolated-test', packageUrl: url + '/runtime.zip', licenseUrl: url + '/license',
    packageSha256: wrongPackageHash ? '0'.repeat(64) : hash(archive),
    files: [...files, { name: 'LICENSE-Microsoft-ConPTY.txt', bytes: license.length, sha256: hash(license) }],
  };
  if (wrongFileHash) manifest.files[0].sha256 = '0'.repeat(64);
  fs.mkdirSync(path.join(checkout, 'scripts'), { recursive: true });
  fs.mkdirSync(path.join(checkout, 'src-tauri', 'conpty'), { recursive: true });
  const script = path.join(checkout, 'scripts', 'prepare-conpty.mjs');
  fs.copyFileSync(path.join(root, 'scripts', 'prepare-conpty.mjs'), script);
  fs.writeFileSync(path.join(checkout, 'src-tauri', 'conpty', 'manifest.json'), JSON.stringify(manifest));
  // 实际解压和失败清理也经过特殊字符路径，不只覆盖启动脚本路径。
  const temp = path.join(dir, "temp [literal] ' 中文");
  fs.mkdirSync(temp);
  const env = { ...process.env, TEMP: temp, TMP: temp };
  return {
    run: async () => {
      try { return await execFileAsync(process.execPath, [script], { env, timeout: 30_000, encoding: 'utf8' }); }
      finally { assert.deepEqual(fs.readdirSync(temp), [], 'staging directories must be cleaned on success and failure'); }
    },
    destination: path.join(checkout, 'src-tauri', 'conpty', 'runtime'), manifest, requests,
  };
}

const windowsX64 = { skip: process.platform !== 'win32' || process.arch !== 'x64' };
test('ConptyBundle_FreshPrepareWithWindowsPowerShell_008', windowsX64, async t => {
  const { run, destination, manifest, requests } = await prepareFixture(t);
  const result = await run();
  assert.match(result.stdout, /Prepared pinned ConPTY/);
  const mod = await import(pathToFileURL(path.join(root, 'scripts/prepare-conpty.mjs')));
  await mod.verifyFiles(destination, manifest.files);
  assert.deepEqual(requests, ['/runtime.zip', '/license']);
});

test('ConptyBundle_RejectCorruptZipBeforeStaging_009', windowsX64, async t => {
  const { run, destination, requests } = await prepareFixture(t, { corruptZip: true });
  await assert.rejects(run(), error => error.code !== 0 && /ConPTY extraction failed/.test(error.stderr));
  assert.equal(fs.existsSync(destination), false);
  assert.deepEqual(requests, ['/runtime.zip']);
});

test('ConptyBundle_RejectPackageHashBeforeExtraction_010', windowsX64, async t => {
  const { run, destination, requests } = await prepareFixture(t, { wrongPackageHash: true });
  await assert.rejects(run(), error => error.code !== 0 && /package SHA-256 mismatch/.test(error.stderr));
  assert.equal(fs.existsSync(destination), false);
  assert.deepEqual(requests, ['/runtime.zip']);
});

test('ConptyBundle_RejectStagedFileHashBeforePublication_011', windowsX64, async t => {
  const { run, destination, requests } = await prepareFixture(t, { wrongFileHash: true });
  await assert.rejects(run(), error => error.code !== 0 && /SHA-256 mismatch: conpty.dll/.test(error.stderr));
  assert.equal(fs.existsSync(destination), false);
  assert.deepEqual(requests, ['/runtime.zip', '/license']);
});
