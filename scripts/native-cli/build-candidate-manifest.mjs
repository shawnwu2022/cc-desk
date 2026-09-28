#!/usr/bin/env node

import { createHash } from 'node:crypto'
import {
  lstatSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { dirname, relative, resolve, sep } from 'node:path'
import { mkdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { candidateIdFor } from './verify-acceptance.mjs'

const COMMIT = /^[0-9a-f]{40}$/
const MAX_FILE_BYTES = 2 * 1024 * 1024 * 1024

const TARGETS = [
  { suffix: '-macos', platform: 'macos', arch: 'aarch64' },
  { suffix: '-linux', platform: 'linux', arch: 'x86_64' },
  { suffix: '-windows', platform: 'windows', arch: 'x86_64' },
]

function fail(code) {
  throw new Error(code)
}

function hashFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function classify(relativePath) {
  const first = relativePath.split('/')[0]
  const match = TARGETS.find(target => first.endsWith(target.suffix))
  if (!match) fail('CANDIDATE_PLATFORM_UNKNOWN')
  return match
}

function walk(root, directory = root) {
  const result = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name)
    const stat = lstatSync(path)
    if (stat.isSymbolicLink()) fail('CANDIDATE_SYMLINK_FORBIDDEN')
    if (stat.isDirectory()) {
      result.push(...walk(root, path))
      continue
    }
    if (!stat.isFile() || stat.size <= 0 || stat.size > MAX_FILE_BYTES) {
      fail('CANDIDATE_FILE_INVALID')
    }
    const rel = relative(root, path).split(sep).join('/')
    if (!rel || rel.startsWith('../')) fail('CANDIDATE_PATH_INVALID')
    result.push({ path, relativePath: rel })
  }
  return result
}

export function buildCandidateManifest(candidateRoot, sourceCommit) {
  if (!COMMIT.test(sourceCommit ?? '')) fail('CANDIDATE_SOURCE_COMMIT_INVALID')
  const root = realpathSync(resolve(candidateRoot))
  const files = walk(root)
  if (files.length === 0) fail('CANDIDATE_FILES_MISSING')

  const manifest = {
    schemaVersion: 1,
    sourceCommit,
    files: files
      .map(file => {
        const target = classify(file.relativePath)
        return {
          path: file.relativePath,
          platform: target.platform,
          arch: target.arch,
          sha256: hashFile(file.path),
        }
      })
      .sort((a, b) => a.path.localeCompare(b.path)),
  }
  manifest.candidateId = candidateIdFor(manifest)
  return manifest
}

function parseArgs(argv) {
  const values = {}
  const allowed = new Set(['--root', '--source-commit', '--out'])
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index]
    if (!allowed.has(option) || index + 1 >= argv.length || values[option]) {
      fail('CANDIDATE_OPTION_INVALID')
    }
    values[option] = argv[++index]
  }
  for (const option of allowed) {
    if (!values[option]) fail('CANDIDATE_OPTION_REQUIRED')
  }
  return values
}

function main() {
  try {
    const args = parseArgs(process.argv.slice(2))
    const manifest = buildCandidateManifest(args['--root'], args['--source-commit'])
    const out = resolve(args['--out'])
    mkdirSync(dirname(out), { recursive: true })
    writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`, {
      encoding: 'utf8',
      mode: 0o600,
    })
    process.stdout.write(`${JSON.stringify({
      candidateId: manifest.candidateId,
      sourceCommit: manifest.sourceCommit,
      fileCount: manifest.files.length,
    })}\n`)
  } catch (error) {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'CANDIDATE_MANIFEST_FAILED'
    process.stderr.write(`[candidate-manifest-error] ${reason}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
