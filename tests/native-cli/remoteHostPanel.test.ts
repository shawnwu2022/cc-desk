import { reactive } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { beforeEach, expect, it, vi } from 'vitest'
import RemoteHostPanel from '@/components/workspace/RemoteHostPanel.vue'
import * as api from '@/api/tauri'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
const state = vi.hoisted(() => ({ pushToast: vi.fn(), tab: { requestId: 'r1', runId: 'run1', generation: 1, status: 'running' } }))
vi.mock('@/stores/nativeTabs', async importOriginal => ({ ...await importOriginal<typeof import('@/stores/nativeTabs')>(), useNativeTabsStore: () => ({ tab: () => state.tab }) }))
vi.mock('@/stores/notifications', () => ({ useNotificationsStore: () => ({ pushToast: state.pushToast }) }))
vi.mock('@/api/tauri', () => ({ remoteRegistrationRequest: vi.fn(), remoteRegistrationCurrent: vi.fn(), remoteRegistrationCancel: vi.fn(), remoteHostPairingScope: vi.fn(), remoteHostConfigure: vi.fn(), remoteHostCurrent: vi.fn(), remoteHostRequest: vi.fn(), remoteHostStart: vi.fn(), remoteHostCancel: vi.fn(), remoteHostDiscardCandidate: vi.fn() }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }))
beforeEach(() => {
 vi.mocked(writeText).mockResolvedValue(undefined)
 vi.clearAllMocks(); state.tab = reactive({ requestId: 'r1', runId: 'run1', generation: 1, status: 'running' })
 vi.mocked(api.remoteRegistrationRequest).mockResolvedValue({state:'PendingKeyApproval'})
 vi.mocked(api.remoteRegistrationCurrent).mockResolvedValue({state:'PendingBrowserApproval'})
 vi.mocked(api.remoteRegistrationCancel).mockResolvedValue({state:'Cancelled'})
 vi.mocked(api.remoteHostDiscardCandidate).mockResolvedValue(undefined)
 vi.mocked(api.remoteHostConfigure).mockResolvedValue({ state: 'Configured', receipt: '1' })
 vi.mocked(api.remoteHostRequest).mockResolvedValue({ state: 'Pending' })
 vi.mocked(api.remoteHostCurrent).mockResolvedValue({ state: 'ApprovalRecorded' })
 vi.mocked(api.remoteHostStart).mockResolvedValue({ state: 'Consumed' })
 vi.mocked(api.remoteHostCancel).mockResolvedValue({ state: 'Cancelled' })
 vi.mocked(api.remoteHostPairingScope).mockResolvedValue({ session: 'ab'.repeat(16), run: 'run1', generation: '1' })
})
async function click(w: ReturnType<typeof mount>, text: string) { const b = w.findAll('button').find(b => b.text() === text); expect(b).toBeDefined(); await b!.trigger('click'); await flushPromises() }
it('RemoteHostPanel_ExplicitStagesNeverAutoStart_001', async () => {
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}})
 await click(w,'单次远程连接'); await w.get('textarea').setValue('{}'); await click(w,'校验并冻结')
 expect(api.remoteHostConfigure).toHaveBeenCalledWith({runId:'run1',generation:1,candidateJson:'{}'})
 expect(api.remoteHostRequest).not.toHaveBeenCalled(); expect(api.remoteHostStart).not.toHaveBeenCalled()
 await click(w,'请求本机批准'); expect(api.remoteHostStart).not.toHaveBeenCalled()
 await click(w,'刷新状态'); expect(api.remoteHostStart).not.toHaveBeenCalled()
 await click(w,'启动一次'); expect(api.remoteHostStart).toHaveBeenCalledTimes(1)
 expect(w.text()).toContain('不代表已连接'); expect(w.text()).toContain('60秒'); w.unmount()
})
it('RemoteHostPanel_CloseDiscardsLateCandidateReceipt_002', async () => {
 let finish!: (v:{state:'Configured',receipt:'1'})=>void
 vi.mocked(api.remoteHostConfigure).mockReturnValue(new Promise(r=>{finish=r}))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}})
 await click(w,'单次远程连接'); await w.get('textarea').setValue('{}'); await click(w,'校验并冻结'); await click(w,'关闭')
 finish({state:'Configured',receipt:'1'}); await flushPromises()
 expect(w.find('textarea').exists()).toBe(false);expect(api.remoteHostDiscardCandidate).toHaveBeenCalledWith({runId:'run1',generation:1,receipt:'1'})
 expect(api.remoteHostRequest).not.toHaveBeenCalled();expect(api.remoteHostStart).not.toHaveBeenCalled();w.unmount()
})
it('RemoteHostPanel_ErrorDoesNotRetryOrExposePayload_003', async () => {
 vi.mocked(api.remoteHostConfigure).mockRejectedValue(new Error('PRIVATE_PAYLOAD'))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('textarea').setValue('{}');await click(w,'校验并冻结')
 expect(api.remoteHostConfigure).toHaveBeenCalledTimes(1);expect(w.text()).not.toContain('PRIVATE_PAYLOAD');expect(api.remoteHostStart).not.toHaveBeenCalled();w.unmount()
})
it('RemoteHostPanel_CloseDiscardsThenReopensEditable_004', async () => {
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}})
 await click(w,'单次远程连接'); await w.get('textarea').setValue('{}'); await click(w,'校验并冻结'); await click(w,'关闭')
 expect(api.remoteHostDiscardCandidate).toHaveBeenCalledWith({runId:'run1',generation:1,receipt:'1'})
 expect(api.remoteHostCancel).not.toHaveBeenCalled()
 await click(w,'单次远程连接'); expect(w.get('textarea').attributes('disabled')).toBeUndefined()
 await w.get('textarea').setValue('{}'); await click(w,'校验并冻结')
 expect(api.remoteHostConfigure).toHaveBeenCalledTimes(2); expect(api.remoteHostRequest).not.toHaveBeenCalled();w.unmount()
})
it('RemoteHostPanel_RunSwitchDoesNotUseLateApproval_005', async () => {
 let finish!: (v:{state:'ApprovalRecorded'})=>void
 vi.mocked(api.remoteHostRequest).mockReturnValue(new Promise(r=>{finish=r}))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}})
 await click(w,'单次远程连接'); await w.get('textarea').setValue('{}'); await click(w,'校验并冻结'); await click(w,'请求本机批准')
 state.tab={requestId:'r2',runId:'run2',generation:2,status:'running'};await w.setProps({tabId:'t2'});await flushPromises()
 finish({state:'ApprovalRecorded'}); await flushPromises()
 expect(w.find('textarea').exists()).toBe(false);expect(api.remoteHostStart).not.toHaveBeenCalled()
 expect(api.remoteHostCancel).toHaveBeenCalledWith({runId:'run1',generation:1}); w.unmount()
})
it('RemoteHostPanel_UnmountedCleanupFailureUsesStableNotification_006', async () => {
 vi.mocked(api.remoteHostDiscardCandidate).mockRejectedValue(new Error('PRIVATE_FAILURE'))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('textarea').setValue('{}');await click(w,'校验并冻结');w.unmount();await flushPromises()
 expect(state.pushToast).toHaveBeenCalledWith({kind:'warning',messageKey:'remoteHostCleanupUnconfirmed'})
 expect(api.remoteHostDiscardCandidate).toHaveBeenCalledTimes(1)
 expect(JSON.stringify(state.pushToast.mock.calls)).not.toContain('PRIVATE_FAILURE')
})
it('RemoteHostPanel_DisabledBuildShowsFixedReason_007', async () => {
 vi.mocked(api.remoteHostConfigure).mockRejectedValue({code:'REMOTE_HOST_UNAVAILABLE',detail:'PRIVATE_PAYLOAD'})
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('textarea').setValue('{}');await click(w,'校验并冻结')
 expect(w.text()).toContain('当前构建未启用远程Host');expect(w.text()).not.toContain('PRIVATE_PAYLOAD');expect(api.remoteHostStart).not.toHaveBeenCalled();w.unmount()
})
it('RemoteHostScope_ExplicitReadWithoutAuthority_001', async () => {
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}})
 await click(w,'单次远程连接'); expect(api.remoteHostPairingScope).not.toHaveBeenCalled()
 await click(w,'读取公开会话范围')
 expect(api.remoteHostPairingScope).toHaveBeenCalledWith({runId:'run1',generation:1})
 expect(w.get('[aria-label="公开会话范围"]').text()).toBe(JSON.stringify({session:'ab'.repeat(16),run:'run1',generation:'1'}))
 expect(w.text()).toContain('不是授权'); expect(api.remoteHostConfigure).not.toHaveBeenCalled(); expect(api.remoteHostRequest).not.toHaveBeenCalled(); expect(api.remoteHostStart).not.toHaveBeenCalled()
 await click(w,'关闭'); await click(w,'单次远程连接')
 expect(w.find('[aria-label="公开会话范围"]').exists()).toBe(false)
 expect(api.remoteHostCancel).not.toHaveBeenCalled(); expect(api.remoteHostDiscardCandidate).not.toHaveBeenCalled(); w.unmount()
})
it.each(['close','requestId','runId','generation','status','unmount'])('RemoteHostScope_LateReadRejected_%s_002', async change => {
 let finish!:(v:{session:string,run:string,generation:string})=>void
 vi.mocked(api.remoteHostPairingScope).mockReturnValue(new Promise(r=>{finish=r}))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await click(w,'读取公开会话范围')
 await click(w,'读取公开会话范围');expect(api.remoteHostPairingScope).toHaveBeenCalledTimes(1)
 if(change==='close'){await click(w,'关闭');await click(w,'单次远程连接')}
 else if(change==='unmount')w.unmount()
 else { Object.assign(state.tab,{[change]:change==='generation'?2:change==='status'?'stopped':'replacement'}); await flushPromises(); expect(w.find('textarea').exists()).toBe(false) }
 finish({session:'cd'.repeat(16),run:'run1',generation:'1'});await flushPromises()
 expect(w.text()).not.toContain('cd'.repeat(16));expect(state.pushToast).not.toHaveBeenCalled()
 expect(api.remoteHostCancel).not.toHaveBeenCalled(); expect(api.remoteHostDiscardCandidate).not.toHaveBeenCalled(); if(change!=='unmount')w.unmount()
})
it('RemoteHostScope_ReadFailureNoRetryOrCleanup_003', async () => {
 vi.mocked(api.remoteHostPairingScope).mockRejectedValue({code:'FORBIDDEN',detail:'PRIVATE_PAYLOAD'})
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await click(w,'读取公开会话范围')
 expect(w.text()).not.toContain('PRIVATE_PAYLOAD');expect(w.find('[aria-label="公开会话范围"]').exists()).toBe(false)
 expect(api.remoteHostPairingScope).toHaveBeenCalledTimes(1);w.unmount();await flushPromises();expect(state.pushToast).not.toHaveBeenCalled()
})
it('RemoteHostScope_OldFailureCannotClearNewRead_004', async () => {
 let fail!:(v:unknown)=>void, finish!:(v:{session:string,run:string,generation:string})=>void
 vi.mocked(api.remoteHostPairingScope).mockReturnValueOnce(new Promise((_,r)=>{fail=r})).mockReturnValueOnce(new Promise(r=>{finish=r}))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await click(w,'读取公开会话范围');await click(w,'关闭');await click(w,'单次远程连接');await click(w,'读取公开会话范围')
 fail(new Error('PRIVATE_OLD'));await flushPromises();expect(w.text()).not.toContain('未能读取');expect(state.pushToast).not.toHaveBeenCalled()
 await click(w,'读取公开会话范围');expect(api.remoteHostPairingScope).toHaveBeenCalledTimes(2)
 finish({session:'ef'.repeat(16),run:'run1',generation:'1'});await flushPromises();expect(w.get('[aria-label="公开会话范围"]').text()).toContain('ef'.repeat(16));w.unmount()
})
it('RemoteHostScope_ReadFailureClearsPreviousScope_005', async () => {
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await click(w,'读取公开会话范围')
 vi.mocked(api.remoteHostPairingScope).mockRejectedValueOnce({code:'REMOTE_HOST_UNAVAILABLE'})
 await click(w,'读取公开会话范围');expect(w.find('[aria-label="公开会话范围"]').exists()).toBe(false);w.unmount()
})

const publicBrowserCandidate = {
 version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw'},
 localSpki:Buffer.alloc(91,1).toString('base64url'),hostSpki:Buffer.alloc(91,2).toString('base64url'),
 trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},
 grant:{issuerSpki:Buffer.alloc(91,3).toString('base64url'),principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw',credentialSpki:Buffer.alloc(91,4).toString('base64url'),backupEligible:false,allowBackedUp:false}},
 relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},
 artifacts:{wasm:'https://desk.invalid/client.wasm',glue:'https://desk.invalid/client.js'},
}
function expectNoAuthorityCalls(){expect(api.remoteHostConfigure).not.toHaveBeenCalled();expect(api.remoteHostRequest).not.toHaveBeenCalled();expect(api.remoteHostStart).not.toHaveBeenCalled()}
it('BrowserCandidate_OfflinePreviewDoesNotApproveOrReadScope_001',async()=>{
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接')
 await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate));await click(w,'预检Browser候选')
 expect(JSON.parse((w.get('[aria-label="待独立批准的Browser候选JSON"]').element as HTMLTextAreaElement).value)).toEqual(publicBrowserCandidate)
 expect(w.text()).toContain('尚未批准');expect(api.remoteHostPairingScope).not.toHaveBeenCalled();expectNoAuthorityCalls();w.unmount()
})
it('BrowserCandidate_ExplicitScopeUpdatePreservesOtherPublicFields_002',async()=>{
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接')
 const text=JSON.stringify(publicBrowserCandidate);await w.get('[aria-label="Browser公开候选JSON"]').setValue(text);await click(w,'读取公开会话范围')
 expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false)
 await click(w,'用当前会话范围生成候选')
 const expected=structuredClone(publicBrowserCandidate);expected.grant.scope={session:'ab'.repeat(16),run:'run1',generation:'1'}
 expect(JSON.parse((w.get('[aria-label="待独立批准的Browser候选JSON"]').element as HTMLTextAreaElement).value)).toEqual(expected)
 expect((w.get('[aria-label="Browser公开候选JSON"]').element as HTMLTextAreaElement).value).toBe(text)
 expect((w.get('[aria-label="公开配对JSON"]').element as HTMLTextAreaElement).value).toBe('')
 expectNoAuthorityCalls();w.unmount()
})
it.each(['duplicate','unknown','tooLarge'])('BrowserCandidate_InvalidInputClearsPreview_%s_003',async kind=>{
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接')
 await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate));await click(w,'预检Browser候选')
 const invalid=kind==='duplicate'?JSON.stringify(publicBrowserCandidate).replace('"version":1','"version":1,"version":1'):kind==='unknown'?JSON.stringify({...publicBrowserCandidate,PRIVATE_SENTINEL:'PRIVATE_PAYLOAD'}):' '.repeat(16385)
 await w.get('[aria-label="Browser公开候选JSON"]').setValue(invalid);expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false);await click(w,'预检Browser候选')
 expect(w.text()).toContain('公开候选格式无效');expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false);expect(w.text()).not.toContain('PRIVATE_PAYLOAD');expectNoAuthorityCalls();w.unmount()
})
it('BrowserCandidate_RefreshScopeClearsPreviewAndLateCloseCannotRestore_004',async()=>{
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate));await click(w,'读取公开会话范围');await click(w,'用当前会话范围生成候选')
 let finish!:(value:{session:string,run:string,generation:string})=>void;vi.mocked(api.remoteHostPairingScope).mockReturnValueOnce(new Promise(r=>{finish=r}));await click(w,'读取公开会话范围')
 expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false);await click(w,'关闭');await click(w,'单次远程连接');finish({session:'cd'.repeat(16),run:'run1',generation:'1'});await flushPromises()
 expect((w.get('[aria-label="Browser公开候选JSON"]').element as HTMLTextAreaElement).value).toBe('');expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false);expectNoAuthorityCalls();w.unmount()
})

it('BrowserCandidate_ClipboardOnlyExplicitAndLateResultCannotMarkNewDraft_005',async()=>{
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate));await click(w,'预检Browser候选')
 const preview=(w.get('[aria-label="待独立批准的Browser候选JSON"]').element as HTMLTextAreaElement).value;expect(writeText).not.toHaveBeenCalled()
 let finish!:()=>void;vi.mocked(writeText).mockReturnValueOnce(new Promise(r=>{finish=r}));await click(w,'复制未授权候选');expect(writeText).toHaveBeenCalledExactlyOnceWith(preview)
 await w.get('[aria-label="Browser公开候选JSON"]').setValue('{}');finish();await flushPromises();expect(w.text()).not.toContain('已复制');expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false);expectNoAuthorityCalls();w.unmount()
})
it('BrowserCandidate_ClipboardFailureIsFixedAndRetainsManualOutput_006',async()=>{
 vi.mocked(writeText).mockRejectedValueOnce(new Error('PRIVATE_CLIPBOARD_FAILURE'));const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate));await click(w,'预检Browser候选');await click(w,'复制未授权候选')
 expect(w.text()).toContain('复制未确认');expect(w.text()).not.toContain('PRIVATE_CLIPBOARD_FAILURE');expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(true);expect(writeText).toHaveBeenCalledTimes(1);expectNoAuthorityCalls();w.unmount()
})

it.each(['requestId', 'runId', 'generation', 'status'])('BrowserCandidate_AttemptChangeDropsDraftAndOutput_%s_007', async change => {
  const w = mount(RemoteHostPanel, { props: { tabId: 't1' } })
  await click(w, '单次远程连接')
  await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate))
  await click(w, '读取公开会话范围')
  await click(w, '用当前会话范围生成候选')
  expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(true)
  Object.assign(state.tab, { [change]: change === 'generation' ? 2 : change === 'status' ? 'stopped' : 'replacement' })
  await flushPromises()
  expect(w.find('[data-browser-candidate-editor]').exists()).toBe(false)
  if (change !== 'status') {
    await click(w, '单次远程连接')
    expect((w.get('[aria-label="Browser公开候选JSON"]').element as HTMLTextAreaElement).value).toBe('')
    expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false)
  }
  expectNoAuthorityCalls()
  expect(writeText).not.toHaveBeenCalled()
  w.unmount()
})

it.each([
  { session: 'invalid', run: 'run1', generation: '1' },
  { session: 'ab'.repeat(16), run: '', generation: '1' },
  { session: 'ab'.repeat(16), run: 'run1', generation: '0' },
])('BrowserCandidate_InvalidScopeCannotProduceCandidate_008_%#', async scope => {
  vi.mocked(api.remoteHostPairingScope).mockResolvedValueOnce(scope)
  const w = mount(RemoteHostPanel, { props: { tabId: 't1' } })
  await click(w, '单次远程连接')
  await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate))
  await click(w, '预检Browser候选')
  await click(w, '读取公开会话范围')
  await click(w, '用当前会话范围生成候选')
  expect(w.text()).toContain('公开候选格式无效')
  expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false)
  expectNoAuthorityCalls()
  expect(writeText).not.toHaveBeenCalled()
  w.unmount()
})

it('BrowserCandidate_FailedScopeReadClearsOutputWithoutRetry_009', async () => {
  const w = mount(RemoteHostPanel, { props: { tabId: 't1' } })
  await click(w, '单次远程连接')
  await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate))
  await click(w, '读取公开会话范围')
  await click(w, '用当前会话范围生成候选')
  vi.mocked(api.remoteHostPairingScope).mockRejectedValueOnce(new Error('PRIVATE_SCOPE_FAILURE'))
  await click(w, '读取公开会话范围')
  expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false)
  expect(w.text()).not.toContain('PRIVATE_SCOPE_FAILURE')
  expect(w.findAll('button').find(b => b.text() === '用当前会话范围生成候选')!.attributes('disabled')).toBeDefined()
  expect(api.remoteHostPairingScope).toHaveBeenCalledTimes(2)
  expectNoAuthorityCalls()
  w.unmount()
})

it('BrowserCandidate_LateClipboardResultCannotAffectReopenedPanel_010', async () => {
  const w = mount(RemoteHostPanel, { props: { tabId: 't1' } })
  await click(w, '单次远程连接')
  await w.get('[aria-label="Browser公开候选JSON"]').setValue(JSON.stringify(publicBrowserCandidate))
  await click(w, '预检Browser候选')
  let finish!: () => void
  vi.mocked(writeText).mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  await click(w, '复制未授权候选')
  await click(w, '关闭')
  await click(w, '单次远程连接')
  finish()
  await flushPromises()
  expect(w.text()).not.toContain('已复制')
  expect(w.find('[aria-label="待独立批准的Browser候选JSON"]').exists()).toBe(false)
  expect(writeText).toHaveBeenCalledTimes(1)
  expectNoAuthorityCalls()
  expect(state.pushToast).not.toHaveBeenCalled()
  w.unmount()
})

it('NativeRegistrationPanel_ExplicitFirstYesNeverStartsHost_001',async()=>{
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接')
 await w.get('[aria-label="首次登记JSON"]').setValue('{}');await click(w,'请求登记密钥使用')
 expect(api.remoteRegistrationRequest).toHaveBeenCalledExactlyOnceWith({runId:'run1',generation:1,candidateJson:'{"remember":false}'})
 await click(w,'请求登记密钥使用');expect(api.remoteRegistrationRequest).toHaveBeenCalledTimes(1)
 expectNoAuthorityCalls();expect(w.get('[aria-label="登记状态"]').text()).toBe('PendingKeyApproval')
 vi.mocked(api.remoteRegistrationCurrent).mockResolvedValue({state:'InstalledLocked',receipt:'42',remembered:false})
 await click(w,'刷新登记状态');expectNoAuthorityCalls();expect(w.text()).toContain('本次会话候选仍为锁定')
 await click(w,'请求本机批准');expect(api.remoteHostStart).not.toHaveBeenCalled();w.unmount()
})
it('NativeRegistrationPanel_CloseCancelsOriginalAndLateResult_002',async()=>{
 let finish!:(v:api.RemoteRegistrationState)=>void
 vi.mocked(api.remoteRegistrationRequest).mockReturnValue(new Promise(r=>{finish=r}))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('[aria-label="首次登记JSON"]').setValue('{}');await click(w,'请求登记密钥使用');await click(w,'关闭')
 expect(api.remoteRegistrationCancel).toHaveBeenCalledWith({runId:'run1',generation:1})
 finish({state:'PendingKeyApproval'});await flushPromises();expect(api.remoteRegistrationCancel).toHaveBeenCalledTimes(2)
 expect(w.find('[aria-label="登记状态"]').exists()).toBe(false);expectNoAuthorityCalls();w.unmount()
})
it('NativeRegistrationPanel_FailureNeverLeaksOrRetries_003',async()=>{
 vi.mocked(api.remoteRegistrationRequest).mockRejectedValue(new Error('PRIVATE_REGISTRATION_FAILURE'))
 const w=mount(RemoteHostPanel,{props:{tabId:'t1'}});await click(w,'单次远程连接');await w.get('[aria-label="首次登记JSON"]').setValue('{}');await click(w,'请求登记密钥使用')
 expect(w.text()).not.toContain('PRIVATE_REGISTRATION_FAILURE');expect(api.remoteRegistrationRequest).toHaveBeenCalledTimes(1);expectNoAuthorityCalls();w.unmount()
})
