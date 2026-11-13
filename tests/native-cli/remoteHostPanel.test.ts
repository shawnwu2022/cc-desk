import { reactive } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { beforeEach, expect, it, vi } from 'vitest'
import RemoteHostPanel from '@/components/workspace/RemoteHostPanel.vue'
import * as api from '@/api/tauri'
const state = vi.hoisted(() => ({ pushToast: vi.fn(), tab: { requestId: 'r1', runId: 'run1', generation: 1, status: 'running' } }))
vi.mock('@/stores/nativeTabs', async importOriginal => ({ ...await importOriginal<typeof import('@/stores/nativeTabs')>(), useNativeTabsStore: () => ({ tab: () => state.tab }) }))
vi.mock('@/stores/notifications', () => ({ useNotificationsStore: () => ({ pushToast: state.pushToast }) }))
vi.mock('@/api/tauri', () => ({ remoteHostPairingScope: vi.fn(), remoteHostConfigure: vi.fn(), remoteHostCurrent: vi.fn(), remoteHostRequest: vi.fn(), remoteHostStart: vi.fn(), remoteHostCancel: vi.fn(), remoteHostDiscardCandidate: vi.fn() }))
beforeEach(() => {
 vi.clearAllMocks(); state.tab = reactive({ requestId: 'r1', runId: 'run1', generation: 1, status: 'running' })
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
