#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { lstatSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SHA = /^[0-9a-f]{40}$/

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

function walk(root, current = root) {
  return readdirSync(current, { withFileTypes: true }).flatMap(entry => {
    const full = path.join(current, entry.name)
    return entry.isDirectory() ? walk(root, full) : [full]
  })
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

export function candidateIdFor(commitSha, files) {
  const normalized = [...files]
    .map(file => ({
      path: file.path,
      kind: file.kind,
      sha256: String(file.sha256).toLowerCase(),
      size: file.size,
    }))
    .sort((a, b) => a.path.localeCompare(b.path))
  const digest = sha256(Buffer.from(JSON.stringify({ commitSha, files: normalized })))
  return `candidate-${commitSha.slice(0, 12)}-${digest}`
}

function normalizedRelative(root, file) {
  const relative = path.relative(root, file).split(path.sep).join('/')
  if (!relative || relative.startsWith('../') || relative.includes('/../')) fail('INVALID_CANDIDATE_PATH')
  return relative
}

function classify(name) {
  if (/-setup\.exe$/i.test(name)) return 'windows-package'
  if (/-setup\.exe\.sig$/i.test(name)) return 'windows-signature'
  if (/\.AppImage$/i.test(name)) return 'linux-package'
  if (/\.AppImage\.sig$/i.test(name)) return 'linux-signature'
  if (/\.app\.tar\.gz$/i.test(name)) return 'macos-updater'
  if (/\.app\.tar\.gz\.sig$/i.test(name)) return 'macos-signature'
  if (/\.dmg$/i.test(name)) return 'macos-installer'
  return null
}

export function buildCandidateManifest({ root, commitSha }) {
  if (!SHA.test(String(commitSha))) fail('INVALID_CANDIDATE_COMMIT')
  const files = walk(root)
    .map(file => {
      const kind = classify(path.basename(file))
      if (!kind) return null
      const metadata = lstatSync(file)
      if (!metadata.isFile() || metadata.isSymbolicLink()) fail('UNSAFE_CANDIDATE_FILE')
      const bytes = readFileSync(file)
      return {
        path: normalizedRelative(root, file),
        kind,
        sha256: sha256(bytes),
        size: statSync(file).size,
      }
    })
    .filter(Boolean)
    .sort((a, b) => a.path.localeCompare(b.path))

  const requiredKinds = [
    'windows-package',
    'windows-signature',
    'linux-package',
    'linux-signature',
    'macos-updater',
    'macos-signature',
    'macos-installer',
  ]
  for (const required of requiredKinds) {
    if (files.filter(file => file.kind === required).length !== 1) {
      fail('CANDIDATE_PLATFORM_INCOMPLETE')
    }
  }

  for (const file of files) {
    if (!requiredKinds.includes(file.kind)) fail('CANDIDATE_PLATFORM_INCOMPLETE')
  }

  return {
    schemaVersion: 1,
    candidateId: candidateIdFor(commitSha, files),
    commitSha,
    files,
  }
}

function main() {
  const root = path.resolve(process.argv[2] || 'artifacts')
  const output = path.resolve(process.argv[3] || 'candidate-manifest.json')
  const commitSha = process.argv[4] || process.env.GITHUB_SHA
  const manifest = buildCandidateManifest({ root, commitSha })
  writeFileSync(output, JSON.stringify(manifest, null, 2) + '\n')
  process.stdout.write(manifest.candidateId + '\n')
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code || 'CANDIDATE_MANIFEST_FAILED') + '\n')
    process.exit(1)
  }
}
