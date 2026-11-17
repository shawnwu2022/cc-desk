import {test,expect,type Page,type BrowserContext} from '@playwright/test'
import {build} from 'esbuild'
import {createECDH} from 'node:crypto'

let helper:string,fixture:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 helper=(await build({stdin:{contents:`export * from './remote/ui-terminal/src/browserRunHandoff.ts';export * from './remote/ui-terminal/src/browserEnrollmentStore.ts';export * from './remote/ui-terminal/src/browserEntry.ts';export * from './remote/ui-terminal/src/browserGrant.ts';`,resolveDir:process.cwd()},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
 const vectors=[1,2,3,4].map(n=>{const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();return{jwk:{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},spki:Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),p]).toString('base64url')}})
 const base={version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw'},localSpki:vectors[0].spki,hostSpki:vectors[3].spki,trust:{hostKeyEpoch:'4',clientKeyEpoch:'5',origin:'wss://relay.invalid/route'},grant:{issuerSpki:vectors[1].spki,principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY_OLD',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw',credentialSpki:vectors[3].spki,backupEligible:false,allowBackedUp:false}},relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},artifacts:{wasm:'https://desk.invalid/fixture.wasm',glue:'https://desk.invalid/fixture.js'}}
 fixture=`export * from '/helper.mjs';export const base=${JSON.stringify(base)};export const encode=value=>new TextEncoder().encode(JSON.stringify(value));export const next=()=>({...base,grant:{...base.grant,scope:{session:'06'.repeat(16),run:'PUBLIC_TEST_ONLY_NEW',generation:'8'}}});export async function material(){const [tlsKey,grantKey,relayKey]=await Promise.all(${JSON.stringify(vectors.slice(0,3).map(v=>v.jwk))}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));return{configuration:encode(base),keys:{tlsKey,grantKey,relayKey}}}`
})
async function route(context:BrowserContext){await context.route('https://desk.invalid/**',r=>{const p=new URL(r.request().url()).pathname;return r.fulfill({headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:p.endsWith('.mjs')?'text/javascript':'text/html',body:p==='/helper.mjs'?helper:p==='/fixture.mjs'?fixture:'<!doctype html><title>TEST_ONLY run handoff</title>'})})}
async function prepare(page:Page){await page.goto('https://desk.invalid/');await page.evaluate(async()=>{
 const api=await import(location.origin+'/fixture.mjs'),controller=new AbortController(),store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'PUBLIC_TEST_ONLY_HANDOFF'})
 const f:any={api,store,controller,opens:0,reads:0,presentations:0,imports:0,signatures:0,sessions:[]};Reflect.set(globalThis,'f',f)
 f.observed={...store,async openExisting(...args:any[]){f.opens++;const session=await Reflect.apply(store.openExisting,store,args);if(session)f.sessions.push(session);return session}}
 f.present=async()=>{f.presentations++}
 f.read=(candidate=api.base)=>api.createStoredBrowserRunProfileReader(api.encode(candidate),f.observed,f.present)
 f.deadline=()=>performance.timeOrigin+performance.now()+10000
 f.raw=async()=>{const db=await new Promise<IDBDatabase>((r,j)=>{const q=indexedDB.open('PUBLIC_TEST_ONLY_HANDOFF',1);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error)});try{return await new Promise<any>((r,j)=>{const tx=db.transaction('enrollment','readonly'),q=tx.objectStore('enrollment').get('current');let value:any;q.onsuccess=()=>{value=q.result};tx.oncomplete=()=>r(value);tx.onabort=()=>j(tx.error)})}finally{db.close()}}
 })}
async function install(page:Page){await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.store.installApproved(undefined,()=>f.api.material(),f.controller.signal)})}
async function finish(page:Page){await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.controller.abort();await Promise.all(f.sessions.map((s:any)=>s.waitClosed()))})}


test('RunHandoff_OriginalStoredKeysAndScopeOnlyFromAuthenticatedChannel_001',async({page,context})=>{
 await route(context);await prepare(page);await install(page);await page.close();const next=await context.newPage();await prepare(next)
 const result=await next.evaluate(async()=>{
  const f=Reflect.get(globalThis,'f'),before=await f.raw(),importKey=crypto.subtle.importKey.bind(crypto.subtle),sign=crypto.subtle.sign.bind(crypto.subtle)
  Object.defineProperty(crypto.subtle,'importKey',{configurable:true,value:(...args:any[])=>{f.imports++;return Reflect.apply(importKey,crypto.subtle,args)}});Object.defineProperty(crypto.subtle,'sign',{configurable:true,value:(...args:any[])=>{f.signatures++;return Reflect.apply(sign,crypto.subtle,args)}})
  f.reader=f.read();const [first,second]=await Promise.all([f.reader(f.controller.signal),f.reader(new AbortController().signal)]),source=await f.sessions[0].provider.readApproved(f.controller.signal),after=await f.raw()
  let exports=0;for(const key of Object.values(after.keys) as CryptoKey[])try{await crypto.subtle.exportKey('jwk',key);exports++}catch{}
  return{sameResult:first===second,sameKeys:first.profile.tlsKey===source.handles.tlsKey&&first.profile.grantKey===source.handles.grantKey,sameRelay:first.relay.identity.sign===source.handles.relaySign,sameLease:first.enrollment===f.sessions[0].provider.enrollment,scopeAbsent:!('scope'in first.grant),policy:first.grant.scopePolicy,persistedRun:JSON.parse(new TextDecoder().decode(after.configuration)).grant.scope.run,unchanged:Array.from(before.configuration).join()===Array.from(after.configuration).join(),originalKeys:Object.values(after.keys).every((key:any)=>key instanceof CryptoKey&&!key.extractable),exports,opens:f.opens,imports:f.imports,signatures:f.signatures,presentations:f.presentations}
 })
 expect(result).toEqual({sameResult:true,sameKeys:true,sameRelay:true,sameLease:true,scopeAbsent:true,policy:'authenticated-host-run',persistedRun:'PUBLIC_TEST_ONLY_OLD',unchanged:true,originalKeys:true,exports:0,opens:1,imports:0,signatures:0,presentations:0});await finish(next)
})

test('RunHandoff_MissingStoreStaysLocked_002',async({page,context})=>{
 await route(context);await prepare(page)
 const result=await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),reader=f.read();return{missing:await reader(f.controller.signal)===undefined,again:await reader(f.controller.signal)===undefined,opens:f.opens}})
 expect(result).toEqual({missing:true,again:true,opens:1})
})

for(const field of ['scope','host','local','issuer','principal','role','relay','epoch','credential','budget','artifacts'])test(`RunHandoff_StoredBindingRejectsChangedCandidate_${field}_003`,async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const result=await page.evaluate(async field=>{
  const f=Reflect.get(globalThis,'f'),candidate=structuredClone(f.api.base)
  if(field==='scope')candidate.grant.scope.run='PUBLIC_TEST_ONLY_INJECTED'
  if(field==='host')candidate.hostSpki=f.api.base.localSpki
  if(field==='local')candidate.localSpki=f.api.base.hostSpki
  if(field==='issuer')candidate.grant.issuerSpki=f.api.base.hostSpki
  if(field==='principal')candidate.grant.principal='09'.repeat(32)
  if(field==='role')candidate.grant.role='Observer'
  if(field==='relay')candidate.relay.keyId=btoa('x'.repeat(16)).replace(/=+$/,'')
  if(field==='epoch')candidate.trust.hostKeyEpoch='9'
  if(field==='credential')candidate.grant.profile.allowBackedUp=true
  if(field==='budget')candidate.grant.budgetMs=9999
  if(field==='artifacts')candidate.artifacts.glue=location.origin+'/other.js'
  const reader=f.read(candidate),errors=[];for(let i=0;i<2;i++)errors.push(await reader(f.controller.signal).then(()=>false,(e:Error)=>String(e)))
  return{errors,opens:f.opens,closed:f.sessions[0].provider.enrollment.signal.aborted,owner:(await f.raw()).owner,stored:JSON.parse(new TextDecoder().decode((await f.raw()).configuration)).grant.scope.run}
 },field)
 expect(result).toEqual({errors:Array(2).fill('Error: browser enrollment store unavailable'),opens:1,closed:true,owner:undefined,stored:'PUBLIC_TEST_ONLY_OLD'})
})

for(const mode of ['both','missing','unknown','undefined-scope','null-scope','inherited','getter'])test(`RunHandoff_StrictExplicitPolicy_${mode}_004`,async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const result=await page.evaluate(async mode=>{
  const f=Reflect.get(globalThis,'f'),inputs=await f.read()(f.controller.signal),grant={...inputs.grant}
  if(mode==='both')grant.scope={session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'}
  if(mode==='missing')delete grant.scopePolicy
  if(mode==='unknown')grant.scopePolicy='accept-any-scope'
  if(mode==='undefined-scope')grant.scope=undefined
  if(mode==='null-scope')grant.scope=null
  if(mode==='inherited'){delete grant.scopePolicy;Object.setPrototypeOf(grant,{scopePolicy:'authenticated-host-run'})}
  let getters=0;if(mode==='getter')Object.defineProperty(grant,'scopePolicy',{enumerable:true,get(){getters++;return 'authenticated-host-run'}})
  let entryRejected=false,brokerRejected=false;try{f.api.approvedBrowserInputs({...inputs,grant})}catch{entryRejected=true}try{const broker=f.api.createBrowserGrantBroker(inputs.profile,{configuration:grant,present:f.present});broker.close();await broker.waitClosed()}catch{brokerRejected=true}
  return{entryRejected,brokerRejected,getters}
 },mode)
 expect(result).toEqual({entryRejected:true,brokerRejected:true,getters:0});await finish(page)
})

test('RunHandoff_PublicCandidateCannotSelectResumeMode_005',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const result=await page.evaluate(()=>{const f=Reflect.get(globalThis,'f'),candidate=structuredClone(f.api.base);delete candidate.grant.scope;candidate.grant.scopePolicy='authenticated-host-run';let rejected=false;try{f.read(candidate)}catch{rejected=true}return{rejected,opens:f.opens}})
 expect(result).toEqual({rejected:true,opens:0})
})

test('RunHandoff_AbortRejectsCachedInputsAndRetainsOriginalRetirement_006',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.reader=f.read();f.inputs=await f.reader(f.controller.signal);f.inputs.enrollment.retain(new Promise<void>(r=>{f.retire=r}));f.controller.abort();f.closed=false;void f.sessions[0].waitClosed().then(()=>{f.closed=true})})
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{closed:f.closed,rejected:await f.reader(new AbortController().signal).then(()=>false,()=>true)}})).toEqual({closed:false,rejected:true})
 const other=await context.newPage();await prepare(other);expect(await other.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return await f.store.openExisting(f.present,f.controller.signal)===undefined})).toBe(true)
 await page.evaluate(()=>Reflect.get(globalThis,'f').retire());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').closed)).toBe(true)
})

test('RunHandoff_ForgetClosesSameLeaseWithoutChangingScope_007',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.reader=f.read();f.inputs=await f.reader(f.controller.signal);f.inputs.enrollment.retain(new Promise<void>(r=>{f.retire=r}))})
 const other=await context.newPage();await prepare(other);await other.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.state='pending';f.promise=f.store.revoke().then(()=>{f.state='revoked'})})
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').inputs.enrollment.signal.aborted)).toBe(true)
 expect(await other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending');await page.evaluate(()=>Reflect.get(globalThis,'f').retire());await expect.poll(()=>other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('revoked')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.reader(new AbortController().signal).then(()=>false,()=>true)})).toBe(true)
})

test('RunHandoff_ExactReaderRemainsExact_008',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const result=await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),reader=f.api.createStoredBrowserProfileReader(f.api.encode(f.api.base),f.observed,f.present),input=await reader(f.controller.signal);return{scope:input.grant.scope,policy:'scopePolicy'in input.grant}})
 expect(result).toEqual({scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY_OLD',generation:'7'},policy:false});await finish(page)
})
