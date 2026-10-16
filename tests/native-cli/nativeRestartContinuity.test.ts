import { expect,it,vi } from 'vitest'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { NativeCliTab } from '@/stores/nativeTabs'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
function fixture() {
  const tab:NativeCliTab={tabId:'tab',cli:'claude',projectId:'project',projectPath:'/repo',profileId:'profile',profileRevision:'1',requestId:'request',runId:'run',generation:1,action:{kind:'new'},status:'exited',errorCode:null,launchRevision:'1',title:'Title',createdAt:1,lastActivityAt:1,observedSessionId:'session'}
  const key=JSON.stringify(['local','claude','root','session'])
  const entry:NativeHistoryEntry={key:'entry',context:{cli:'claude',projectId:'project',projectPath:'/repo',profileId:'profile',profileRevision:'1'},sourceRootKey:'root',loading:false,loaded:true,error:null,requestEpoch:'1',sessions:[{type:'session',sessionKey:key,nativeSessionId:'session',title:'Title',truncated:false,archived:false,cwd:'/repo',updatedAt:null}]}
  const map=new Map([['tab',tab]]),load=vi.fn(async()=>entry),restartTab=vi.fn(async()=>({...tab,generation:2})),runSource=vi.fn(async()=>({cli:'claude',sourceRootKey:'root',profileId:'profile',profileRevision:'1',target:{kind:'run',runId:'run',generation:1},basis:'launch-environment'} as any))
  const adapter=createNativeCliAdapter({tabs:{tabs:map,activeTabId:'tab',setActive:vi.fn(),close:vi.fn(),rename:vi.fn()},history:{all:()=>[entry],load},runtime:{createTab:vi.fn(),restartTab,stopTab:vi.fn(),renameTab:vi.fn()},archive:{getArchivedSessions:()=>[],archiveSession:vi.fn(),restoreSession:vi.fn()},runSource})
  return {tab,key,entry,map,load,restartTab,runSource,adapter}
}
it('Restart_NewSessionWithFreshExactHistoryResumesSameId_001',async()=>{
  const h=fixture();await h.adapter.restartSession('native-tab:tab')
  expect(h.load).toHaveBeenCalledWith(expect.objectContaining({...h.entry.context,force:true}))
  expect(h.restartTab).toHaveBeenCalledWith('tab',expect.objectContaining({nativeSessionId:'session',sourceSessionKey:h.key,sourceRootKey:'root'}))
})
it('Restart_UnknownIdentityNeverSilentlyCreatesConversation_002',async()=>{
  const h=fixture();delete h.tab.observedSessionId
  await expect(h.adapter.restartSession('native-tab:tab')).rejects.toThrow('SESSION_NOT_RESUMABLE')
  expect(h.restartTab).not.toHaveBeenCalled()
})
it('Restart_ChangedRootCannotClaimSameUuid_003',async()=>{
  const h=fixture();h.entry.sourceRootKey='other-root'
  await expect(h.adapter.restartSession('native-tab:tab')).rejects.toThrow('SOURCE_CHANGED')
  expect(h.restartTab).not.toHaveBeenCalled()
})
it('Restart_ReplacedOwnerDuringVerificationCannotRestart_004',async()=>{
  const h=fixture();h.load.mockImplementation(async()=>{h.map.set('tab',{...h.tab,generation:2});return h.entry})
  await expect(h.adapter.restartSession('native-tab:tab')).rejects.toThrow('STALE_SESSION_ATTEMPT')
  expect(h.restartTab).not.toHaveBeenCalled()
})
it('Restart_RawKeepsExplicitRawSemantics_005',async()=>{
  const h=fixture();h.tab.action={kind:'raw',argv:['--custom']}
  await h.adapter.restartSession('native-tab:tab')
  expect(h.restartTab).toHaveBeenCalledWith('tab');expect(h.runSource).not.toHaveBeenCalled()
})
it('Restart_ResumedConversationStillChecksFrozenRoot_006',async()=>{
  const h=fixture();h.tab.action={kind:'resume-id',nativeSessionId:'session'};h.tab.sourceSessionKey=h.key
  h.entry.sourceRootKey='replacement-root'
  await expect(h.adapter.restartSession('native-tab:tab')).rejects.toThrow('SOURCE_CHANGED')
  expect(h.restartTab).not.toHaveBeenCalled()
})
