import { createRequire } from 'node:module'
import { describe, expect, it } from 'vitest'

type UpdaterAsset = {
  name: string
  signature: string
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
  notes?: string
  pubDate?: string
}) => UpdaterManifest

const requireModule = createRequire(import.meta.url)
const { buildUpdaterManifest } = requireModule('../../scripts/generate-updater-manifest.js') as {
  buildUpdaterManifest: BuildUpdaterManifest
}

const assets: UpdaterAsset[] = [
  { name: 'CC Desk_1.2.3_x64-setup.exe', signature: 'win-sig\n' },
  { name: 'CC Desk_aarch64.app.tar.gz', signature: 'mac-sig\n' },
  { name: 'CC Desk_1.2.3_amd64.AppImage', signature: 'linux-sig\n' },
]

describe('generate updater manifest', () => {
  it('UpdaterManifest_AllPlatforms_001', () => {
    const manifest = buildUpdaterManifest({
      repository: 'shawnwu2022/cc-desk',
      tag: 'v1.2.3',
      assets,
      notes: 'test notes',
      pubDate: '2026-07-20T00:00:00.000Z',
    })

    expect(manifest.version).toBe('1.2.3')
    expect(manifest.notes).toBe('test notes')
    expect(manifest.pub_date).toBe('2026-07-20T00:00:00.000Z')
    expect(manifest.platforms['windows-x86_64'].signature).toBe('win-sig')
    expect(manifest.platforms['darwin-aarch64'].signature).toBe('mac-sig')
    expect(manifest.platforms['linux-x86_64'].signature).toBe('linux-sig')
  })

  it('UpdaterManifest_EncodedAssetName_002', () => {
    const manifest = buildUpdaterManifest({
      repository: 'shawnwu2022/cc-desk',
      tag: 'v1.2.3',
      assets,
    })

    expect(manifest.platforms['windows-x86_64'].url).toBe(
      'https://github.com/shawnwu2022/cc-desk/releases/download/v1.2.3/CC%20Desk_1.2.3_x64-setup.exe',
    )
  })

  it('UpdaterManifest_MissingPlatform_003', () => {
    expect(() =>
      buildUpdaterManifest({
        repository: 'x/y',
        tag: 'v1.0.0',
        assets: assets.slice(0, 2),
      }),
    ).toThrow(/missing updater asset for linux-x86_64/)
  })
})