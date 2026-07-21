#!/usr/bin/env node

const fs = require('fs')
const path = require('path')

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

function buildUpdaterManifest({ repository, tag, assets, notes = '', pubDate = new Date().toISOString() }) {
  if (!repository || !tag) throw new Error('repository and tag are required')

  const platforms = {}
  for (const [platform, matcher] of Object.entries(PLATFORM_MATCHERS)) {
    const asset = assets.find(item => matcher.test(item.name))
    if (!asset) throw new Error(`missing updater asset for ${platform}`)
    if (!asset.signature) throw new Error(`missing signature for ${asset.name}`)

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
        signature: fs.readFileSync(signaturePath, 'utf8'),
      }
    })
}

function main() {
  const artifactsDir = path.resolve(process.argv[2] || 'artifacts')
  const outputPath = path.resolve(process.argv[3] || 'latest.json')
  const repository = process.env.GITHUB_REPOSITORY
  const tag = process.env.GITHUB_REF_NAME
  const manifest = buildUpdaterManifest({
    repository,
    tag,
    assets: collectAssets(artifactsDir),
  })
  fs.writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(`Wrote updater manifest: ${outputPath}`)
}

module.exports = { buildUpdaterManifest, collectAssets }

if (require.main === module) main()