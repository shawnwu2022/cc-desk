#!/usr/bin/env node

const fs = require('fs')

function readManifest(source) {
  if (/^https?:\/\//i.test(source)) {
    return fetch(source).then(async response => {
      if (!response.ok) throw new Error(`failed to fetch updater manifest: HTTP ${response.status}`)
      return response.json()
    })
  }

  return Promise.resolve(JSON.parse(fs.readFileSync(source, 'utf8')))
}

async function verifyUpdaterManifestUrls(manifest, request = fetch) {
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

async function main() {
  const source = process.argv[2]
  if (!source) throw new Error('usage: verify-updater-manifest.js <manifest-file-or-url>')
  const manifest = await readManifest(source)
  await verifyUpdaterManifestUrls(manifest)
  console.log('Verified updater asset URLs')
}

module.exports = { readManifest, verifyUpdaterManifestUrls }

if (require.main === module) {
  main().catch(error => {
    console.error(error.message)
    process.exit(1)
  })
}
