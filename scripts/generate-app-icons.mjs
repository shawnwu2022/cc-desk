import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = join(root, 'src-tauri/icons/source')
const check = process.argv.includes('--check')
assert(process.argv.slice(2).every(arg => arg === '--check'), 'Usage: node scripts/generate-app-icons.mjs [--check]')
const readSource = path => readFileSync(join(source, path))
const manifest = readSource('SHA256SUMS.txt').toString('utf8').trim().split(/\r?\n/)
assert.equal(manifest.length, 19, 'Approved source inventory changed')
for (const row of manifest) {
  const match = /^([0-9a-f]{64})  ([a-zA-Z0-9_./-]+)$/.exec(row)
  assert(match && !match[2].split('/').includes('..'), 'Invalid source manifest entry')
  assert.equal(createHash('sha256').update(readSource(match[2])).digest('hex'), match[1], `Source checksum: ${match[2]}`)
}

function chunks(bytes) {
  assert.equal(bytes.toString('ascii', 0, 4), 'icns', 'Invalid ICNS signature')
  assert.equal(bytes.readUInt32BE(4), bytes.length, 'Invalid ICNS length')
  const result = new Map()
  for (let offset = 8; offset < bytes.length;) {
    assert(offset + 8 <= bytes.length, 'Truncated ICNS entry')
    const kind = bytes.toString('ascii', offset, offset + 4)
    const length = bytes.readUInt32BE(offset + 4)
    assert(length >= 8 && offset + length <= bytes.length && !result.has(kind), 'Invalid ICNS entry')
    result.set(kind, bytes.subarray(offset + 8, offset + length))
    offset += length
  }
  return result
}

function buildIcns() {
  const scratch = mkdtempSync(join(tmpdir(), 'ccdesk-icons-'))
  try {
    const result = spawnSync(process.execPath, [join(root, 'node_modules/@tauri-apps/cli/tauri.js'),
      'icon', join(source, 'png/ccdesk-1024.png'), '--output', scratch],
    { cwd: root, encoding: 'utf8', timeout: 60_000, windowsHide: true })
    assert.ifError(result.error)
    assert.equal(result.status, 0, `Tauri icon generation failed: ${result.stderr}`)
    const base = chunks(readFileSync(join(scratch, 'icon.icns')))
    const legacy = new Map()
    const ico = readSource('windows/ccdesk.ico')
    // Tauri 缩放会预乘/还原边缘颜色；直接取已核验 ICO 的 RGB 和 alpha，避免任何舍入。
    for (const [size, index, rgbKind, maskKind] of [[16, 0, 'is32', 's8mk'], [32, 2, 'il32', 'l8mk']]) {
      const offset = ico.readUInt32LE(6 + index * 16 + 12)
      assert.equal(ico.readUInt32LE(offset), 40)
      assert.equal(ico.readInt32LE(offset + 4), size)
      assert.equal(ico.readInt32LE(offset + 8), size * 2)
      const rgb = Buffer.alloc(size * size * 3), alpha = Buffer.alloc(size * size)
      for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
        const pixel = offset + 40 + ((size - 1 - y) * size + x) * 4
        const target = y * size + x
        rgb.set([ico[pixel + 2], ico[pixel + 1], ico[pixel]], target * 3)
        alpha[target] = ico[pixel + 3]
      }
      legacy.set(rgbKind, rgb)
      legacy.set(maskKind, alpha)
    }
    const pngSizes = { ic11: 32, ic12: 64, ic13: 256, ic07: 128, ic14: 512, ic09: 512, ic10: 1024, ic08: 256 }
    const kinds = ['is32', 's8mk', 'il32', 'l8mk', ...Object.keys(pngSizes)]
    // Tauri 内部使用无序集合；固定输出顺序，但仍核对完整类型集合。
    assert.deepEqual([...base.keys()].sort(), [...kinds].sort(), 'Tauri ICNS layout changed; review before regenerating')
    const entries = kinds.map(kind => {
      const data = pngSizes[kind] ? readSource(`png/ccdesk-${pngSizes[kind]}.png`) : legacy.get(kind)
      assert(data, `Missing ICNS entry: ${kind}`)
      const header = Buffer.alloc(8)
      header.write(kind, 0, 'ascii')
      header.writeUInt32BE(data.length + 8, 4)
      return Buffer.concat([header, data])
    })
    const header = Buffer.alloc(8)
    header.write('icns', 0, 'ascii')
    header.writeUInt32BE(8 + entries.reduce((sum, entry) => sum + entry.length, 0), 4)
    return Buffer.concat([header, ...entries])
  } finally {
    // 仅清理由 mkdtemp 创建并持有的本次生成目录。
    assert.equal(dirname(scratch), resolve(tmpdir()), 'Unexpected temporary directory')
    assert(basename(scratch).startsWith('ccdesk-icons-'), 'Unexpected temporary directory')
    rmSync(scratch, { recursive: true, force: true })
  }
}

const outputs = new Map([
  ['src/assets/icons/app-icon.png', readSource('png/ccdesk-256.png')],
  ['src/assets/icons/app-icon.ico', readSource('windows/ccdesk.ico')],
  ['src-tauri/icons/32x32.png', readSource('png/ccdesk-32.png')],
  ['src-tauri/icons/128x128.png', readSource('png/ccdesk-128.png')],
  ['src-tauri/icons/128x128@2x.png', readSource('png/ccdesk-256.png')],
  ['src-tauri/icons/cc-desk-icon-source.png', readSource('png/ccdesk-1024.png')],
  ['src-tauri/icons/icon.ico', readSource('windows/ccdesk.ico')],
  ['src-tauri/icons/icon.icns', buildIcns()],
])
for (const size of [16, 24, 32, 48, 64]) outputs.set(`src/assets/icons/app-icon-${size}.png`, readSource(`png/ccdesk-${size}.png`))
for (const [path, data] of outputs) {
  if (check) assert(readFileSync(join(root, path)).equals(data), `Stale generated icon: ${path}`)
  else {
    mkdirSync(dirname(join(root, path)), { recursive: true })
    writeFileSync(join(root, path), data)
  }
}
console.log(`${check ? 'Verified' : 'Generated'} ${outputs.size} app icons; all ${manifest.length} approved source checksums verified.`)
