import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

const source = (name: string) => readFileSync(`android-client/src/cn/ccdesk/android/${name}.java`, 'utf8')
const backend = source('NativeDesktopBackend')
const journal = source('PairingRecoveryJournal')
const activity = source('MainActivity')

it('AndroidPairing_StagesBeforeNativeAndPublishesOnlyAfterDurableIndex_001', () => {
  expect(backend).toMatch(/recovery\.stage\(attempt\.link\);\s*attempt\.handle = nativeBeginPairing/)
  expect(backend).toMatch(/if \(!recovery\.promote\(attempt\.link, desktop, store\)\)[\s\S]*paired\.put\(attempt\.id,[\s\S]*attempt\.callback\.approved\(desktop\)/)
  expect(journal).toMatch(/store\.addApproved\(approved\);\s*remove\(link\.id\);/)
  expect(journal).not.toContain('copyCode')
  expect(activity).not.toContain('store.addApproved')
})

it('AndroidPairing_InterruptedIndexRestoresOnlyUnderOriginalVerifiedReceipt_002', () => {
  const restore = backend.slice(backend.indexOf('    private void restoreInterrupted'), backend.indexOf('    boolean verified'))
  expect(restore.match(/keys\.open\(link\.id/g)).toHaveLength(3)
  expect(restore).toMatch(/nativeReadPairedHostId[\s\S]*!link\.accepts\(verified\)[\s\S]*recovery\.promote\(link, desktop, store\)/)
  expect(restore).not.toContain('nativeBeginPairing')
  expect(restore).not.toContain('loadShares')
  expect(backend).toMatch(/void cancelEnrollment\(\)[\s\S]*recovery\.remove\(pending\.id\);\s*cancelPending\(\);/)
  expect(journal).toMatch(/if \(!file\(link\.id\)\.getBaseFile\(\)\.exists\(\)\) return false;/)
})
