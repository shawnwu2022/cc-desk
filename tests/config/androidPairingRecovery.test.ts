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

  expect(journal).toMatch(/if \(!file\(link\.id\)\.getBaseFile\(\)\.exists\(\)\) return false;/)
})

it('AndroidPairing_CancelWaitsForTheWorkerAndPreservesAnAlreadyCommittedResult_003', () => {
  const cancelUi = activity.slice(activity.indexOf('private void cancelEnrollment('), activity.indexOf('private void finishEnrollment('))
  expect(cancelUi).toContain('cancellingEnrollment = true')
  expect(cancelUi).not.toMatch(/enrollmentEpoch\+\+|pairingPending = false/)
  expect(cancelUi).toMatch(/backend\.cancelEnrollment\(removed -> runOnUiThread\(\(\) ->[\s\S]*finishEnrollment\(epoch,/)
  expect(activity).toMatch(/finishEnrollment\(epoch, PairingNotice\.SAVED\)/)
  const cancelWorker = backend.slice(backend.indexOf('void cancelEnrollment('), backend.indexOf('@Override public void close()'))
  expect(cancelWorker).toMatch(/executor\.execute\(\(\) -> \{[\s\S]*Attempt attempt = pending;[\s\S]*if \(attempt == null\) return;[\s\S]*recovery\.remove\(attempt\.id\);[\s\S]*cancelPending\(\);[\s\S]*completion\.accept\(removed\)/)
  expect(journal).toMatch(/target\.delete\(\);[\s\S]*!base\.exists\(\)[\s\S]*"\.new"[\s\S]*"\.bak"/)
})