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

import preparationWire from '../fixtures/version-history-preparation-wire.json'
import { parsePreparationTicket, parsePreparedPackageSummary, parseCancelPrepareSummary } from '@/utils/versionHistoryContracts'

describe('Rust preparation serialization fixture', () => {
  // Rust共享契约保持发布者验证与安装准入分离。
  it('HistoryWire_Preparation_001', () => {
    expect(parsePreparationTicket(preparationWire.ticket)).toEqual(preparationWire.ticket)
    expect(parsePreparedPackageSummary(preparationWire.prepared)).toEqual(preparationWire.prepared)
    expect(parseCancelPrepareSummary(preparationWire.cancelled)).toEqual(preparationWire.cancelled)
  })
  // 安装授权、路径、错误详情或未知包身份不能借准备回执进入UI。
  it('HistoryWire_RejectAuthority_002', () => {
    for (const changed of [{ installReady: true }, { verification: 'verified' }, { blockedReason: null },
      { path: 'C:\\private\\package.exe' }, { blockedReason: 'raw diagnostics' }]) {
      expect(() => parsePreparedPackageSummary({ ...preparationWire.prepared, ...changed })).toThrow('HISTORY_INVALID_RESPONSE')
    }
    expect(() => parseCancelPrepareSummary({ ...preparationWire.cancelled, cancelled: false })).toThrow('HISTORY_INVALID_RESPONSE')
  })
})

import switchWire from '../fixtures/version-switch-wire.json'
import { parseSwitchReview, parseSwitchTicket } from '@/utils/versionHistoryContracts'

describe('Rust ordinary switch serialization fixture', () => {
  // 检查确切的共享契约，准备身份与管理器UUID不可混用。
  it('HistoryWire_SwitchReview_003', () => {
    for (const review of switchWire.reviews) expect(parseSwitchReview(review)).toEqual(review)
    expect(parseSwitchTicket(switchWire.ticket)).toEqual(switchWire.ticket)
  })
  // 路径、原始错误、未知策略或矛盾的签发身份必须拒绝。
  it('HistoryWire_RejectSwitchAuthority_004', () => {
    for (const changed of [{ path: '/private/package' }, { contextPolicy: 'keep-current-data' },
      { allowedActions: ['install'] }, { allowedActions: ['review', 'review'] }, { blockReason: 'raw error' },
      { transactionId: switchWire.ticket.transactionId }, { preparationId: switchWire.ticket.transactionId }]) {
      expect(() => parseSwitchReview({ ...switchWire.reviews[3], ...changed })).toThrow('HISTORY_INVALID_RESPONSE')
    }
    for (const changed of [{ transactionId: null }, { allowedActions: ['refresh', 'cancel-preparation'] }]) {
      expect(() => parseSwitchReview({ ...switchWire.reviews[4], ...changed })).toThrow('HISTORY_INVALID_RESPONSE')
    }
    expect(() => parseSwitchTicket(preparationWire.ticket)).toThrow('HISTORY_INVALID_RESPONSE')
  })
})

describe('ordinary switch cross-field authority', () => {
  // 阶段、阻止原因和动作必须一致；繁忙或已交接不能授权取消。
  it('HistoryWire_RejectContradictions_005', () => {
    const invalidReviews = [
      { ...switchWire.reviews[3], blockReason: 'PREPARATION_BUSY', allowedActions: ['refresh', 'review', 'cancel-preparation'] },
      { ...switchWire.reviews[3], blockReason: 'HANDOFF_ISSUED', allowedActions: ['refresh', 'review', 'cancel-preparation'] },
      { ...switchWire.reviews[3], blockReason: 'PREPARATION_PENDING' },
      { ...switchWire.reviews[0], blockReason: null },
      { ...switchWire.reviews[4], blockReason: null },
      { ...switchWire.reviews.find(review => review.phase === 'aborted'), blockReason: 'HANDOFF_ISSUED' },
      { ...switchWire.reviews.find(review => review.phase === 'cancelled'), blockReason: 'PREPARATION_FAILED' },
      { ...switchWire.reviews.find(review => review.phase === 'unavailable'), blockReason: 'COORDINATOR_UNAVAILABLE' },
    ]
    for (const review of invalidReviews) expect(() => parseSwitchReview(review)).toThrow('HISTORY_INVALID_RESPONSE')
  })
  // 有效响应可以收紧动作列表，包括繁忙状态仅允许查看或刷新。
  it('HistoryWire_AcceptWithheldSubsets_006', () => {
    const validReviews = [...switchWire.reviews,
      { ...switchWire.reviews[3], blockReason: 'PREPARATION_BUSY', allowedActions: ['refresh', 'review'] },
      { ...switchWire.reviews.find(review => review.phase === 'unavailable')!, blockReason: 'PREPARATION_FAILED' },
    ]
    for (const review of validReviews) {
      expect(parseSwitchReview({ ...review, allowedActions: [] }).allowedActions).toEqual([])
      for (const action of review.allowedActions) {
        expect(parseSwitchReview({ ...review, allowedActions: [action] }).allowedActions).toEqual([action])
      }
    }
  })
})
