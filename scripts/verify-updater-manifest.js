#!/usr/bin/env node

const fs = require('fs')
const path = require('path')
const { verifyUpdaterSignature } = require('./updater-signature.js')

function readManifest(source) {
  if (/^https?:\/\//i.test(source)) {
    return fetch(source).then(async response => {
      if (!response.ok) throw new Error(`failed to fetch updater manifest: HTTP ${response.status}`)
      return response.json()
    })
  }

  return Promise.resolve(JSON.parse(fs.readFileSync(source, 'utf8')))
}

async function verifyUpdaterManifestUrls(manifest, expectedVersion, request = fetch) {
  if (expectedVersion && manifest?.version !== expectedVersion) {
    throw new Error(`expected updater version ${expectedVersion}, received ${manifest?.version ?? 'missing'}`)
  }

  const platforms = manifest?.platforms
  if (!platforms || typeof platforms !== 'object') throw new Error('updater manifest has no platforms')

  for (const [platform, entry] of Object.entries(platforms)) {
    if (!entry?.url) throw new Error(`updater manifest has no URL for ${platform}`)
    const response = await request(entry.url, { method: 'HEAD', redirect: 'manual' })
    if (response.status === 404) throw new Error(`updater asset URL returns 404 for ${platform}: ${entry.url}`)
    if (response.status < 200 || response.status >= 400) {
      throw new Error(`updater asset URL is unavailable for ${platform}: HTTP ${response.status}`)
    }
  }
}

async function verifyUpdaterManifest(manifest, expectedVersion, pubkey, repository, request = fetch) {
  if (!expectedVersion || manifest?.version !== expectedVersion) throw new Error('unexpected updater manifest version')
  const matchers = {
    'windows-x86_64': /-setup\.exe$/i,
    'darwin-aarch64': /\.app\.tar\.gz$/i,
    'linux-x86_64': /\.AppImage$/i,
  }
  const platforms = manifest?.platforms
  if (!platforms || Object.keys(platforms).length !== 3 || !Object.keys(matchers).every(key => Object.hasOwn(platforms, key))) {
    throw new Error('updater manifest requires exact three-platform coverage')
  }
  const prefix = `https://github.com/${repository}/releases/download/v${expectedVersion}/`
  for (const [platform, matcher] of Object.entries(matchers)) {
    const entry = platforms[platform]
    if (typeof entry?.url !== 'string' || !entry.url.startsWith(prefix)) throw new Error('updater URL source/tag mismatch')
    const name = decodeURIComponent(entry.url.slice(prefix.length))
    if (name.includes('/') || name.includes('\\') || !matcher.test(name)) throw new Error('updater URL platform mismatch')
    const response = await request(entry.url, { redirect: 'follow' })
    if (!response.ok) throw new Error(`updater asset download failed for ${platform}: HTTP ${response.status}`)
    verifyUpdaterSignature(Buffer.from(await response.arrayBuffer()), entry.signature, pubkey)
  }
}

async function main() {
  const source = process.argv[2]
  const expectedVersion = process.argv[3]
  if (!source) throw new Error('usage: verify-updater-manifest.js <manifest-file-or-url> [expected-version]')
  const manifest = await readManifest(source)
  const config = JSON.parse(fs.readFileSync(path.join(__dirname, '../src-tauri/tauri.conf.json'), 'utf8'))
  await verifyUpdaterManifest(manifest, expectedVersion, config.plugins.updater.pubkey, process.env.GITHUB_REPOSITORY)
  console.log(`Verified updater manifest ${expectedVersion} and all three downloaded artifact signatures`)
}

module.exports = { readManifest, verifyUpdaterManifestUrls, verifyUpdaterManifest }

if (require.main === module) {
  main().catch(error => {
    console.error(error.message)
    process.exit(1)
  })
}
