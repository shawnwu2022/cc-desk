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
  notes_i18n?: { en: string; zh?: string }
  pub_date: string
  platforms: Record<string, { signature: string; url: string }>
}

type BuildUpdaterManifest = (input: {
  repository: string
  tag: string
  assets: UpdaterAsset[]
  pubkey: string
  notes?: string
  notesByLocale?: { en: string; zh?: string }
  pubDate?: string
}) => UpdaterManifest

const requireModule = createRequire(import.meta.url)
const scriptPath = resolve(process.cwd(), 'scripts/generate-updater-manifest.js')
const { buildUpdaterManifest } = requireModule('../../scripts/generate-updater-manifest.js') as {
  buildUpdaterManifest: BuildUpdaterManifest
}
const { readNotes, renderDocument, composeReleaseNotes } = requireModule('../../scripts/release-notes.js')

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
  it('ReleaseNotes_ExactVersionAndDocuments_012', () => {
    const source = { schemaVersion: 1, releases: {
      Unreleased: { en: 'Future changes', zh: '未来更新' },
      '1.2.3': { en: 'Fixed\n\n- Session titles.\n\nImportant\n\n- Keep backups.', zh: '修复\n\n- 会话标题。\n\n重要说明\n\n- 保留备份。' },
    } }
    expect(readNotes('v1.2.3', source)).toEqual(source.releases['1.2.3'])
    expect(readNotes('v1.2.4', source)).toBeNull()
    expect(renderDocument(source, 'en')).toContain('Keep backups.')
    expect(renderDocument(source, 'en')).not.toContain('保留备份。')
    expect(renderDocument(source, 'zh')).toContain('保留备份。')
    expect(() => readNotes('1.2.3', { schemaVersion: 1, releases: { '1.2.3': { zh: '不能假装有英文摘要' } } })).toThrow(/English/)
    const actual = JSON.parse(readFileSync('docs/release-notes.json', 'utf8'))
    expect(readFileSync('RELEASE_NOTES.md', 'utf8')).toBe(renderDocument(actual, 'en'))
    expect(readFileSync('RELEASE_NOTES_CN.md', 'utf8')).toBe(renderDocument(actual, 'zh'))
  })

  it('ReleaseNotes_PublicBodyRetainsExactDisclosure_013', () => {
    const technical = 'Validation policy: source-bound\nSource: ' + 'a'.repeat(40) + '\nImportant historical limitation.   \n\n'
    const source = { schemaVersion: 1, releases: { '1.2.3': { en: 'Fixed\n\n- Better session titles.\n\nImportant\n\n- Back up shared data.', zh: '中文主要日志。' } } }
    const input = { version: 'v1.2.3', repository: 'shawnwu2022/cc-desk', sha: 'a'.repeat(40), technical, data: source }
    const body = composeReleaseNotes(input)
    expect(body.startsWith(source.releases['1.2.3'].en)).toBe(true)
    expect(body.indexOf('Back up shared data.')).toBeLessThan(body.indexOf('<details>'))
    expect(body).toContain(technical)
    expect(body).not.toContain('中文主要日志。')
    expect(body).toContain('/blob/' + input.sha + '/RELEASE_NOTES_CN.md')
    expect(() => composeReleaseNotes({ ...input, version: 'v1.2.4' })).toThrow(/versioned English/)
  })
  // 默认给旧客户端英文主要说明，支持语言的客户端从独立字段选择对应摘要。
  it('ReleaseNotes_ManifestKeepsEnglishDefault_011', () => {
    const localized = { en: 'Fixed\n\n- Keep terminal function keys working.\n\nImportant\n\n- Back up shared data before switching versions.', zh: '修复\n\n- 终端功能键不再被应用抢走。\n\n重要说明\n\n- 切换版本前备份共享数据。' }
    const manifest = buildUpdaterManifest({ repository: 'x/y', tag: 'v1.2.3', assets, pubkey, notesByLocale: localized })
    expect(manifest.notes).toBe(localized.en)
    expect(manifest.notes_i18n).toEqual(localized)
  })
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
      copyFileSync(resolve(process.cwd(), 'scripts/release-notes.js'), join(root, 'scripts/release-notes.js'))
      mkdirSync(join(root, 'docs'))
      writeFileSync(join(root, 'docs/release-notes.json'), JSON.stringify({ schemaVersion: 1, releases: {
        Unreleased: { en: 'Not this version' }, '1.2.3': { en: 'Main English update', zh: '主要中文更新' },
      } }))
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
      expect(manifest.notes).toBe('Main English update')
      expect(manifest.notes_i18n?.zh).toBe('主要中文更新')
      expect(manifest.platforms['windows-x86_64'].url).toContain('/releases/download/v1.2.3/')
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})
