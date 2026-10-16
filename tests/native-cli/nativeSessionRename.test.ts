import { describe, expect, it, vi } from 'vitest'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { NativeHistoryContext, NativeHistoryEntry } from '@/stores/nativeHistory'
import type { SourceRef } from '@/types/nativeProjection'
import type { NativeCliTab } from '@/stores/nativeTabs'

const source: SourceRef = { scopeId:'scope',instanceId:'instance',cli:'codex',sourceRootKey:'root',identityEpoch:'1',profileId:'profile',profileRevision:'1',basis:'configured-profile',target:{kind:'profile',profileId:'profile',expectedProfileRevision:'1',projectIds:['project']} }
function fixture(cli:'claude'|'codex'='codex') {
  const key=JSON.stringify(['local',cli,'root','session'])
  const entry:NativeHistoryEntry={key:'entry',context:{cli,profileId:'profile',profileRevision:'1',projectId:'project',projectPath:'/repo'},sessions:[{type:'session',sessionKey:key,nativeSessionId:'session',title:'Old title',titleSource:'custom',truncated:false,archived:false,cwd:'/repo',updatedAt:null}],loading:false,loaded:true,error:null,requestEpoch:'1',sourceRootKey:'root',source:{...source,cli}}
  const records=new Map(),write=vi.fn(async(id,record,before)=>{before?.();records.set(id,record)})
  const entries=[entry]
  const rename=vi.fn(async request=>({...request,state:'renamed' as const})),load=vi.fn(async(context:NativeHistoryContext)=>entries.find(item=>item.context.profileId===context.profileId)??entry)
  const tabs=new Map<string,NativeCliTab>(),runSource=vi.fn(async(tab:NativeCliTab)=>({...source,cli,target:{kind:'run' as const,runId:tab.runId,generation:tab.generation},basis:'launch-environment' as const}))
  const runtime={createTab:vi.fn(),restartTab:vi.fn(),stopTab:vi.fn(),renameTab:vi.fn()}
  const legacyRoot=vi.fn(async():Promise<string|undefined>=>undefined)
  const adapter=createNativeCliAdapter({tabs:{tabs,activeTabId:null,setActive:vi.fn(),close:vi.fn(),rename:vi.fn()},history:{all:()=>entries,load},runtime,archive:{getArchivedSessions:()=>[],archiveSession:vi.fn(),restoreSession:vi.fn()},metadata:{sessionRecords:records,upsertSessionRecord:write},nativeRename:{rename},runSource,legacyClaudeRootKey:legacyRoot})
  return {adapter,entry,entries,rename,load,write,records,tabs,runSource,runtime,legacyRoot}
}
function resumed(cli:'claude'|'codex'='codex') {
  const h=fixture(cli)
  const tab:NativeCliTab={tabId:'tab',cli,projectId:'project',projectPath:'/repo',profileId:'profile',profileRevision:'1',requestId:'request',runId:'run',generation:1,action:{kind:'resume-id',nativeSessionId:'session'},sourceSessionKey:h.entry.sessions[0].sessionKey,status:'running',errorCode:null,launchRevision:'1',title:'Old title',createdAt:1,lastActivityAt:1}
  h.tabs.set('tab',tab)
  return {...h,tab,id:'native-tab:tab'}
}
describe('Native session rename admission',()=>{
  it('Rename_CodexHistoryUsesNativeReceiptThenReload_001',async()=>{
    const h=fixture(),row=(await h.adapter.listSessions())[0]
    expect(await h.adapter.renameSession(row.id,' New native title ')).toBe('native-title-saved')
    expect(h.rename).toHaveBeenCalledWith(expect.objectContaining({source:h.entry.source,sessionKey:h.entry.sessions[0].sessionKey,title:'New native title'}))
    expect(h.load).toHaveBeenCalledWith(expect.objectContaining({...h.entry.context,force:true}))
    expect(h.write).not.toHaveBeenCalled()
  })
  it('Rename_ClaudeHistoryUsesOfficialNativeReceipt_002',async()=>{
    const h=fixture('claude'),row=(await h.adapter.listSessions())[0]
    await expect(h.adapter.renameSession(row.id,'Title')).resolves.toBe('native-title-saved')
    expect(h.rename).toHaveBeenCalledWith(expect.objectContaining({source:h.entry.source,sessionKey:h.entry.sessions[0].sessionKey,title:'Title'}))
    expect(h.write).not.toHaveBeenCalled()
  })
  it('Rename_FailedNativeWriteKeepsExistingOverride_003',async()=>{
    const h=fixture(),row=(await h.adapter.listSessions())[0]
    h.records.set(row.id,{runtime:row.runtime,cli:row.cli,projectPath:row.projectPath,adapterSessionId:row.adapterSessionId,nativeSessionId:row.nativeSessionId,title:'Old override',lastActivityAt:0})
    h.rename.mockRejectedValue(new Error('RENAME_OUTCOME_UNKNOWN'))
    await expect(h.adapter.renameSession(row.id,'Title')).rejects.toThrow('RENAME_OUTCOME_UNKNOWN')
    expect(h.records.get(row.id).title).toBe('Old override');expect(h.write).not.toHaveBeenCalled();expect(h.load).not.toHaveBeenCalled()
  })
  it('Rename_RevokedBeforeDispatchNeverWrites_004',async()=>{
    const h=fixture(),row=(await h.adapter.listSessions())[0]
    await expect(h.adapter.renameSession(row.id,'Title',()=>false)).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(h.rename).not.toHaveBeenCalled();expect(h.write).not.toHaveBeenCalled()
  })
  it('Rename_ResumeSourceUsesFreshProfileAuthority_005',async()=>{
    const h=resumed(),row=(await h.adapter.listSessions()).find(row=>row.opened)!
    expect(row.renameTarget).toBe('resume-source')
    h.tab.observedSessionId='another-current-terminal-session'
    expect(await h.adapter.renameSession(h.id,'Source title')).toBe('native-title-saved')
    expect(h.runSource).toHaveBeenCalled()
    expect(h.load).toHaveBeenCalledTimes(2)
    expect(h.load.mock.calls[0][0]).toEqual({...h.entry.context,force:true})
    expect(h.rename).toHaveBeenCalledWith(expect.objectContaining({source:h.entry.source,sessionKey:h.tab.sourceSessionKey,title:'Source title'}))
    expect(h.runtime.renameTab).not.toHaveBeenCalled();expect(h.runtime.stopTab).not.toHaveBeenCalled()
  })
  it.each(['root','revision','project','key','id','duplicate','run-owner'] as const)('Rename_ResumeSourceChangeCannotWrite_006 %s',async change=>{
    const h=resumed()
    h.load.mockImplementation(async()=>{
      if(change==='root') h.entry.sourceRootKey='other-root'
      if(change==='revision') h.entry.context={...h.entry.context,profileRevision:'2'}
      if(change==='project') h.entry.context={...h.entry.context,projectId:'another-project'}
      if(change==='key') h.entry.sessions=[{...h.entry.sessions[0],sessionKey:'other-key'}]
      if(change==='id') h.entry.sessions=[{...h.entry.sessions[0],nativeSessionId:'other-id'}]
      if(change==='duplicate') h.entry.sessions.push({...h.entry.sessions[0]})
      if(change==='run-owner') h.tabs.set('tab',{...h.tab,generation:2})
      return h.entry
    })
    await expect(h.adapter.renameSession(h.id,'No write')).rejects.toThrow()
    expect(h.load).toHaveBeenCalled();expect(h.rename).not.toHaveBeenCalled();expect(h.write).not.toHaveBeenCalled()
  })
  it.each(['new','raw','resume-picker'] as const)('Rename_UnknownSourceNeverGuesses_007 %s',async kind=>{
    const h=resumed();h.tab.action=kind==='raw'?{kind,argv:[]}:kind==='resume-picker'?{kind,scope:'current-project'}:{kind}
    h.tab.observedSessionId='session'
    await expect(h.adapter.renameSession(h.id,'No write')).rejects.toThrow('SESSION_IDENTITY_UNKNOWN')
    expect(h.load).not.toHaveBeenCalled();expect(h.rename).not.toHaveBeenCalled()
  })
  it('Rename_ResumeFailurePreservesAllOldAliases_008',async()=>{
    const h=resumed(),historyId=(await fixture().adapter.listSessions())[0].id
    const identity={runtime:'native-cli',cli:'codex',projectPath:'/repo',lastActivityAt:1}
    h.records.set(historyId,{...identity,adapterSessionId:h.tab.sourceSessionKey,nativeSessionId:'session',title:'History alias'})
    h.records.set(h.id,{...identity,adapterSessionId:'tab',nativeSessionId:'session',title:'Tab alias'})
    h.rename.mockRejectedValue(new Error('RENAME_OUTCOME_UNKNOWN'))
    await expect(h.adapter.renameSession(h.id,'New title')).rejects.toThrow('RENAME_OUTCOME_UNKNOWN')
    expect(h.records.get(historyId).title).toBe('History alias');expect(h.records.get(h.id).title).toBe('Tab alias');expect(h.write).not.toHaveBeenCalled()
  })
  it('Rename_ResumeSuccessClearsAllMatchingAliasesOnly_009',async()=>{
    const h=resumed(),historyId=(await fixture().adapter.listSessions())[0].id
    const identity={runtime:'native-cli',cli:'codex',projectPath:'/repo',lastActivityAt:1,lastOpenedAt:7}
    h.records.set(historyId,{...identity,adapterSessionId:h.tab.sourceSessionKey,nativeSessionId:'session',title:'History alias'})
    h.records.set(h.id,{...identity,adapterSessionId:'tab',nativeSessionId:'session',title:'Tab alias'})
    h.rename.mockImplementation(async request=>{h.entry.sessions[0].title=request.title;return {...request,state:'renamed' as const}})
    await h.adapter.renameSession(h.id,'Native title')
    expect(h.records.get(historyId)).toMatchObject({title:'',lastOpenedAt:7})
    expect(h.records.get(h.id)).toMatchObject({title:'',lastOpenedAt:7})
    expect((await h.adapter.listSessions()).find(row=>row.opened)?.title).toBe('Native title')
  })
  it('Rename_EarlyTabAliasClearsAfterNativeSuccess_010',async()=>{
    const h=resumed()
    h.records.set(h.id,{runtime:'native-cli',cli:'codex',projectPath:'/repo',adapterSessionId:'tab',nativeSessionId:null,title:'Early alias',lastActivityAt:1})
    h.records.set('unrelated',{runtime:'native-cli',cli:'codex',projectPath:'/other',adapterSessionId:'tab',nativeSessionId:'session',title:'Unrelated',lastActivityAt:1})
    await h.adapter.renameSession(h.id,'Native title')
    expect(h.records.get(h.id).title).toBe('');expect(h.records.get('unrelated').title).toBe('Unrelated')
  })
  it.each(['profile','revision','root','run-scope','project-list'] as const)('Rename_FreshSourceMismatchRejects_011 %s',async change=>{
    const h=resumed()
    h.entry.source={...source,target:{...source.target} as SourceRef['target']}
    if(change==='profile') h.entry.source.profileId='other-profile'
    if(change==='revision') h.entry.source.profileRevision='2'
    if(change==='root') h.entry.source.sourceRootKey='other-root'
    if(change==='run-scope') h.entry.source.target={kind:'run',runId:'run',generation:1}
    if(change==='project-list') h.entry.source.target={kind:'profile',profileId:'profile',expectedProfileRevision:'1',projectIds:['other-project']}
    await expect(h.adapter.renameSession(h.id,'No write')).rejects.toThrow('SOURCE_CHANGED')
    expect(h.rename).not.toHaveBeenCalled();expect(h.write).not.toHaveBeenCalled()
  })
  it('Rename_CancelDuringFreshReadCannotIssue_012',async()=>{
    const h=resumed(),controller=new AbortController(),issued=vi.fn()
    h.load.mockImplementation(async()=>{controller.abort();return h.entry})
    await expect(h.adapter.renameSession(h.id,'No write',()=>true,issued,controller.signal)).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(issued).not.toHaveBeenCalled();expect(h.rename).not.toHaveBeenCalled()
  })
  it('Rename_ReplacedAfterNativeReceiptCannotClearAliases_013',async()=>{
    const h=resumed()
    h.records.set(h.id,{runtime:'native-cli',cli:'codex',projectPath:'/repo',adapterSessionId:'tab',nativeSessionId:'session',title:'Old alias',lastActivityAt:1})
    h.rename.mockImplementation(async request=>{h.tabs.set('tab',{...h.tab,generation:2});return {...request,state:'renamed' as const}})
    await expect(h.adapter.renameSession(h.id,'Issued title')).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(h.rename).toHaveBeenCalledTimes(1);expect(h.write).not.toHaveBeenCalled();expect(h.records.get(h.id).title).toBe('Old alias')
  })
  it('Rename_BackgroundRefreshUsesReturnedFreshAuthority_014',async()=>{
    const h=resumed(),fresh={...h.entry,source:{...source,scopeId:'new-profile-scope'}}
    h.load.mockResolvedValue(fresh)
    await h.adapter.renameSession(h.id,'Source title')
    expect(h.rename).toHaveBeenCalledWith(expect.objectContaining({source:fresh.source}))
    expect(h.rename).toHaveBeenCalledTimes(1)
  })
  it('Rename_PostReceiptRefreshFailureNeverReplays_015',async()=>{
    const h=resumed()
    h.load.mockResolvedValueOnce(h.entry).mockResolvedValueOnce({...h.entry,error:'SOURCE_READ_FAILED'})
    await expect(h.adapter.renameSession(h.id,'Already native')).rejects.toThrow('RENAME_DISPLAY_SYNC_FAILED')
    expect(h.rename).toHaveBeenCalledTimes(1);expect(h.load).toHaveBeenCalledTimes(2)
  })
  it.each(['resume-id','new'] as const)('Rename_ClaudeVerifiedRecordNeverWritesTerminal_016 %s',async kind=>{
    const h=resumed('claude')
    if(kind==='new'){h.tab.action={kind:'new'};delete h.tab.sourceSessionKey;h.tab.observedSessionId='session'}
    await expect(h.adapter.renameSession(h.id,'Claude native')).resolves.toBe('native-title-saved')
    expect(h.rename).toHaveBeenCalledWith(expect.objectContaining({source:h.entry.source,sessionKey:h.entry.sessions[0].sessionKey,title:'Claude native'}))
    expect(h.runtime.renameTab).not.toHaveBeenCalled();expect(h.runtime.stopTab).not.toHaveBeenCalled()
  })
  it('Rename_ClaudeChangedObservedIdCannotIssue_017',async()=>{
    const h=resumed('claude');h.tab.action={kind:'new'};delete h.tab.sourceSessionKey;h.tab.observedSessionId='session'
    h.load.mockImplementation(async()=>{h.tab.observedSessionId='another-session';return h.entry})
    await expect(h.adapter.renameSession(h.id,'No write')).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(h.rename).not.toHaveBeenCalled();expect(h.write).not.toHaveBeenCalled()
  })
  it.each(['active','history'] as const)('Rename_AllProvenOpenedAliasesClearAfterSuccess_018 %s',async origin=>{
    const h=resumed(),other={...h.tab,tabId:'other',requestId:'other-request',runId:'other-run'}
    h.tabs.set(other.tabId,other)
    const row=(await h.adapter.listSessions()).find(row=>row.id==='native-tab:other')!
    h.records.set(row.id,{runtime:row.runtime,cli:row.cli,projectPath:row.projectPath,adapterSessionId:row.adapterSessionId,nativeSessionId:row.nativeSessionId,title:'Other old alias',lastActivityAt:1})
    const id=origin==='active'?h.id:(await fixture().adapter.listSessions())[0].id
    await h.adapter.renameSession(id,'Native name')
    expect(h.records.get(row.id).title).toBe('')
  })
  it('Rename_AllProvenProfileCachesRefresh_019',async()=>{
    const h=resumed()
    const other:NativeHistoryEntry={...h.entry,key:'other-entry',context:{...h.entry.context,profileId:'other-profile'},source:{...source,profileId:'other-profile',target:{kind:'profile',profileId:'other-profile',expectedProfileRevision:'1',projectIds:['project']}},sessions:[{...h.entry.sessions[0]}]}
    h.entries.push(other)
    h.tabs.set('other',{...h.tab,tabId:'other',profileId:'other-profile',requestId:'other-request',runId:'other-run'})
    let nativeTitle='Old title'
    h.rename.mockImplementation(async request=>{nativeTitle=request.title;return {...request,state:'renamed' as const}})
    h.load.mockImplementation(async context=>{const entry=h.entries.find(item=>item.context.profileId===context.profileId)!;entry.sessions[0].title=nativeTitle;return entry})
    await h.adapter.renameSession(h.id,'Native new title')
    h.tabs.delete('tab')
    expect((await h.adapter.listSessions()).find(row=>row.opened)?.title).toBe('Native new title')
    expect(h.load).toHaveBeenCalledWith(expect.objectContaining({profileId:'other-profile',force:true}))
  })
  it('Rename_CompanionSwitchDoesNotClearItsEarlyAlias_020',async()=>{
    const h=resumed('claude'),other={...h.tab,tabId:'other',requestId:'other-request',runId:'other-run',action:{kind:'new' as const},sourceSessionKey:undefined,observedSessionId:'session'}
    h.tabs.set('other',other)
    h.records.set('native-tab:other',{runtime:'native-cli',cli:'claude',projectPath:'/repo',adapterSessionId:'other',nativeSessionId:null,title:'Other session alias',lastActivityAt:1})
    h.rename.mockImplementation(async request=>{other.observedSessionId='other-session';return {...request,state:'renamed' as const}})
    await h.adapter.renameSession(h.id,'Target native')
    expect(h.records.get('native-tab:other').title).toBe('Other session alias')
  })
  it.each([true,false])('Rename_LegacyAliasOnlyClearsWithSameRootProof_021 %s',async proven=>{
    const h=fixture('claude'),row=(await h.adapter.listSessions())[0],id='legacy-history:/repo:session'
    h.records.set(id,{runtime:'legacy-claude',cli:'claude',projectPath:'/repo',adapterSessionId:'session',nativeSessionId:'session',title:'Legacy override',lastActivityAt:1})
    if(proven) h.legacyRoot.mockResolvedValue('root')
    await h.adapter.renameSession(row.id,'Native title')
    expect(h.records.get(id).title).toBe(proven?'':'Legacy override')
  })
})
