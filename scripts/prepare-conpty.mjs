// Build-time only. Runtime never downloads executable code. Pinned upstream bits.
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const digest = data => createHash('sha256').update(data).digest('hex');
export async function verifyFiles(dir, files) {
  for (const file of files) {
    if (!/^[A-Za-z0-9_.-]+$/.test(file.name) || file.name === '.' || file.name === '..') throw new Error('Unsafe runtime file name');
    const name = path.join(dir, file.name);
    const st = await fs.lstat(name);
    if (!st.isFile() || st.isSymbolicLink() || st.size !== file.bytes) throw new Error(`Invalid size/type: ${file.name}`);
    const data = await fs.readFile(name);
    if (digest(data) !== file.sha256.toLowerCase()) throw new Error(`SHA-256 mismatch: ${file.name}`);
    if (/\.(dll|exe)$/i.test(file.name)) {
      const off = data.length >= 64 ? data.readUInt32LE(60) : data.length;
      if (data.toString('ascii', 0, 2) !== 'MZ' || off + 6 > data.length ||
          data.toString('ascii', off, off + 4) !== 'PE\0\0' || data.readUInt16LE(off + 4) !== 0x8664) {
        throw new Error(`Expected x64 PE: ${file.name}`);
      }
    }
  }
}
async function download(url, limit) {
  const response = await fetch(url, { signal: AbortSignal.timeout(90_000) });
  if (!response.ok || !response.body) throw new Error(`Download failed: ${response.status}`);
  const chunks = []; let bytes = 0;
  for await (const chunk of response.body) {
    bytes += chunk.length;
    if (bytes > limit) throw new Error('Upstream artifact exceeds size limit');
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks);
}
async function walk(dir) {
  const result = [];
  for (const item of await fs.readdir(dir, { withFileTypes: true })) {
    const p = path.join(dir, item.name);
    if (item.isDirectory()) result.push(...await walk(p));
    else if (item.isFile()) result.push(p);
  }
  return result;
}
export async function prepare() {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('Bundled ConPTY currently supports Windows x64 only');
  const manifest = JSON.parse(await fs.readFile(path.join(root, 'src-tauri/conpty/manifest.json'), 'utf8'));
  const destination = path.join(root, 'src-tauri/conpty/runtime');
  try { await verifyFiles(destination, manifest.files); console.log(`Verified cached ConPTY ${manifest.version}`); return; } catch { /* Re-stage only from pinned upstream. */ }
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'cc-conpty-build-'));
  try {
    const archive = await download(manifest.packageUrl, 8 * 1024 * 1024);
    if (digest(archive) !== manifest.packageSha256) throw new Error('Microsoft ConPTY package SHA-256 mismatch');
    const zip = path.join(temp, 'runtime.zip'); const extracted = path.join(temp, 'extracted');
    await fs.writeFile(zip, archive);
    // Paths are passed as environment data, not interpolated PowerShell code.
    const run = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      '$ErrorActionPreference="Stop"; Expand-Archive -LiteralPath $env:CC_CONPTY_ZIP -DestinationPath $env:CC_CONPTY_EXTRACT'],
      { env: { ...process.env, CC_CONPTY_ZIP: zip, CC_CONPTY_EXTRACT: extracted }, encoding: 'utf8', timeout: 90_000 });
    if (run.error || run.status !== 0) throw new Error(`ConPTY extraction failed: ${run.error || run.stderr}`);
    const candidates = await walk(extracted); const staged = path.join(temp, 'staged'); await fs.mkdir(staged);
    for (const file of manifest.files) {
      if (/\.(dll|exe)$/i.test(file.name)) {
        const matching = candidates.filter(p => path.basename(p).toLowerCase() === file.name.toLowerCase() && /[\\/](win-)?x64[\\/]/i.test(p));
        if (matching.length !== 1) throw new Error(`Expected exactly one x64 ${file.name}`);
        await fs.copyFile(matching[0], path.join(staged, file.name));
      } else {
        await fs.writeFile(path.join(staged, file.name), await download(manifest.licenseUrl, 64 * 1024));
      }
    }
    await verifyFiles(staged, manifest.files);
    // This is a generated directory inside the checkout, never an installation.
    await fs.rm(destination, { recursive: true, force: true });
    await fs.mkdir(destination, { recursive: true });
    for (const file of manifest.files) await fs.copyFile(path.join(staged, file.name), path.join(destination, file.name));
    await verifyFiles(destination, manifest.files);
    console.log(`Prepared pinned ConPTY ${manifest.version} (x64)`);
  } finally { await fs.rm(temp, { recursive: true, force: true }); }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  try {
    if (command === '--verify' && process.argv[3]) {
      const manifest = JSON.parse(await fs.readFile(path.join(root, 'src-tauri/conpty/manifest.json'), 'utf8'));
      await verifyFiles(path.resolve(process.argv[3]), manifest.files);
      console.log('Runtime files match pinned manifest');
    } else if (command === undefined) await prepare();
    else throw new Error('Usage: node scripts/prepare-conpty.mjs [--verify DIRECTORY]');
  } catch (error) { console.error(String(error)); process.exitCode = 1; }
}
