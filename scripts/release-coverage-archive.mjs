import { createHash } from 'node:crypto'
import { inflateRawSync } from 'node:zlib'
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { MAX_REPORT_BYTES, REPORT_FILENAME, readNativeCoverageArtifact, validateNativeCoverage } from './windows-native-validation.mjs'

export const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
const MAX_LOG_BYTES = 64 * 1024 * 1024, MAX_TOTAL_BYTES = 256 * 1024 * 1024
function requireThat(value, message) { if (!value) throw new Error(`Coverage archive: ${message}`) }
export function crc32(bytes) {
  let value = 0xffffffff
  for (const byte of bytes) {
    value ^= byte
    for (let bit = 0; bit < 8; bit++) value = (value >>> 1) ^ ((value & 1) ? 0xedb88320 : 0)
  }
  return (value ^ 0xffffffff) >>> 0
}
function fileName(bytes) {
  const name = bytes.toString('utf8')
  requireThat(Buffer.from(name).equals(bytes) && (name === REPORT_FILENAME || name === 'logs/'
    || /^logs\/[A-Za-z0-9_.-]+\.log$/.test(name)) && !name.includes('..'), 'unexpected or unsafe entry path')
  return name
}
// Classic single-disk ZIP only. Central/local agreement and complete ranges are
// verified before inflation; no caller archive path is ever extracted directly.
export function readCoverageZip(bytes) {
  requireThat(Buffer.isBuffer(bytes) && bytes.length >= 22 && bytes.length <= MAX_ARCHIVE_BYTES, 'archive size outside bounds')
  let end = -1
  for (let at = bytes.length - 22; at >= Math.max(0, bytes.length - 22 - 65535); at--) {
    if (bytes.readUInt32LE(at) === 0x06054b50 && at + 22 + bytes.readUInt16LE(at + 20) === bytes.length) { end = at; break }
  }
  requireThat(end >= 0 && bytes.readUInt16LE(end + 4) === 0 && bytes.readUInt16LE(end + 6) === 0, 'missing or multidisk directory')
  const count = bytes.readUInt16LE(end + 10), directorySize = bytes.readUInt32LE(end + 12), directoryOffset = bytes.readUInt32LE(end + 16)
  requireThat(count > 0 && count <= 64 && bytes.readUInt16LE(end + 8) === count
    && directoryOffset + directorySize === end, 'directory count/range or ZIP64 unsupported')
  const files = new Map(), ranges = []
  let at = directoryOffset, total = 0
  for (let index = 0; index < count; index++) {
    requireThat(at + 46 <= end && bytes.readUInt32LE(at) === 0x02014b50, 'invalid central header')
    const flags = bytes.readUInt16LE(at + 8), method = bytes.readUInt16LE(at + 10), crc = bytes.readUInt32LE(at + 16)
    const packed = bytes.readUInt32LE(at + 20), size = bytes.readUInt32LE(at + 24)
    const nameLength = bytes.readUInt16LE(at + 28), extraLength = bytes.readUInt16LE(at + 30), commentLength = bytes.readUInt16LE(at + 32)
    const offset = bytes.readUInt32LE(at + 42), next = at + 46 + nameLength + extraLength + commentLength
    requireThat(next <= end && nameLength > 0 && bytes.readUInt16LE(at + 6) <= 20 && packed !== 0xffffffff && size !== 0xffffffff && offset !== 0xffffffff
      && bytes.readUInt16LE(at + 34) === 0 && (flags & ~0x080e) === 0 && [0, 8].includes(method), 'unsupported ZIP flags, method or ZIP64')
    const nameBytes = bytes.subarray(at + 46, at + 46 + nameLength), name = fileName(nameBytes)
    const mode = (bytes.readUInt32LE(at + 38) >>> 16) & 0xf000
    requireThat(!files.has(name) && mode !== 0xa000 && [0, 0x4000, 0x8000].includes(mode), 'duplicate or nonregular entry')
    requireThat(name === 'logs/' ? size === 0 && mode !== 0x8000 : size > 0 && mode !== 0x4000, 'entry kind or empty file')
    const maximum = name === REPORT_FILENAME ? MAX_REPORT_BYTES : MAX_LOG_BYTES
    total += size
    requireThat(size <= maximum && total <= MAX_TOTAL_BYTES, 'expanded size outside bounds')
    requireThat(offset + 30 <= directoryOffset && bytes.readUInt32LE(offset) === 0x04034b50, 'invalid local header range')
    const localNameLength = bytes.readUInt16LE(offset + 26), localExtraLength = bytes.readUInt16LE(offset + 28)
    const dataStart = offset + 30 + localNameLength + localExtraLength, dataEnd = dataStart + packed
    requireThat(dataEnd <= directoryOffset && bytes.readUInt16LE(offset + 4) === bytes.readUInt16LE(at + 6)
      && bytes.readUInt16LE(offset + 6) === flags && bytes.readUInt16LE(offset + 8) === method
      && bytes.subarray(offset + 30, offset + 30 + localNameLength).equals(nameBytes), 'local/central name, flags or range mismatch')
    let rangeEnd = dataEnd
    if (flags & 8) {
      requireThat(rangeEnd + 12 <= directoryOffset, 'missing data descriptor')
      if (bytes.readUInt32LE(rangeEnd) === 0x08074b50) rangeEnd += 4
      requireThat(rangeEnd + 12 <= directoryOffset && bytes.readUInt32LE(rangeEnd) === crc
        && bytes.readUInt32LE(rangeEnd + 4) === packed && bytes.readUInt32LE(rangeEnd + 8) === size, 'descriptor differs from directory')
      rangeEnd += 12
    } else {
      requireThat(bytes.readUInt32LE(offset + 14) === crc && bytes.readUInt32LE(offset + 18) === packed
        && bytes.readUInt32LE(offset + 22) === size, 'local/central size or CRC mismatch')
    }
    const data = method === 0 ? bytes.subarray(dataStart, dataEnd) : inflateRawSync(bytes.subarray(dataStart, dataEnd), { maxOutputLength: Math.max(1, size) })
    requireThat(data.length === size && crc32(data) === crc, 'expanded size or CRC mismatch')
    files.set(name, data); ranges.push([offset, rangeEnd]); at = next
  }
  requireThat(at === end, 'central directory trailing bytes')
  ranges.sort((a, b) => a[0] - b[0])
  requireThat(ranges[0][0] === 0 && ranges.at(-1)[1] === directoryOffset
    && ranges.every((range, index) => index === 0 || range[0] === ranges[index - 1][1]), 'overlapping or unexplained local ranges')
  requireThat(files.has(REPORT_FILENAME), 'report missing')
  files.delete('logs/')
  return files
}
function safeDownloadLocation(location) {
  let url
  try { url = new URL(location) } catch { throw new Error('Coverage archive: invalid redirect') }
  requireThat(url.protocol === 'https:' && !url.username && !url.password && (!url.port || url.port === '443')
    && (/^productionresultssa\d+\.blob\.core\.windows\.net$/.test(url.hostname)
      || url.hostname.endsWith('.actions.githubusercontent.com') || url.hostname.endsWith('.githubusercontent.com')),
  'unsafe download redirect')
  return url.href
}
async function boundedBody(response) {
  const declared = response.headers.get('content-length')
  requireThat(declared === null || (/^\d+$/.test(declared) && Number(declared) <= MAX_ARCHIVE_BYTES), 'declared archive size outside bounds')
  requireThat(response.body && typeof response.body.getReader === 'function', 'archive stream missing')
  const reader = response.body.getReader(), chunks = []
  let size = 0
  try {
    for (;;) {
      const { value, done } = await reader.read()
      if (done) break
      size += value.byteLength
      requireThat(size <= MAX_ARCHIVE_BYTES, 'archive stream exceeds bound')
      chunks.push(Buffer.from(value))
    }
  } finally { await reader.cancel() }
  requireThat(size > 0 && (declared === null || Number(declared) === size), 'archive length mismatch')
  return Buffer.concat(chunks, size)
}
export async function fetchCoverageArchive({ repository, token, artifact, request = fetch }) {
  requireThat(/^[-\w.]+\/[-\w.]+$/.test(repository) && Number.isSafeInteger(artifact.id) && artifact.id > 0, 'invalid archive endpoint binding')
  let url = `https://api.github.com/repos/${repository}/actions/artifacts/${artifact.id}/zip`
  let response
  for (let hop = 0; hop <= 3; hop++) {
    try {
      response = await request(url, { redirect: 'manual', signal: AbortSignal.timeout(30000),
        headers: hop === 0 ? { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' } : {} })
    } catch { throw new Error('Coverage archive: download request failed') }
    if (![301, 302, 303, 307, 308].includes(response.status)) break
    requireThat(hop < 3, 'redirect limit exceeded')
    url = safeDownloadLocation(response.headers.get('location'))
    await response.body?.cancel()
  }
  requireThat(response.status === 200, `download failed: HTTP ${response.status}`)
  const bytes = await boundedBody(response)
  if (artifact.digest !== undefined && artifact.digest !== null) {
    requireThat(/^sha256:[a-f0-9]{64}$/.test(artifact.digest), 'unsupported archive digest')
    requireThat(`sha256:${createHash('sha256').update(bytes).digest('hex')}` === artifact.digest, 'API archive digest mismatch')
  }
  return readCoverageZip(bytes)
}
export function validateFetchedCoverage(files, context, localDirectory) {
  const report = JSON.parse(files.get(REPORT_FILENAME).toString('utf8'))
  validateNativeCoverage(report, context)
  const names = [REPORT_FILENAME, ...report.harnesses.flatMap(h => ['full', 'ignored', 'selected', 'execution'].map(phase => h.logs[phase])), ...report.doctests.logs]
  requireThat(files.size === names.length && names.every(name => files.has(name)), 'archive entries differ from expected report/logs')
  const temporaryParent = realpathSync(tmpdir()), temporary = mkdtempSync(join(temporaryParent, 'ccdesk-release-coverage-verified-'))
  try {
    requireThat(dirname(resolve(temporary)) === temporaryParent, 'temporary path escaped owner')
    mkdirSync(join(temporary, 'logs'))
    for (const name of names) writeFileSync(join(temporary, name), files.get(name), { flag: 'wx' })
    const validated = readNativeCoverageArtifact(temporary, context)
    const localRoot = realpathSync(localDirectory)
    for (const name of names) {
      const filename = join(localRoot, name), stat = lstatSync(filename), maximum = name === REPORT_FILENAME ? MAX_REPORT_BYTES : MAX_LOG_BYTES
      requireThat(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum && stat.size === files.get(name).length
        && (name === REPORT_FILENAME || dirname(realpathSync(filename)) === join(localRoot, 'logs')), 'local publication copy path/size mismatch')
      requireThat(readFileSync(filename).equals(files.get(name)), 'local publication copy differs from authenticated archive')
    }
    return { ...validated, reportBytes: files.get(REPORT_FILENAME) }
  } finally {
    requireThat(dirname(resolve(temporary)) === temporaryParent, 'temporary cleanup escaped owner')
    rmSync(temporary, { recursive: true, force: true })
  }
}
