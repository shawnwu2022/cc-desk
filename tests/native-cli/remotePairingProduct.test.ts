import { randomUUID } from 'node:crypto'
import { reactive } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { beforeEach, expect, it, vi } from 'vitest'
import RemoteHostPanel from '@/components/workspace/RemoteHostPanel.vue'
import * as api from '@/api/tauri'
const state = vi.hoisted(() => ({ pushToast: vi.fn(), tab: { requestId: 'r1', runId: 'run1', generation: 1, status: 'running' } }))
vi.mock('@/stores/nativeTabs', async original => ({ ...await original<typeof import('@/stores/nativeTabs')>(), useNativeTabsStore: () => ({ tab: () => state.tab }) }))
vi.mock('@/stores/notifications', () => ({ useNotificationsStore: () => ({ pushToast: state.pushToast }) }))
vi.mock('@/api/tauri', () => Object.fromEntries(['remoteRegistrationRequest', 'remoteRegistrationCurrent', 'remoteRegistrationCancel', 'remoteHostPairingScope', 'remoteHostConfigure', 'remoteHostCurrent', 'remoteHostRequest', 'remoteHostStart', 'remoteHostCancel', 'remoteHostDiscardCandidate', 'remotePairingList', 'remotePairingSelect', 'remotePairingForget', 'remotePairingForgetCancel'].map(name => [name, vi.fn()])))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }))
const listing = { state: 'Locked' as const, storeId: 'store1', revision: '3', devices: [{ deviceId: 'device1', browserOrigin: 'https://browser.invalid', relayOrigin: 'wss://relay.invalid', profileId: 'profile1' }] }
const run = { runId: 'run1', generation: 1 }
const selection = { ...run, candidateJson: JSON.stringify({ storeId: 'store1', revision: '3', deviceId: 'device1' }) }
beforeEach(() => {
 vi.resetAllMocks()
 Object.assign(globalThis.crypto, { randomUUID })
 state.tab = reactive({ requestId: 'r1', runId: 'run1', generation: 1, status: 'running' })
 vi.mocked(api.remoteRegistrationRequest).mockResolvedValue({ state: 'PendingKeyApproval' })
 vi.mocked(api.remoteRegistrationCurrent).mockResolvedValue({ state: 'PendingBrowserApproval' })
 vi.mocked(api.remoteRegistrationCancel).mockResolvedValue({ state: 'Cancelled' })
 vi.mocked(api.remotePairingList).mockResolvedValue(structuredClone(listing))
 vi.mocked(api.remotePairingSelect).mockResolvedValue({ state: 'Configured', receipt: 'receipt1' })
 vi.mocked(api.remotePairingForget).mockResolvedValue({ state: 'Forgotten' })
 vi.mocked(api.remoteHostDiscardCandidate).mockResolvedValue(undefined)
 vi.mocked(api.remotePairingForgetCancel).mockResolvedValue(undefined)
 vi.mocked(api.remoteHostRequest).mockResolvedValue({ state: 'Pending' })
 vi.mocked(api.remoteHostCancel).mockResolvedValue({ state: 'Cancelled' })
})
async function click(w: ReturnType<typeof mount>, text: string) { const b = w.findAll('button').find(b => b.text() === text); expect(b, text).toBeDefined(); await b!.trigger('click'); await flushPromises() }
async function panel() { const w = mount(RemoteHostPanel, { props: { tabId: 't1' } }); await click(w, '单次远程连接'); return w }
function noAuthority() { expect(api.remoteHostRequest).not.toHaveBeenCalled(); expect(api.remoteHostStart).not.toHaveBeenCalled(); expect(api.remoteHostConfigure).not.toHaveBeenCalled() }
it.each([false, true])('PairingRemember_ExplicitCheckboxOwnsIntent_001_%s', async remember => {
 const w = await panel()
 expect((w.get('[aria-label="在本机记住公开设备"]').element as HTMLInputElement).checked).toBe(false)
 await w.get('[aria-label="首次登记JSON"]').setValue('{"remember":true}')
 await w.get('[aria-label="在本机记住公开设备"]').setValue(remember)
 await click(w, '请求登记密钥使用')
 expect(api.remoteRegistrationRequest).toHaveBeenCalledExactlyOnceWith({ ...run, candidateJson: JSON.stringify({ remember }) })
 expect(api.remotePairingList).not.toHaveBeenCalled(); noAuthority(); w.unmount()
})
it.each(['{"remember":true,"remember":false}', '{"nested":{"x":1,"x":2}}', '[]', 'null'])('PairingRemember_InvalidJsonNeverNormalized_002_%s', async json => {
 const w = await panel(); await w.get('[aria-label="首次登记JSON"]').setValue(json); await click(w, '请求登记密钥使用')
 expect(api.remoteRegistrationRequest).not.toHaveBeenCalled(); expect(w.text()).toContain('登记请求格式无效'); noAuthority(); w.unmount()
})
it.each(['NativeStoreDeclined', 'NativeStoreFailed', 'NativeStoreUnknown'])('PairingRemember_NoFailureUnknownNeverRetry_003_%s', async result => {
 vi.mocked(api.remoteRegistrationCurrent).mockResolvedValue({ state: result } as api.RemoteRegistrationState)
 const w = await panel(); await w.get('[aria-label="首次登记JSON"]').setValue('{}'); await w.get('[aria-label="在本机记住公开设备"]').setValue(true)
 await click(w, '请求登记密钥使用'); await click(w, '刷新登记状态'); await click(w, '请求登记密钥使用')
 expect(api.remoteRegistrationRequest).toHaveBeenCalledTimes(1); expect(w.text()).toContain('不会自动重试'); noAuthority(); w.unmount()
})
it('PairingRemember_InstalledRemainsLockedWithSeparateHostApproval_004', async () => {
 vi.mocked(api.remoteRegistrationCurrent).mockResolvedValue({ state: 'InstalledLocked', receipt: 'receipt1', remembered: true })
 const w = await panel(); await w.get('[aria-label="首次登记JSON"]').setValue('{}'); await w.get('[aria-label="在本机记住公开设备"]').setValue(true)
 await click(w, '请求登记密钥使用'); await click(w, '刷新登记状态')
 expect(w.text()).toContain('已在本机记住公开设备'); expect(w.text()).toContain('尚无批准'); noAuthority()
 await click(w, '关闭'); await click(w, '单次远程连接'); expect((w.get('[aria-label="在本机记住公开设备"]').element as HTMLInputElement).checked).toBe(false); w.unmount()
})
it('PairingList_ExplicitReadAndSelectNeverRestoreApproval_001', async () => {
 const w = await panel(); expect(api.remotePairingList).not.toHaveBeenCalled()
 await click(w, '刷新已记住设备'); expect(api.remotePairingList).toHaveBeenCalledExactlyOnceWith(run)
 expect(w.get('[aria-label="已记住设备"]').text()).toContain('Locked'); noAuthority()
 await click(w, '用于当前会话'); expect(api.remotePairingSelect).toHaveBeenCalledExactlyOnceWith(selection)
 expect(w.text()).toContain('不恢复旧授权'); expect(w.text()).toContain('尚无批准'); noAuthority()
 expect(w.findAll('button').find(b => b.text() === '启动一次')!.attributes('disabled')).toBeDefined(); w.unmount()
})
it.each(['close', 'requestId', 'runId', 'generation', 'status', 'unmount'])('PairingSelect_LateReceiptDiscarded_002_%s', async change => {
 let finish!: (value: { state: 'Configured'; receipt: string }) => void
 vi.mocked(api.remotePairingSelect).mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
 const w = await panel(); await click(w, '刷新已记住设备'); await click(w, '用于当前会话'); await click(w, '用于当前会话')
 expect(api.remotePairingSelect).toHaveBeenCalledTimes(1)
 if (change === 'close') { await click(w, '关闭'); await click(w, '单次远程连接') }
 else if (change === 'unmount') w.unmount()
 else { Object.assign(state.tab, { [change]: change === 'generation' ? 2 : change === 'status' ? 'stopped' : 'replacement' }); await flushPromises() }
 finish({ state: 'Configured', receipt: 'late1' }); await flushPromises()
 expect(api.remoteHostDiscardCandidate).toHaveBeenCalledExactlyOnceWith({ ...run, receipt: 'late1' }); expect(w.text()).not.toContain('已选为'); noAuthority(); if (change !== 'unmount') w.unmount()
})
it('PairingSelect_ErrorDoesNotRetryOrExposePayload_003', async () => {
 vi.mocked(api.remotePairingSelect).mockRejectedValueOnce(new Error('PRIVATE_PAYLOAD'))
 const w = await panel(); await click(w, '刷新已记住设备'); await click(w, '用于当前会话')
 expect(api.remotePairingSelect).toHaveBeenCalledTimes(1); expect(w.text()).not.toContain('PRIVATE_PAYLOAD'); expect(w.text()).toContain('不会自动重试'); noAuthority(); w.unmount()
})
it('PairingForget_IndependentConfirmationRefreshesWithoutCancellingHost_001', async () => {
 const w = await panel(); await click(w, '刷新已记住设备'); await click(w, '用于当前会话'); await click(w, '请求本机批准')
 vi.mocked(api.remotePairingList).mockResolvedValueOnce({ state: 'Locked', storeId: 'store1', revision: '4', devices: [] })
 expect(w.text()).toContain('不终止独立已批准的连接'); await click(w, '忘记设备（本机确认）')
 expect(api.remotePairingForget).toHaveBeenCalledTimes(1); const forgotten = vi.mocked(api.remotePairingForget).mock.calls[0][0]; expect(forgotten).toMatchObject(run); expect(JSON.parse(forgotten.candidateJson)).toEqual({ ...JSON.parse(selection.candidateJson), requestId: expect.any(String) }); expect(api.remotePairingList).toHaveBeenCalledTimes(2)
 expect(w.get('[aria-label="已记住设备"]').text()).not.toContain('https://browser.invalid'); expect(api.remoteHostCancel).not.toHaveBeenCalled(); expect(api.remoteHostDiscardCandidate).not.toHaveBeenCalled(); w.unmount()
})
it.each(['Declined', 'error'])('PairingForget_NoFailureNeverRetries_002_%s', async result => {
 if (result === 'error') vi.mocked(api.remotePairingForget).mockRejectedValueOnce({ code: 'REMOTE_NATIVE_STORE_UNKNOWN', detail: 'PRIVATE_FAILURE' })
 else vi.mocked(api.remotePairingForget).mockResolvedValueOnce({ state: 'Declined' })
 const w = await panel(); await click(w, '刷新已记住设备'); await click(w, '忘记设备（本机确认）')
 expect(api.remotePairingForget).toHaveBeenCalledTimes(1); expect(api.remotePairingList).toHaveBeenCalledTimes(1)
 expect(w.text()).not.toContain('PRIVATE_FAILURE'); expect(w.text()).toContain('不会自动重试'); noAuthority(); w.unmount()
})
it.each(['list', 'forget'])('PairingList_LateCompletionCannotAffectReopenedPanel_003_%s', async kind => {
 const w = await panel(); let finish!: (value: any) => void
 if (kind === 'list') { vi.mocked(api.remotePairingList).mockReturnValueOnce(new Promise(resolve => { finish = resolve })); await click(w, '刷新已记住设备') }
 else { await click(w, '刷新已记住设备'); vi.mocked(api.remotePairingForget).mockReturnValueOnce(new Promise(resolve => { finish = resolve })); await click(w, '忘记设备（本机确认）'); await click(w, '忘记设备（本机确认）'); expect(api.remotePairingForget).toHaveBeenCalledTimes(1) }
 await click(w, '关闭'); await click(w, '单次远程连接'); finish(kind === 'list' ? listing : { state: 'Forgotten' }); await flushPromises()
 expect(w.text()).not.toContain('https://browser.invalid'); expect(w.text()).not.toContain('已忘记'); expect(api.remotePairingList).toHaveBeenCalledTimes(1); noAuthority(); w.unmount()
})
it('PairingList_FailureClearsStaleTargetsWithoutRetry_004', async () => {
 const w = await panel(); await click(w, '刷新已记住设备'); vi.mocked(api.remotePairingList).mockRejectedValueOnce(new Error('PRIVATE_LIST'))
 await click(w, '刷新已记住设备'); expect(w.text()).not.toContain('https://browser.invalid'); expect(w.text()).not.toContain('PRIVATE_LIST'); expect(api.remotePairingList).toHaveBeenCalledTimes(2); noAuthority(); w.unmount()
})

it.each(['close', 'requestId', 'runId', 'generation', 'status', 'unmount', 'cancel'])('PairingForget_InvalidationSendsExactCancellation_005_%s', async change => {
 let finish!: (value: { state: 'Forgotten' | 'Declined' }) => void
 vi.mocked(api.remotePairingForget).mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
 const w = await panel(); await click(w, '刷新已记住设备'); await click(w, '忘记设备（本机确认）')
 const requestId = JSON.parse(vi.mocked(api.remotePairingForget).mock.calls[0][0].candidateJson).requestId
 expect(requestId).toMatch(/^[0-9a-f-]{36}$/)
 if (change === 'close') { await click(w, '关闭'); await click(w, '单次远程连接') }
 else if (change === 'cancel') await click(w, '取消忘记确认')
 else if (change === 'unmount') w.unmount()
 else { Object.assign(state.tab, { [change]: change === 'generation' ? 2 : change === 'status' ? 'stopped' : 'replacement' }); await flushPromises() }
 expect(api.remotePairingForgetCancel).toHaveBeenCalledExactlyOnceWith({ ...run, candidateJson: JSON.stringify({ requestId }) })
 finish({ state: 'Declined' }); await flushPromises(); expect(api.remotePairingList).toHaveBeenCalledTimes(1)
 expect(w.text()).not.toContain('已忘记设备'); noAuthority(); if (change !== 'unmount') w.unmount()
})
it('PairingRemember_IncompleteBrowserInstallKeepsNativeSaveFact_005', async () => {
 vi.mocked(api.remoteRegistrationCurrent).mockResolvedValue({ state: 'NativeStoredRegistrationIncomplete' })
 const w = await panel(); await w.get('[aria-label="首次登记JSON"]').setValue('{}'); await click(w, '请求登记密钥使用'); await click(w, '刷新登记状态')
 expect(w.text()).toContain('已在本机记住公开设备'); expect(w.text()).toContain('浏览器登记未确认完成'); noAuthority(); w.unmount()
})
it('PairingRemember_OnlyTopLevelBooleanChangesPreservesRawNumbers_006', async () => {
 const w = await panel()
 const input = '{"version":1e0,"nested":{"remember":true},"reme\\u006dber":true,"note":"escaped \\\"remember\\\""}'
 await w.get('[aria-label="首次登记JSON"]').setValue(input); await click(w, '请求登记密钥使用')
 expect(api.remoteRegistrationRequest).toHaveBeenCalledExactlyOnceWith({ ...run, candidateJson: input.replace('"reme\\u006dber":true', '"reme\\u006dber":false') }); w.unmount()
})
