#!/usr/bin/env node

const fs = require('fs')
const path = require('path')
const { verifyUpdaterSignature } = require('./updater-signature.js')

const PLATFORM_MATCHERS = {
  'windows-x86_64': /-setup\.exe$/i,
  'darwin-aarch64': /\.app\.tar\.gz$/i,
  'linux-x86_64': /\.AppImage$/i,
}

function encodeAssetName(name) {
  return name.split('/').map(encodeURIComponent).join('/')
}

function toPublishedAssetName(name) {
  return name.replaceAll(' ', '.')
}

function buildUpdaterManifest({ repository, tag, assets, pubkey, notes = '', pubDate = new Date().toISOString() }) {
  if (!repository || !tag) throw new Error('repository and tag are required')
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(tag)) {
    throw new Error(`invalid release tag: ${tag}`)
  }

  const platforms = {}
  for (const [platform, matcher] of Object.entries(PLATFORM_MATCHERS)) {
    const matches = assets.filter(item => matcher.test(item.name))
    if (matches.length !== 1) throw new Error(`expected exactly one updater asset for ${platform}, received ${matches.length}`)
    const asset = matches[0]
    if (!asset.signature) throw new Error(`missing signature for ${asset.name}`)
    verifyUpdaterSignature(asset.data, asset.signature, pubkey)

    platforms[platform] = {
      signature: asset.signature.trim(),
      url: `https://github.com/${repository}/releases/download/${tag}/${encodeAssetName(toPublishedAssetName(asset.name))}`,
    }
  }

  return {
    version: tag.replace(/^v/, ''),
    notes,
    pub_date: pubDate,
    platforms,
  }
}

function walkFiles(root) {
  return fs.readdirSync(root, { withFileTypes: true }).flatMap(entry => {
    const fullPath = path.join(root, entry.name)
    return entry.isDirectory() ? walkFiles(fullPath) : [fullPath]
  })
}

function collectAssets(root) {
  const files = walkFiles(root)
  return files
    .filter(file => Object.values(PLATFORM_MATCHERS).some(matcher => matcher.test(path.basename(file))))
    .map(file => {
      const signaturePath = `${file}.sig`
      if (!fs.existsSync(signaturePath)) throw new Error(`missing signature file: ${signaturePath}`)
      return {
        name: path.basename(file),
        data: fs.readFileSync(file),
        signature: fs.readFileSync(signaturePath, 'utf8'),
      }
    })
}

function main() {
  const artifactsDir = path.resolve(process.argv[2] || 'artifacts')
  const outputPath = path.resolve(process.argv[3] || 'latest.json')
  const repository = process.env.GITHUB_REPOSITORY
  const tag = process.argv[4]
  const manifest = buildUpdaterManifest({
    repository,
    tag,
    assets: collectAssets(artifactsDir),
    pubkey: JSON.parse(fs.readFileSync(path.join(__dirname, '../src-tauri/tauri.conf.json'), 'utf8')).plugins.updater.pubkey,
  })
  fs.writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(`Wrote updater manifest: ${outputPath}`)
}

module.exports = { buildUpdaterManifest, collectAssets }

if (require.main === module) main()
