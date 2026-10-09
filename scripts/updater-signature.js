const { createHash, createPublicKey, verify } = require('node:crypto')

function decode(value, label) {
  if (typeof value !== 'string' || !value || !/^[A-Za-z0-9+/]+={0,2}$/.test(value) || value.length % 4 !== 0) {
    throw new Error(`invalid ${label} base64`)
  }
  const bytes = Buffer.from(value, 'base64')
  if (bytes.toString('base64') !== value) throw new Error(`invalid ${label} base64`)
  return bytes
}

// Tauri wraps the complete minisign text in base64. Verify both the payload
// signature and the trusted-comment signature, as minisign's verifier does.
function verifyUpdaterSignature(data, signature, pubkey) {
  if (!Buffer.isBuffer(data)) throw new Error('signature verification requires actual artifact bytes')
  const keyLines = decode(pubkey, 'public key').toString('utf8').trim().split(/\r?\n/)
  if (keyLines.length !== 2 || !keyLines[0].startsWith('untrusted comment: ')) throw new Error('invalid public key text')
  const key = decode(keyLines[1], 'public key packet')
  if (key.length !== 42 || key.subarray(0, 2).toString() !== 'Ed') throw new Error('invalid public key packet')
  const lines = decode(signature.trim(), 'signature').toString('utf8').trim().split(/\r?\n/)
  if (lines.length !== 4 || !lines[0].startsWith('untrusted comment: ') || !lines[2].startsWith('trusted comment: ')) {
    throw new Error('invalid minisign signature text')
  }
  const packet = decode(lines[1], 'signature packet')
  const global = decode(lines[3], 'comment signature')
  if (packet.length !== 74 || global.length !== 64) throw new Error('invalid signature packet length')
  const algorithm = packet.subarray(0, 2).toString()
  if (!['Ed', 'ED'].includes(algorithm) || !packet.subarray(2, 10).equals(key.subarray(2, 10))) {
    throw new Error('signature algorithm or key identity mismatch')
  }
  const publicKey = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]), type: 'spki', format: 'der' })
  const message = algorithm === 'ED' ? createHash('blake2b512').update(data).digest() : data
  const detached = packet.subarray(10)
  if (!verify(null, message, publicKey, detached)) throw new Error('updater artifact signature verification failed')
  const comment = Buffer.from(lines[2].slice('trusted comment: '.length))
  if (!verify(null, Buffer.concat([detached, comment]), publicKey, global)) throw new Error('trusted comment signature verification failed')
}

module.exports = { verifyUpdaterSignature }
