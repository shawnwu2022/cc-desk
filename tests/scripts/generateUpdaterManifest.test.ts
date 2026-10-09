import { createRequire } from 'node:module'
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createHash, generateKeyPairSync, sign } from 'node:crypto'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'

type UpdaterAsset = {
  name: string
  signature: string
  data: Buffer
}

type UpdaterManifest = {
  version: string
  notes: string
  pub_date: string
  platforms: Record<string, { signature: string; url: string }>
}

type BuildUpdaterManifest = (input: {
  repository: string
  tag: string
  assets: UpdaterAsset[]
  pubkey: string
  notes?: string
  pubDate?: string
}) => UpdaterManifest

const requireModule = createRequire(import.meta.url)
const scriptPath = resolve(process.cwd(), 'scripts/generate-updater-manifest.js')
const { buildUpdaterManifest } = requireModule('../../scripts/generate-updater-manifest.js') as {
  buildUpdaterManifest: BuildUpdaterManifest
}

const { publicKey, privateKey } = generateKeyPairSync('ed25519')
const keyId = Buffer.from('0102030405060708', 'hex')
const publicPacket = Buffer.concat([Buffer.from('Ed'), keyId, publicKey.export({ type: 'spki', format: 'der' }).subarray(-32)])
const pubkey = Buffer.from(`untrusted comment: fixture\n${publicPacket.toString('base64')}\n`).toString('base64')
const assets: UpdaterAsset[] = ['CC Desk_1.2.3_x64-setup.exe', 'CC Desk_aarch64.app.tar.gz', 'CC Desk_1.2.3_amd64.AppImage'].map(name => {
  const data = Buffer.from(`fixture payload: ${name}`)
  const detached = sign(null, createHash('blake2b512').update(data).digest(), privateKey)
  const packet = Buffer.concat([Buffer.from('ED'), keyId, detached])
  const comment = `timestamp:1\tfile:${name}`
  const global = sign(null, Buffer.concat([detached, Buffer.from(comment)]), privateKey)
  const signature = Buffer.from(`untrusted comment: fixture\n${packet.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}\n`).toString('base64')
  return { name, signature, data }
})

describe('generate updater manifest', () => {
  it('UpdaterManifest_AllPlatforms_001', () => {
    const manifest = buildUpdaterManifest({
      repository: 'shawnwu2022/cc-desk',
      tag: 'v1.2.3',
      assets,
      pubkey,
      notes: 'test notes',
      pubDate: '2026-07-20T00:00:00.000Z',
    })

    expect(manifest.version).toBe('1.2.3')
    expect(manifest.notes).toBe('test notes')
    expect(manifest.pub_date).toBe('2026-07-20T00:00:00.000Z')
    expect(manifest.platforms['windows-x86_64'].signature).toBe(assets[0].signature)
    expect(manifest.platforms['darwin-aarch64'].signature).toBe(assets[1].signature)
    expect(manifest.platforms['linux-x86_64'].signature).toBe(assets[2].signature)
  })

  it('UpdaterManifest_PublishedAssetName_002', () => {
    const manifest = buildUpdaterManifest({
      repository: 'shawnwu2022/cc-desk',
      tag: 'v1.2.3',
      assets,
      pubkey,
    })

    expect(manifest.platforms['windows-x86_64'].url).toBe(
      'https://github.com/shawnwu2022/cc-desk/releases/download/v1.2.3/CC.Desk_1.2.3_x64-setup.exe',
    )
    expect(manifest.platforms['darwin-aarch64'].url).toBe(
      'https://github.com/shawnwu2022/cc-desk/releases/download/v1.2.3/CC.Desk_aarch64.app.tar.gz',
    )
    expect(manifest.platforms['linux-x86_64'].url).toBe(
      'https://github.com/shawnwu2022/cc-desk/releases/download/v1.2.3/CC.Desk_1.2.3_amd64.AppImage',
    )
  })

  it('UpdaterManifest_MissingPlatform_003', () => {
    expect(() =>
      buildUpdaterManifest({
        repository: 'x/y',
        tag: 'v1.0.0',
        assets: assets.slice(0, 2),
        pubkey,
      }),
    ).toThrow(/exactly one updater asset for linux-x86_64/)
  })

  it('UpdaterManifest_RejectsBranchNameAsVersion_004', () => {
    expect(() =>
      buildUpdaterManifest({
        repository: 'shawnwu2022/cc-desk',
        tag: 'main',
        assets,
        pubkey,
      }),
    ).toThrow(/release tag/i)
  })

  it('UpdaterManifest_CliUsesExplicitReleaseTag_005', () => {
    const root = mkdtempSync(join(tmpdir(), 'cc-desk-updater-manifest-'))
    const artifactsDir = join(root, 'artifacts')
    const outputPath = join(root, 'latest.json')
    const fixtureScript = join(root, 'scripts/generate-updater-manifest.js')

    try {
      mkdirSync(dirname(fixtureScript), { recursive: true })
      copyFileSync(scriptPath, fixtureScript)
      copyFileSync(resolve(process.cwd(), 'scripts/updater-signature.js'), join(root, 'scripts/updater-signature.js'))
      mkdirSync(join(root, 'src-tauri'))
      writeFileSync(join(root, 'src-tauri/tauri.conf.json'), JSON.stringify({ plugins: { updater: { pubkey } } }))
      for (const asset of assets) {
        const assetPath = join(artifactsDir, asset.name)
        mkdirSync(dirname(assetPath), { recursive: true })
        writeFileSync(assetPath, asset.data)
        writeFileSync(`${assetPath}.sig`, asset.signature)
      }

      const result = spawnSync(
        process.execPath,
        [fixtureScript, artifactsDir, outputPath, 'v1.2.3'],
        {
          encoding: 'utf8',
          env: {
            ...process.env,
            GITHUB_REPOSITORY: 'shawnwu2022/cc-desk',
            GITHUB_REF_NAME: 'main',
          },
        },
      )

      expect(result.status, result.stderr).toBe(0)
      const manifest = JSON.parse(readFileSync(outputPath, 'utf8')) as UpdaterManifest
      expect(manifest.version).toBe('1.2.3')
      expect(manifest.platforms['windows-x86_64'].url).toContain('/releases/download/v1.2.3/')
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})
