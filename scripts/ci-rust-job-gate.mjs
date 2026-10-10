import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function requireRustJobSuccess(results) {
  for (const key of ['compile', 'static', 'shards']) {
    if (results[key] !== 'success') throw new Error(`Rust checks: ${key} job result must be success`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    requireRustJobSuccess({ compile: process.env.COMPILE_RESULT, static: process.env.STATIC_RESULT, shards: process.env.SHARD_RESULT });
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
