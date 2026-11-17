// Reviewed local content lock/replay check, not a publisher-signature assertion.
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const manifest = JSON.parse(await fs.readFile(path.join(root, 'vendor/portable-pty-upstream.json'), 'utf8'));
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
async function inventory(dir, prefix = '') {
  const result = [];
  for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
    const name = prefix + entry.name;
    if (entry.isSymbolicLink()) throw new Error('Symlink rejected');
    if (entry.isDirectory()) result.push(...await inventory(path.join(dir, entry.name), name + '/'));
    else if (entry.isFile()) result.push({ path: name, sha256: digest(await fs.readFile(path.join(dir, entry.name))) });
    else throw new Error('Unexpected filesystem entry');
  }
  return result.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
}
function same(actual, expected) {
  const sorted = [...expected].sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  if (JSON.stringify(actual) !== JSON.stringify(sorted)) throw new Error('Exact file inventory/hash mismatch');
}
const patch = path.join(root, 'patches/portable-pty-owned-job.patch');
same(await inventory(path.join(root, 'vendor/portable-pty')), manifest.patchedFiles);
if (digest(await fs.readFile(patch)) !== manifest.patchSha256) throw new Error('Patch hash mismatch');
if (process.argv.length === 4 && process.argv[2] === '--archive') {
  const archive = path.resolve(process.argv[3]);
  if (digest(await fs.readFile(archive)) !== manifest.crateSha256) throw new Error('Registry archive checksum mismatch');
  const tempBase = path.resolve(os.tmpdir());
  const temp = await fs.mkdtemp(path.join(tempBase, 'cc-portable-pty-replay-'));
  try {
    function run(program, args, cwd) {
      const result = spawnSync(program, args, { cwd, encoding: 'utf8', timeout: 10000 });
      if (result.error || result.status !== 0) throw new Error(`${program} replay failed: ${result.error || result.stderr}`);
    }
    run('tar', ['-xf', archive, '-C', temp], temp);
    const upstream = path.join(temp, 'portable-pty-0.8.1');
    same(await inventory(upstream), manifest.files);
    run('git', ['-c', 'core.autocrlf=false', 'apply', '--check', patch], upstream);
    run('git', ['-c', 'core.autocrlf=false', 'apply', patch], upstream);
    same(await inventory(upstream), manifest.patchedFiles);
    console.log('PASS registry checksum + exact upstream inventory + patch replay + patched inventory');
  } finally {
    const resolved = path.resolve(temp);
    if (path.dirname(resolved) !== tempBase || !path.basename(resolved).startsWith('cc-portable-pty-replay-')) throw new Error('Cleanup boundary rejected');
    await fs.rm(resolved, { recursive: true, force: true });
  }
} else if (process.argv.length !== 2) throw new Error('Usage: node verify-portable-pty.mjs [--archive FILE]');
else console.log('PASS exact patched inventory + patch hash (content lock only)');
