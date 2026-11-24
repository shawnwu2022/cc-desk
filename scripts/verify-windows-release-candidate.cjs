const { createHash } = require('node:crypto')
const { readFileSync, readdirSync } = require('node:fs')
const { join, resolve } = require('node:path')
const { verifyUpdaterSignature } = require('./updater-signature.js')

function inspectWindowsCandidate(directory, pubkey) {
  const entries = readdirSync(directory, { withFileTypes: true })
  const installers = entries.filter(entry => /\.exe$/i.test(entry.name))
  const signatures = entries.filter(entry => /\.exe\.sig$/i.test(entry.name))
  if (installers.length !== 1 || signatures.length !== 1 ||
      !installers[0].isFile() || !signatures[0].isFile()) {
    throw new Error('expected exactly one regular Windows installer and one regular updater signature')
  }
  if (signatures[0].name !== installers[0].name + '.sig') throw new Error('expected a matching signature')
  const installer = resolve(directory, installers[0].name)
  const bytes = readFileSync(installer)
  const signature = readFileSync(join(directory, signatures[0].name))
  verifyUpdaterSignature(bytes, signature.toString('utf8'), pubkey)
  const hash = value => createHash('sha256').update(value).digest('hex')
  return { installer, sha256: hash(bytes), signatureSha256: hash(signature) }
}

if (require.main === module) {
  const tauri = require('../src-tauri/tauri.conf.json')
  const directory = join(__dirname, '../src-tauri/target/release/bundle/nsis')
  console.log(JSON.stringify(inspectWindowsCandidate(directory, tauri.plugins.updater.pubkey)))
}

module.exports = { inspectWindowsCandidate }
