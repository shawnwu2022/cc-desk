// Observational build evidence only. These files never admit tests or releases.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const MAX_INPUT = 64 * 1024 * 1024;
const COMMANDS = {
  tests: 'cargo test --locked --no-run --message-format=json',
  clippy: 'cargo clippy --locked --all-targets --message-format=json-render-diagnostics -- -D warnings',
  loader: 'cargo build --locked --message-format=json-render-diagnostics',
};
const validName = value => typeof value === 'string' && /^[A-Za-z0-9_+./?-]{1,200}$/.test(value);
function requireThat(ok, message) { if (!ok) throw new Error(`Build metrics: ${message}`); }

export function summarizeCargoMessages(text) {
  requireThat(typeof text === 'string' && Buffer.byteLength(text) <= MAX_INPUT, 'Cargo JSON exceeds input bound');
  const counts = { fresh: 0, rebuilt: 0, buildScripts: 0 }, crates = new Map();
  let buildFinishedSuccess = null, finished = 0;
  for (const line of text.replace(/^\uFEFF/, '').split(/\r?\n/).filter(line => line.trim())) {
    const record = JSON.parse(line);
    if (record.reason === 'compiler-artifact') {
      requireThat(typeof record.fresh === 'boolean', 'compiler-artifact fresh must be boolean');
      requireThat(validName(record.target?.name) && Array.isArray(record.target.kind) && record.target.kind.every(validName), 'invalid compiler target');
      requireThat(Array.isArray(record.features) && record.features.every(validName), 'invalid compiler features');
      const features = [...record.features].sort(), kinds = [...record.target.kind].sort();
      const key = JSON.stringify([record.target.name, kinds, features]);
      const crate = crates.get(key) ?? { name: record.target.name, kinds, features, fresh: 0, rebuilt: 0 };
      const status = record.fresh ? 'fresh' : 'rebuilt'; counts[status]++; crate[status]++; crates.set(key, crate);
    } else if (record.reason === 'build-script-executed') counts.buildScripts++;
    else if (record.reason === 'build-finished') {
      requireThat(++finished === 1 && typeof record.success === 'boolean', 'invalid/duplicate build-finished record');
      buildFinishedSuccess = record.success;
    }
  }
  return { counts, buildFinishedSuccess, crates: [...crates.values()].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))) };
}

export function classifyCacheHit(value) {
  requireThat(['true', 'false', '', undefined].includes(value), 'invalid cache-hit output');
  return value === 'true' ? 'exact' : value === 'false' ? 'non-exact-or-miss' : 'unknown';
}

function command(command, args, root) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', timeout: 10000, maxBuffer: 64 * 1024, windowsHide: true });
  return result.status === 0 ? result.stdout.trim() : null;
}
function binding(root) {
  const sourceSha = process.env.GITHUB_SHA, runId = process.env.GITHUB_RUN_ID, runAttempt = Number(process.env.GITHUB_RUN_ATTEMPT);
  requireThat(/^[a-f0-9]{40}$/.test(sourceSha ?? '') && /^[1-9][0-9]*$/.test(runId ?? '') && Number.isSafeInteger(runAttempt) && runAttempt > 0, 'invalid source/run/attempt');
  requireThat(command('git', ['rev-parse', 'HEAD'], root) === sourceSha, 'checkout source mismatch');
  return { sourceSha, runId, runAttempt };
}
function write(filename, value) { fs.mkdirSync(path.dirname(filename), { recursive: true }); fs.writeFileSync(filename, JSON.stringify(value, null, 2) + '\n'); }
function collectContext(root) {
  const files = ['src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', '.cargo/config.toml', 'src-tauri/.cargo/config.toml', 'src-tauri/conpty/manifest.json'];
  const manifests = files.map(filename => ({ path: filename, sha256: fs.existsSync(path.join(root, filename)) ? createHash('sha256').update(fs.readFileSync(path.join(root, filename))).digest('hex') : null }));
  const cacheStartedMs = Number(process.env.CI_CACHE_STARTED_MS);
  const restoreSeconds = Number.isFinite(cacheStartedMs) && cacheStartedMs > 0 ? (Date.now() - cacheStartedMs) / 1000 : null;
  const rustcVerbose = command('rustc', ['-Vv'], root);
  // link.exe's help exits nonzero on some MSVC images. Only the version banner
  // is retained; never archive the full output, executable path or environment.
  const linker = process.platform === 'win32' ? spawnSync('link.exe', ['/?'], { cwd: root, encoding: 'utf8', timeout: 10000, maxBuffer: 64 * 1024, windowsHide: true }) : null;
  const linkerVersion = ((linker?.stdout ?? '') + (linker?.stderr ?? '')).match(/Microsoft .*Linker Version [0-9.]+[^\r\n]*/)?.[0] ?? null;
  return {
    schema: 1, kind: 'observational-cargo-build-metrics-v1', ...binding(root), role: process.env.GITHUB_JOB,
    rustcVerbose, cargoVersion: command('cargo', ['-V'], root), activeToolchain: command('rustup', ['show', 'active-toolchain'], root),
    installedToolchains: command('rustup', ['toolchain', 'list', '--quiet'], root)?.split(/\r?\n/) ?? null,
    target: rustcVerbose?.match(/^host: (.+)$/m)?.[1] ?? null,
    runner: { imageOS: process.env.ImageOS ?? null, imageVersion: process.env.ImageVersion ?? null, linkerVersion },
    profiles: Object.fromEntries(['CARGO_PROFILE_DEV_DEBUG', 'CARGO_PROFILE_TEST_DEBUG', 'CARGO_INCREMENTAL'].map(key => [key, process.env[key] ?? null])),
    features: 'default; no extra features', manifests,
    cache: { namespace: 'rust-compile', match: classifyCacheHit(process.env.CI_CACHE_HIT), restoreSeconds, restoreTiming: 'wall interval surrounding cache action; includes step overhead', bytes: null, saveSeconds: null, missingFields: 'cache bytes/fallback-vs-miss/save time require the original action log and final job-step timestamps' },
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = process.cwd(), directory = path.join(root, 'src-tauri/target/ci-build-metrics');
    const [action, commandId, input, seconds, code] = process.argv.slice(2);
    if (action === 'context' && process.argv.length === 3) write(path.join(directory, 'context.json'), collectContext(root));
    else {
      requireThat(action === 'cargo' && process.argv.length === 7 && Object.hasOwn(COMMANDS, commandId), 'expected context or cargo <tests|clippy|loader> <jsonl> <seconds> <exit-code>');
      const durationSeconds = Number(seconds), exitCode = Number(code);
      requireThat(Number.isFinite(durationSeconds) && durationSeconds >= 0 && Number.isSafeInteger(exitCode), 'invalid command duration/exit code');
      const filename = path.resolve(root, input);
      requireThat(fs.statSync(filename).size <= MAX_INPUT, 'Cargo JSON exceeds input bound');
      const cargo = summarizeCargoMessages(fs.readFileSync(filename, 'utf8'));
      const durationScope = commandId === 'tests' ? 'existing Windows Compile entry; includes its read-only host/token and inventory checks' : 'Cargo invocation with JSON redirection';
      const value = { schema: 1, kind: 'observational-cargo-build-metrics-v1', ...binding(root), command: COMMANDS[commandId], durationScope, durationSeconds, exitCode, ...cargo };
      write(path.join(directory, `${commandId}.json`), value);
      console.log(JSON.stringify({ commandId, durationSeconds, exitCode, counts: cargo.counts, buildFinishedSuccess: cargo.buildFinishedSuccess }));
    }
  } catch {
    // JSON parser and filesystem errors can contain raw excerpts or full paths.
    console.error('Build metrics: recording failed; raw paths and build output withheld');
    process.exitCode = 1;
  }
}
