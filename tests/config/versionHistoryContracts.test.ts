import { describe, expect, it } from 'vitest'
import { parseHistoryCatalogPage, parseHistorySelection } from '@/utils/versionHistoryContracts'

const row = {
  releaseId: 'a'.repeat(32), assetId: '576637999', version: '0.17.7', publishedAt: '2026-09-20T10:45:00Z',
  platform: 'windows-x86_64', availablePlatforms: ['windows-x86_64', 'darwin-aarch64', 'linux-x86_64'],
  packageFormat: 'nsis', verification: 'awaiting-verification', selectAllowed: true, installReady: false,
  dataModes: { freshSettings: 'available', keepCurrentData: 'unavailable' }, blockedReason: null,
}

describe('historical release wire boundary', () => {
  it('accepts selectable metadata while keeping byte verification and installation pending', () => {
    expect(parseHistoryCatalogPage({ rows: [row], nextCursor: null, truncated: false }).rows[0].installReady).toBe(false)
    expect(parseHistorySelection({ selectionToken: 'b'.repeat(32), releaseId: row.releaseId, assetId: row.assetId,
      version: row.version, expiresAt: '2026-10-02T06:00:00Z', verification: 'awaiting-verification', installReady: false })).toMatchObject({ version: '0.17.7', installReady: false })
  })

  it('rejects a catalog that upgrades metadata into verified bytes or shared-data approval', () => {
    for (const changed of [
      { installReady: true }, { verification: 'verified' }, { dataModes: { freshSettings: 'available', keepCurrentData: 'available' } },
      { selectAllowed: false, blockedReason: null }, { selectAllowed: true, blockedReason: 'SIGNATURE_MISSING' },
      { assetId: null }, { packageFormat: null }, { platform: 'unsupported' },
    ]) expect(() => parseHistoryCatalogPage({ rows: [{ ...row, ...changed }], nextCursor: null, truncated: false })).toThrow('HISTORY_INVALID_RESPONSE')
  })

  it('rejects native URLs, paths, unknown diagnostic text and duplicate observation identities', () => {
    for (const changed of [
      { downloadUrl: 'https://example.com/installer.exe' }, { installerPath: 'C:\\private\\setup.exe' },
      { blockedReason: 'raw backend error' }, { version: '0.17.7-beta.1' }, { assetId: '0576637999' },
      { publishedAt: 'tomorrow' }, { releaseId: 'https://example.com/release' },
    ]) expect(() => parseHistoryCatalogPage({ rows: [{ ...row, ...changed }], nextCursor: null, truncated: false })).toThrow('HISTORY_INVALID_RESPONSE')
    expect(() => parseHistoryCatalogPage({ rows: [row, row], nextCursor: null, truncated: false })).toThrow('HISTORY_INVALID_RESPONSE')
  })

  it('accepts an explicitly unavailable row and a bounded partial catalog', () => {
    const unavailable = { ...row, assetId: null, packageFormat: null, selectAllowed: false,
      dataModes: { freshSettings: 'unavailable', keepCurrentData: 'unavailable' }, blockedReason: 'PLATFORM_ASSET_MISSING' }
    expect(parseHistoryCatalogPage({ rows: [unavailable], nextCursor: null, truncated: true }).truncated).toBe(true)
  })

  it('rejects oversize pages, URL cursors and selection fields supplied as native authority', () => {
    expect(() => parseHistoryCatalogPage({ rows: Array(26).fill(row), nextCursor: null, truncated: false })).toThrow('HISTORY_INVALID_RESPONSE')
    expect(() => parseHistoryCatalogPage({ rows: [], nextCursor: 'https://api.github.com/other', truncated: false })).toThrow('HISTORY_INVALID_RESPONSE')
    const selection = { selectionToken: 'b'.repeat(32), releaseId: row.releaseId, assetId: row.assetId,
      version: row.version, expiresAt: '2026-10-02T06:00:00Z', verification: 'awaiting-verification', installReady: false }
    for (const changed of [{ installReady: true }, { expiresAt: 'not a date' }, { size: 4 }, { sha256: 'a'.repeat(64) }]) {
      expect(() => parseHistorySelection({ ...selection, ...changed })).toThrow('HISTORY_INVALID_RESPONSE')
    }
  })
})
