import { test } from 'node:test'
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { CCDeskObserver } from '../../src-tauri/src/opencode_observer.mjs'

async function fixture(run) {
  const keys=['CC_BOX_HOOK_PORT','CC_DESK_OBSERVER_RUN','CC_DESK_OBSERVER_GENERATION','CC_DESK_OBSERVER_CAPABILITY']
  const old=keys.map(k=>process.env[k]), previousFetch=globalThis.fetch, sent=[]
  Object.assign(process.env,{CC_BOX_HOOK_PORT:'12345',CC_DESK_OBSERVER_RUN:randomUUID(),CC_DESK_OBSERVER_GENERATION:'1',CC_DESK_OBSERVER_CAPABILITY:'a'.repeat(32)})
  globalThis.fetch=async(url,request)=>{ assert.equal(url,'http://127.0.0.1:12345/observer');sent.push({headers:request.headers,body:JSON.parse(request.body)});return {ok:true} }
  try { await run(sent) } finally {await new Promise(resolve=>setTimeout(resolve,10));globalThis.fetch=previousFetch;keys.forEach((key,i)=>{if(old[i]===undefined)delete process.env[key];else process.env[key]=old[i]})}
}
const metadata=id=>({id,directory:'/fixture',parentID:id==='ses_child'?'ses_main':undefined})
const client={session:{get:async request=>({data:metadata(request.path.id)})}}
const emit=(plugin,type,properties,id=randomUUID())=>plugin.event({event:{id,type,properties}})

test('Opencode_PluginPreservesStatusAndPendingRequestSemantics_001',()=>fixture(async sent=>{
  const plugin=await CCDeskObserver({client,directory:'/fixture'},{sessionID:'ses_main',directory:'/fixture'})
  await emit(plugin,'session.updated',{info:metadata('ses_main')})
  assert.equal(sent[0].body.hook_event_name,'SessionStart')
  const status=()=>sent.at(-1).body.notification_type
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'},secret:'never'})
  assert.equal(status(),'opencode_busy')
  await emit(plugin,'permission.asked',{sessionID:'ses_main',id:'one',permission:'secret'})
  await emit(plugin,'permission.asked',{sessionID:'ses_main',id:'two'})
  await emit(plugin,'permission.replied',{sessionID:'ses_main',requestID:'one'})
  assert.equal(status(),'opencode_permission')
  await emit(plugin,'permission.replied',{sessionID:'ses_main',requestID:'two'})
  assert.equal(status(),'opencode_busy')
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'idle'}})
  await emit(plugin,'permission.asked',{sessionID:'ses_main',id:'one'})
  assert.equal(status(),'opencode_idle')
  await emit(plugin,'question.asked',{sessionID:'ses_main',id:'q',questions:[{text:'never'}]})
  assert.equal(status(),'opencode_question')
  await emit(plugin,'question.rejected',{sessionID:'ses_main',requestID:'q'})
  assert.equal(status(),'opencode_idle')
  await emit(plugin,'session.error',{sessionID:'ses_main',error:{message:'private'}})
  assert.equal(status(),'opencode_error')
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'idle'}})
  assert.equal(status(),'opencode_error')
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'retry',message:'private',next:1,attempt:1}})
  assert.equal(status(),'opencode_retry')
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'future'}})
  assert.equal(status(),'opencode_unknown')
  assert(!JSON.stringify(sent).includes('private'))
  assert(!sent.some(x=>x.body.hook_event_name==='Stop'))
}))

test('Opencode_PluginBindsOnlyExactMainAndHandlesDelayedIdentity_002',()=>fixture(async sent=>{
  const plugin=await CCDeskObserver({client,directory:'/fixture'},{directory:'/fixture'})
  await emit(plugin,'session.created',{info:metadata('ses_child')})
  assert.equal(sent.length,0)
  await emit(plugin,'session.created',{info:{id:'ses_foreign',directory:'/other'}})
  assert.equal(sent.length,0)
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}})
  assert.equal(sent[0].body.session_id,'ses_main')
  assert.equal(sent.at(-1).body.notification_type,'opencode_busy')
  const count=sent.length
  await emit(plugin,'session.status',{sessionID:'ses_child',status:{type:'idle'}})
  await emit(plugin,'session.status',{sessionID:'ses_other',status:{type:'idle'}})
  await emit(plugin,'session.error',{error:{message:'no identity'}})
  assert.equal(sent.length,count)
}))

test('Opencode_PluginSerializesAndDeduplicatesWithoutInventingCompletion_003',()=>fixture(async sent=>{
  const plugin=await CCDeskObserver({client,directory:'/fixture'},{sessionID:'ses_main',directory:'/fixture'})
  const first=emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}},'duplicate')
  const second=emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'idle'}},'second')
  await Promise.all([first,second])
  const count=sent.length
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}},'duplicate')
  assert.equal(sent.length,count)
  assert.deepEqual(sent.slice(-2).map(x=>x.body.notification_type),['opencode_busy','opencode_idle'])
  assert.deepEqual(sent.map(x=>x.headers['x-cc-desk-event']),sent.map((_,i)=>`o${String(i+1).padStart(16,'0')}`))
}))

test('Opencode_PluginOptionalFailuresDoNotThrowOrGuessIdentity_004',()=>fixture(async sent=>{
  const plugin=await CCDeskObserver({client:{session:{get:async()=>{throw Error('unsupported')}}},directory:'/fixture'},{sessionID:'ses_main',directory:'/fixture'})
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}})
  assert.equal(sent.length,0)
  const invalid=await CCDeskObserver({client,directory:'/fixture'},{sessionID:'../../other'})
  assert.deepEqual(invalid,{})
}))

test('Opencode_PluginIdleRetiresCanceledRequests_005',()=>fixture(async sent=>{
  const plugin=await CCDeskObserver({client,directory:'/fixture'},{sessionID:'ses_main',directory:'/fixture'})
  await emit(plugin,'permission.asked',{sessionID:'ses_main',id:'canceled'})
  await emit(plugin,'question.asked',{sessionID:'ses_main',id:'canceled-question'})
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'idle'}})
  assert.equal(sent.at(-1).body.notification_type,'opencode_idle')
  await emit(plugin,'permission.asked',{sessionID:'ses_main',id:'canceled'})
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}})
  assert.equal(sent.at(-1).body.notification_type,'opencode_busy')
}))

test('Opencode_PluginDisposeDropsPendingIdentityAndFutureEvents_006',()=>fixture(async sent=>{
  let resolve
  const get=new Promise(done=>{resolve=done})
  const plugin=await CCDeskObserver({client:{session:{get:()=>get}},directory:'/fixture'},{directory:'/fixture'})
  const pending=emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}})
  await Promise.resolve()
  assert.equal(typeof plugin.dispose,'function')
  plugin.dispose()
  resolve({data:metadata('ses_main')})
  await pending
  await emit(plugin,'session.created',{info:metadata('ses_main')})
  assert.equal(sent.length,0)
}))

test('Opencode_PluginIgnoresTokenFloodBeforeQueueingAndHaltsAtCapacity_007',()=>fixture(async sent=>{
  const plugin=await CCDeskObserver({client,directory:'/fixture'},{sessionID:'ses_main',directory:'/fixture'})
  await Promise.all(Array.from({length:10000},(_,i)=>emit(plugin,'message.part.delta',{sessionID:'ses_main',text:'private'},String(i))))
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}})
  assert.equal(sent.at(-1).body.notification_type,'opencode_busy')
  for(let i=0;i<129;i++)await emit(plugin,'permission.asked',{sessionID:'ses_main',id:String(i)})
  assert.equal(sent.at(-1).body.notification_type,'opencode_unknown')
  const count=sent.length
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'idle'}})
  assert.equal(sent.length,count)
}))

test('Opencode_PluginReplayCapacityCannotEvictAndResurrectOldEvents_008',()=>fixture(async sent=>{
  const plugin=await CCDeskObserver({client,directory:'/fixture'},{sessionID:'ses_main',directory:'/fixture'})
  for(let i=0;i<4097;i++)await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'busy'}},String(i))
  assert.equal(sent.at(-1).body.notification_type,'opencode_unknown')
  const count=sent.length
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'idle'}},'0')
  await emit(plugin,'session.status',{sessionID:'ses_main',status:{type:'idle'}},'fresh')
  assert.equal(sent.length,count)
}))

test('Opencode_PluginOverflowCannotPublishAnUnverifiedRequestedId_009',()=>fixture(async sent=>{
  const unavailable={session:{get:async()=>{throw Error('unavailable')}}}
  const plugin=await CCDeskObserver({client:unavailable,directory:'/fixture'},{sessionID:'ses_unverified',directory:'/fixture'})
  await Promise.all(Array.from({length:129},()=>emit(plugin,'session.status',{sessionID:'ses_unverified',status:{type:'busy'}})))
  assert.equal(sent.length,0)
}))
