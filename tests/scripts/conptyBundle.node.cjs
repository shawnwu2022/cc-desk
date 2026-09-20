const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { createHash } = require('node:crypto');
const { test } = require('node:test');
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
