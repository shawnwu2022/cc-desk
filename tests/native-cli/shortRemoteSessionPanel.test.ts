import { mount, flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import ShortRemoteSessionPanel from '@/components/workspace/ShortRemoteSessionPanel.vue'
import * as api from '@/api/tauri'
const toast=vi.hoisted(()=>vi.fn())
vi.mock('@/stores/notifications',()=>({useNotificationsStore:()=>({pushToast:toast})}))
vi.mock('@/api/tauri',()=>({remoteShortRequest:vi.fn(),remoteShortAdvance:vi.fn(),remoteShortCurrent:vi.fn(),remoteShortClose:vi.fn()}))
const id='11111111-1111-4111-8111-111111111111'
const devices:api.RemotePairingList={state:'Locked',storeId:'store',revision:'2',devices:[{deviceId:'phone',browserOrigin:'https://phone.invalid',relayOrigin:'wss://relay.invalid',profileId:'profile',installation:'confirmed'}]}
const run={runId:'run',generation:1}
beforeEach(()=>{vi.resetAllMocks();vi.useFakeTimers();vi.mocked(api.remoteShortClose).mockResolvedValue();vi.mocked(api.remoteShortCurrent).mockResolvedValue({phase:'empty',operationId:null,limitSeconds:10})})
afterEach(()=>vi.useRealTimers())
function button(w:ReturnType<typeof mount>,text:string){return w.findAll('button').find(b=>b.text()===text)!}
it('ShortSession_DefaultOffRequiresConfirmedExplicitSelection_001',async()=>{
 const w=mount(ShortRemoteSessionPanel,{props:{run,devices}})
 expect(api.remoteShortRequest).not.toHaveBeenCalled();expect(api.remoteShortCurrent).not.toHaveBeenCalled()
 expect(button(w,'请求10秒短会话').attributes('disabled')).toBeDefined()
 await w.get('select').setValue('phone');expect(button(w,'请求10秒短会话').attributes('disabled')).toBeUndefined()
 await w.setProps({devices:{...devices,devices:[{...devices.devices[0],installation:'pending'}]}})
 expect(button(w,'请求10秒短会话').attributes('disabled')).toBeDefined();w.unmount()
})
it('ShortSession_ExactSelectionAdvancesSeparateNativeConfirmation_002',async()=>{
 vi.mocked(api.remoteShortRequest).mockResolvedValue({phase:'preparing',operationId:id,limitSeconds:10})
 vi.mocked(api.remoteShortAdvance).mockResolvedValue({phase:'confirming',operationId:id,limitSeconds:10})
 const w=mount(ShortRemoteSessionPanel,{props:{run,devices}});await w.get('select').setValue('phone');await button(w,'请求10秒短会话').trigger('click');await flushPromises()
 expect(api.remoteShortRequest).toHaveBeenCalledExactlyOnceWith({...run,candidateJson:JSON.stringify({storeId:'store',revision:'2',deviceId:'phone'})})
 await vi.advanceTimersByTimeAsync(200);expect(api.remoteShortAdvance).toHaveBeenCalledExactlyOnceWith(id)
 expect(w.text()).toContain('确认和连接均占用这10秒');expect(w.text()).toContain('等待独立短网络确认');w.unmount();await flushPromises();expect(api.remoteShortClose).toHaveBeenCalledWith(id)
})
it('ShortSession_StatusAndCloseSurviveRunExitWithoutAdvance_003',async()=>{
 vi.mocked(api.remoteShortCurrent).mockResolvedValue({phase:'accepted',operationId:id,limitSeconds:10})
 const w=mount(ShortRemoteSessionPanel,{props:{run:null,devices:null}})
 await button(w,'刷新短会话状态').trigger('click');await flushPromises()
 expect(w.text()).toContain('不代表手机已连接');expect(api.remoteShortAdvance).not.toHaveBeenCalled()
 await button(w,'关闭短会话').trigger('click');await flushPromises();expect(api.remoteShortClose).toHaveBeenCalledExactlyOnceWith(id);w.unmount()
})
it('ShortSession_LateRequestAfterUnmountClosesWithoutAdvance_004',async()=>{
 let done!:(v:api.RemoteShortReply)=>void
 vi.mocked(api.remoteShortRequest).mockReturnValue(new Promise(resolve=>done=resolve))
 const w=mount(ShortRemoteSessionPanel,{props:{run,devices}});await w.get('select').setValue('phone');await button(w,'请求10秒短会话').trigger('click');w.unmount()
 done({phase:'preparing',operationId:id,limitSeconds:10});await flushPromises();expect(api.remoteShortClose).toHaveBeenCalledExactlyOnceWith(id);expect(api.remoteShortAdvance).not.toHaveBeenCalled()
})
it('ShortSession_ReadFailureDoesNotClaimClosedOrRetryStart_005',async()=>{
 vi.mocked(api.remoteShortCurrent).mockRejectedValue(Error('offline'))
 const w=mount(ShortRemoteSessionPanel,{props:{run:null,devices:null}});await button(w,'刷新短会话状态').trigger('click');await flushPromises()
 expect(w.text()).toContain('请勿把读取失败当作已关闭');expect(api.remoteShortRequest).not.toHaveBeenCalled();w.unmount()
})

it('ShortSession_RequestFailureCannotRepeatWithinAppAttempt_006',async()=>{
 vi.mocked(api.remoteShortRequest).mockRejectedValue(Error('conflict'))
 const w=mount(ShortRemoteSessionPanel,{props:{run,devices}});await w.get('select').setValue('phone');await button(w,'请求10秒短会话').trigger('click');await flushPromises()
 expect(w.text()).toContain('本次App不再重试');expect(button(w,'请求10秒短会话').attributes('disabled')).toBeDefined();w.unmount()
})

it('ShortSession_LateCleanupFailureRemainsVisibleAfterUnmount_007',async()=>{
 let done!:(v:api.RemoteShortReply)=>void
 vi.mocked(api.remoteShortRequest).mockReturnValue(new Promise(resolve=>done=resolve))
 vi.mocked(api.remoteShortClose).mockRejectedValue(Error('offline'))
 const w=mount(ShortRemoteSessionPanel,{props:{run,devices}});await w.get('select').setValue('phone');await button(w,'请求10秒短会话').trigger('click');w.unmount()
 done({phase:'preparing',operationId:id,limitSeconds:10});await flushPromises()
 expect(api.remoteShortClose).toHaveBeenCalledExactlyOnceWith(id)
 expect(toast).toHaveBeenCalledExactlyOnceWith({kind:'warning',messageKey:'remoteHostCleanupUnconfirmed'})
 expect(api.remoteShortAdvance).not.toHaveBeenCalled()
})
