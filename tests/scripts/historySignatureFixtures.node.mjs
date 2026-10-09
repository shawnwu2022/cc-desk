import { readFileSync } from 'node:fs'
import { createHash, createPublicKey, verify } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'
import { test } from 'node:test'
import assert from 'node:assert/strict'

const directory = fileURLToPath(new URL('../fixtures/version-history-minisign/', import.meta.url))
const read = name => readFileSync(directory + name)

test('hashed public signature fixtures disable checkout text conversion', () => {
  const names = [...Object.keys(JSON.parse(read('provenance.json')).sha256), 'provenance.json']
  const paths = names.map(name => `tests/fixtures/version-history-minisign/${name}`)
  const attributes = execFileSync('git', ['check-attr', 'text', '--', ...paths], {
    cwd: fileURLToPath(new URL('../../', import.meta.url)), encoding: 'utf8',
  }).trim().split(/\r?\n/)
  assert.equal(attributes.length, paths.length)
  for (const line of attributes) assert.match(line, /: text: unset$/)
})

// This gate independently checks the provenance bytes with Node's OpenSSL
// Ed25519 verifier. It does not execute or substitute for production Rust tests.
test('public Minisign fixture provenance hashes and Tauri wrappers match', () => {
  const provenance = JSON.parse(read('provenance.json'))
  for (const [name, hash] of Object.entries(provenance.sha256)) {
    assert.equal(createHash('sha256').update(read(name)).digest('hex'), hash, name)
  }
  assert.deepEqual(Buffer.from(read('tauri-public-key.txt').toString(), 'base64'), read('public-key.pub'))
  assert.deepEqual(Buffer.from(read('tauri-signature.sig').toString(), 'base64'), read('prehashed.minisig'))
  assert.equal(read('payload.bin').toString(), 'test')
})

test('real upstream prehashed and legacy vectors authenticate bytes and trusted comments', () => {
  const publicPacket = Buffer.from(read('public-key.pub').toString().trimEnd().split('\n')[1], 'base64')
  const key = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicPacket.subarray(10)]), format: 'der', type: 'spki' })
  for (const [file, prehashed] of [['prehashed.minisig', true], ['legacy.minisig', false]]) {
    const lines = read(file).toString().trimEnd().split('\n')
    const packet = Buffer.from(lines[1], 'base64')
    assert.equal(packet.length, 74)
    assert.deepEqual(packet.subarray(2, 10), publicPacket.subarray(2, 10))
    const message = prehashed ? createHash('blake2b512').update(read('payload.bin')).digest() : read('payload.bin')
    assert.equal(verify(null, message, key, packet.subarray(10)), true, `${file}: exact payload`)
    const modified = prehashed ? createHash('blake2b512').update('Test').digest() : Buffer.from('Test')
    assert.equal(verify(null, modified, key, packet.subarray(10)), false, `${file}: modified payload`)
    const trusted = Buffer.from(lines[2].slice('trusted comment: '.length))
    const global = Buffer.from(lines[3], 'base64')
    assert.equal(verify(null, Buffer.concat([packet.subarray(10), trusted]), key, global), true, `${file}: trusted comment`)
    assert.equal(verify(null, Buffer.concat([packet.subarray(10), Buffer.from(trusted.toString().replace('timestamp:', 'Timestamp:'))]), key, global), false)
  }
})

test('committed Tauri publisher key has the expected public-key text envelope', () => {
  const config = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url)))
  const lines = Buffer.from(config.plugins.updater.pubkey, 'base64').toString().trimEnd().split('\n')
  assert.equal(lines.length, 2)
  assert.match(lines[0], /^untrusted comment: /)
  assert.equal(Buffer.from(lines[1], 'base64').length, 42)
  assert.notEqual(lines[1], read('public-key.pub').toString().trimEnd().split('\n')[1])
})
