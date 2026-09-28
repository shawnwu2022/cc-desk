import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { basename, relative, resolve, sep } from 'node:path'
import { pathToFileURL } from 'node:url'

const SHA256 = /^[0-9a-f]{64}$/
const SOURCE_SHA = /^[0-9a-f]{40,64}$/
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function normalizedRelative(root, file) {
  const value = relative(root, file).split(sep).join('/')
  if (!value || value.startsWith('../') || value.includes('/../')) fail('CANDIDATE_PATH_INVALID')
  return value
}

function walk(root) {
  return readdirSync(root, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap(entry => {
      const full = resolve(root, entry.name)
      if (entry.isDirectory()) return walk(full)
      if (!entry.isFile()) return []
      return [full]
    })
}

function classify(path) {
  const name = basename(path)
  if (/-setup\.exe(?:\.sig)?$/i.test(name)) return 'windows-x86_64'
  if (/\.AppImage(?:\.sig)?$/i.test(name)) return 'linux-x86_64'
  if (/\.app\.tar\.gz(?:\.sig)?$/i.test(name) || /\.dmg$/i.test(name)) return 'darwin-aarch64'
  return 'support'
}

function validateCandidateShape(files) {
  if (!Array.isArray(files) || files.length === 0) fail('CANDIDATE_EMPTY')
  if (files.some(file => file.platform === 'support')) fail('CANDIDATE_UNDECLARED_FILE')

  const byPath = new Map(files.map(file => [file.path, file]))
  const match = pattern => files.filter(file => pattern.test(file.path))

  const windows = match(/-setup\.exe$/i)
  const linux = match(/\.AppImage$/i)
  const macUpdater = match(/\.app\.tar\.gz$/i)
  const macDmg = match(/\.dmg$/i)
  if (windows.length !== 1 || linux.length !== 1 || macUpdater.length !== 1 || macDmg.length !== 1) {
    fail('CANDIDATE_PLATFORM_SHAPE_INVALID')
  }

  for (const file of [...windows, ...linux, ...macUpdater]) {
    const signature = byPath.get(`${file.path}.sig`)
    if (!signature || signature.platform !== file.platform) fail('CANDIDATE_SIGNATURE_MISSING')
  }

  const expectedSignatures = new Set([...windows, ...linux, ...macUpdater].map(file => `${file.path}.sig`))
  const signatures = files.filter(file => file.path.endsWith('.sig'))
  if (signatures.length !== expectedSignatures.size
    || signatures.some(file => !expectedSignatures.has(file.path))) {
    fail('CANDIDATE_SIGNATURE_SET_INVALID')
  }
}

function canonicalIdentity(input) {
  return JSON.stringify({
    schemaVersion: 1,
    sourceSha: input.sourceSha,
    version: input.version,
    files: input.files.map(file => ({
      path: file.path,
      sha256: file.sha256,
      size: file.size,
      platform: file.platform,
    })),
  })
}

export function buildCandidateManifest({ root, sourceSha, version }) {
  if (typeof root !== 'string' || !root) fail('CANDIDATE_ROOT_REQUIRED')
  if (typeof sourceSha !== 'string' || !SOURCE_SHA.test(sourceSha)) fail('CANDIDATE_SOURCE_SHA_INVALID')
  if (typeof version !== 'string' || !SEMVER.test(version)) fail('CANDIDATE_VERSION_INVALID')

  const resolvedRoot = resolve(root)
  const files = walk(resolvedRoot).map(file => {
    const bytes = readFileSync(file)
    return {
      path: normalizedRelative(resolvedRoot, file),
      sha256: sha256(bytes),
      size: bytes.byteLength,
      platform: classify(file),
    }
  })

  const paths = new Set()
  for (const file of files) {
    if (paths.has(file.path)) fail('CANDIDATE_DUPLICATE_PATH')
    paths.add(file.path)
  }
  validateCandidateShape(files)

  const identity = { schemaVersion: 1, sourceSha, version, files }
  const candidateId = sha256(Buffer.from(canonicalIdentity(identity)))
  return { ...identity, candidateId }
}

export function verifyCandidateFiles(manifest, root) {
  if (!manifest || manifest.schemaVersion !== 1 || !SHA256.test(String(manifest.candidateId ?? ''))) {
    fail('CANDIDATE_MANIFEST_INVALID')
  }
  if (!SOURCE_SHA.test(String(manifest.sourceSha ?? '')) || !SEMVER.test(String(manifest.version ?? ''))) {
    fail('CANDIDATE_MANIFEST_INVALID')
  }
  if (!Array.isArray(manifest.files) || manifest.files.length === 0) fail('CANDIDATE_MANIFEST_INVALID')

  const expectedId = sha256(Buffer.from(canonicalIdentity(manifest)))
  if (expectedId !== manifest.candidateId) fail('CANDIDATE_ID_MISMATCH')

  const resolvedRoot = resolve(root)
  const actualPaths = new Set(walk(resolvedRoot).map(file => normalizedRelative(resolvedRoot, file)))
  const manifestPaths = new Set()
  for (const file of manifest.files) {
    if (!file || typeof file.path !== 'string' || !SHA256.test(String(file.sha256 ?? ''))
      || !Number.isSafeInteger(file.size) || file.size < 0
      || file.platform !== classify(file.path)) {
      fail('CANDIDATE_MANIFEST_INVALID')
    }
    if (manifestPaths.has(file.path)) fail('CANDIDATE_DUPLICATE_PATH')
    manifestPaths.add(file.path)
    const full = resolve(resolvedRoot, file.path)
    if (normalizedRelative(resolvedRoot, full) !== file.path) fail('CANDIDATE_PATH_INVALID')
    let bytes
    try {
      const info = statSync(full)
      if (!info.isFile()) fail('CANDIDATE_FILE_MISSING')
      bytes = readFileSync(full)
    } catch {
      fail('CANDIDATE_FILE_MISSING')
    }
    if (bytes.byteLength !== file.size || sha256(bytes) !== file.sha256) fail('CANDIDATE_FILE_HASH_MISMATCH')
  }
  validateCandidateShape(manifest.files)
  if (actualPaths.size !== manifestPaths.size
    || [...actualPaths].some(path => !manifestPaths.has(path))) {
    fail('CANDIDATE_UNDECLARED_FILE')
  }
  return true
}

function main() {
  const [root, output, sourceSha, version] = process.argv.slice(2)
  if (!root || !output || !sourceSha || !version) {
    fail('usage: candidate-manifest.mjs <artifact-root> <output> <source-sha> <version>')
  }
  const manifest = buildCandidateManifest({ root, sourceSha, version })
  writeFileSync(resolve(output), JSON.stringify(manifest, null, 2) + '\n')
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code ?? error?.message ?? 'CANDIDATE_MANIFEST_FAILED') + '\n')
    process.exit(1)
  }
}
