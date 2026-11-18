import {test,expect,type Page,type BrowserContext} from '@playwright/test'
import {build} from 'esbuild'
import {createECDH} from 'node:crypto'
import {readFile} from 'node:fs/promises'
import {resolve} from 'node:path'

let helper:string,fixture:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 helper=(await build({stdin:{contents:`export * from './remote/ui-terminal/src/browserProductStartup.ts';export * from './remote/ui-terminal/src/browserEnrollmentStore.ts';export * from './remote/ui-terminal/src/browserEntry.ts';export * from './remote/ui-terminal/src/browserGrant.ts';export * from './remote/ui-terminal/src/browserSource.ts';`,resolveDir:process.cwd()},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
 const vectors=[1,2,3,4].map(n=>{const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();return{jwk:{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},spki:Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),p]).toString('base64url')}})
 const base={version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw'},localSpki:vectors[0].spki,hostSpki:vectors[3].spki,trust:{hostKeyEpoch:'4',clientKeyEpoch:'5',origin:'wss://relay.invalid/route'},grant:{issuerSpki:vectors[1].spki,principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY_OLD',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw',credentialSpki:vectors[3].spki,backupEligible:false,allowBackedUp:false}},relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},artifacts:{wasm:'https://desk.invalid/fixture.wasm',glue:'https://desk.invalid/fixture.js'}}
 fixture=`export * from '/helper.mjs';export const base=${JSON.stringify(base)};export const encode=value=>new TextEncoder().encode(JSON.stringify(value));export const next=()=>({...base,grant:{...base.grant,scope:{session:'06'.repeat(16),run:'PUBLIC_TEST_ONLY_NEW',generation:'8'}}});export async function material(){const [tlsKey,grantKey,relayKey]=await Promise.all(${JSON.stringify(vectors.slice(0,3).map(v=>v.jwk))}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));return{configuration:encode(base),keys:{tlsKey,grantKey,relayKey}}}`
})
async function route(context:BrowserContext){await context.route('https://desk.invalid/**',async r=>{const p=new URL(r.request().url()).pathname;return r.fulfill({headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:/\.(mjs|js)$/.test(p)?'text/javascript':'text/html',body:p==='/helper.mjs'?helper:p==='/fixture.mjs'?fixture:p==='/browser-profile.js'||p.startsWith('/assets/')?await readFile(resolve('dist','.'+p)):'<!doctype html><title>TEST_ONLY run handoff</title>'})})}
async function prepare(page:Page){await page.goto('https://desk.invalid/?databaseName=PUBLIC_TEST_ONLY_URL#profile=PUBLIC_TEST_ONLY_URL');await page.evaluate(async()=>{
 const api=await import(location.origin+'/fixture.mjs'),controller=new AbortController(),store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'CC_DESK_BROWSER_ENROLLMENT_V1'})
 const f:any={api,store,controller,opens:0,reads:0,presentations:0,imports:0,signatures:0,sessions:[]};Reflect.set(globalThis,'f',f)
 f.observed={...store,async openExisting(...args:any[]){f.opens++;const session:any=await Reflect.apply(store.openExisting,store,args);if(session){f.sessions.push(session);return{...session,provider:{enrollment:session.provider.enrollment,readApproved:async(signal:AbortSignal)=>{f.reads++;return session.provider.readApproved(signal)}}}}}}
 f.present=async()=>{f.presentations++}
 f.read=()=>api.createInstalledBrowserRunProfileReader(f.observed,f.present)
 f.deadline=()=>performance.timeOrigin+performance.now()+10000
 f.raw=async()=>{const db=await new Promise<IDBDatabase>((r,j)=>{const q=indexedDB.open('CC_DESK_BROWSER_ENROLLMENT_V1',1);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error)});try{return await new Promise<any>((r,j)=>{const tx=db.transaction('enrollment','readonly'),q=tx.objectStore('enrollment').get('current');let value:any;q.onsuccess=()=>{value=q.result};tx.oncomplete=()=>r(value);tx.onabort=()=>j(tx.error)})}finally{db.close()}}
 })}
async function install(page:Page){await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.store.installApproved(undefined,()=>f.api.material(),f.controller.signal)})}
async function finish(page:Page){await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.controller.abort();await Promise.all(f.sessions.map((s:any)=>s.waitClosed()))})}


async function productRead(page:Page){return page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),api=await import(location.origin+'/browser-profile.js');return api.readApprovedBrowserInputs(f.controller.signal).then((v:any)=>v?'ready':'locked',()=> 'locked')})}
async function corrupt(page:Page,kind:string){await page.evaluate(async kind=>{const db=await new Promise<IDBDatabase>((r,j)=>{const q=indexedDB.open('CC_DESK_BROWSER_ENROLLMENT_V1',1);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error)});try{await new Promise<void>((r,j)=>{const tx=db.transaction('enrollment','readwrite'),store=tx.objectStore('enrollment'),q=store.get('current');q.onsuccess=()=>{const value=q.result;if(kind==='owner')value.owner='01'.repeat(16);else{const config=JSON.parse(new TextDecoder().decode(value.configuration));config.privateKey='PUBLIC_TEST_ONLY_FORBIDDEN';value.configuration=new TextEncoder().encode(JSON.stringify(config))}store.put(value,'current')};tx.oncomplete=()=>r();tx.onabort=()=>j(tx.error)})}finally{db.close()}},kind)}

test('ProductStartup_BuiltModuleImportHasSoleExportAndNoEffects_001',async({page,context})=>{
 await route(context);await prepare(page)
 const result=await page.evaluate(async()=>{
  let opens=0,imports=0,signs=0,fetches=0,sockets=0,presentations=0,credentials=0
  const deny=()=>{throw Error('unexpected effect')}
  Object.defineProperty(indexedDB,'open',{value:()=>{opens++;return deny()}})
  Object.defineProperty(crypto.subtle,'importKey',{value:()=>{imports++;return deny()}});Object.defineProperty(crypto.subtle,'sign',{value:()=>{signs++;return deny()}})
  globalThis.fetch=(()=>{fetches++;return deny()}) as any;globalThis.WebSocket=class {constructor(){sockets++;deny()}} as any
  HTMLDialogElement.prototype.showModal=()=>{presentations++;deny()};Object.defineProperty(navigator.credentials,'create',{value:()=>{credentials++;return deny()}})
  const api=await import(location.origin+'/browser-profile.js');return{exports:Object.keys(api),opens,imports,signs,fetches,sockets,presentations,credentials}
 })
 expect(result).toEqual({exports:['readApprovedBrowserInputs'],opens:0,imports:0,signs:0,fetches:0,sockets:0,presentations:0,credentials:0})
})

test('ProductStartup_ReopensOriginalRecordHandlesLeaseWithoutSigning_002',async({page,context})=>{
 await route(context);await prepare(page);await install(page);await page.close();const next=await context.newPage();await prepare(next)
 const result=await next.evaluate(async()=>{
  const f=Reflect.get(globalThis,'f'),before=await f.raw();let imports=0,signatures=0
  Object.defineProperty(crypto.subtle,'importKey',{value:()=>{imports++;throw Error('unexpected import')}});Object.defineProperty(crypto.subtle,'sign',{value:()=>{signatures++;throw Error('unexpected signing')}})
  const read=f.read(),first=await read(f.controller.signal),second=await read(new AbortController().signal),session=f.sessions[0],source=await session.provider.readApproved(f.controller.signal),after=await f.raw()
  return{same:first===second,keys:first.profile.tlsKey===source.handles.tlsKey&&first.profile.grantKey===source.handles.grantKey,relay:first.relay.identity.sign===source.handles.relaySign,lease:first.enrollment===session.provider.enrollment,policy:first.grant.scopePolicy,scopeAbsent:!('scope'in first.grant),bytesUnchanged:Array.from(before.configuration).join()===Array.from(after.configuration).join(),revision:after.revision,allNonExportable:Object.values(after.keys).every((k:any)=>!k.extractable),opens:f.opens,reads:f.reads,imports,signatures}
 })
 expect(result).toEqual({same:true,keys:true,relay:true,lease:true,policy:'authenticated-host-run',scopeAbsent:true,bytesUnchanged:true,revision:'1',allNonExportable:true,opens:1,reads:1,imports:0,signatures:0});await finish(next)
})

for(const state of ['absent','revoked','owner','invalid'])test(`ProductStartup_UnavailableRecordLocked_${state}_003`,async({page,context})=>{
 await route(context);await prepare(page);if(state!=='absent')await install(page)
 if(state==='revoked')await page.evaluate(()=>Reflect.get(globalThis,'f').store.revoke())
 if(state==='owner'||state==='invalid')await corrupt(page,state)
 expect(await productRead(page)).toBe('locked');expect(await productRead(page)).toBe('locked')
})

test('ProductStartup_FixedDatabaseCannotBeSelectedByUrl_004',async({page,context})=>{
 await route(context);await prepare(page)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');const store=f.api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'PUBLIC_TEST_ONLY_URL'});await store.installApproved(undefined,()=>f.api.material(),f.controller.signal)})
 expect(await productRead(page)).toBe('locked')
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').api.BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME)).toBe('CC_DESK_BROWSER_ENROLLMENT_V1')
})

test('ProductStartup_OriginalOwnerCompetitionAndRevoke_005',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.reader=f.read();await f.reader(f.controller.signal)})
 const other=await context.newPage();await prepare(other);expect(await productRead(other)).toBe('locked')
 await other.evaluate(()=>Reflect.get(globalThis,'f').store.revoke())
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.reader(new AbortController().signal).then(()=>false,()=>true)})).toBe(true)
 const fresh=await context.newPage();await prepare(fresh);expect(await productRead(fresh)).toBe('locked')
})

for(const abort of ['current','original'])test(`ProductStartup_CachedCancellationJoinsOriginalRetirement_${abort}_006`,async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async abort=>{const f=Reflect.get(globalThis,'f');f.reader=f.read();await f.reader(f.controller.signal);f.sessions[0].provider.enrollment.retain(new Promise<void>(r=>{f.release=r}));const current=new AbortController();(abort==='original'?f.controller:current).abort();f.state='pending';f.promise=f.reader(current.signal).then(()=>{f.state='ready'},()=>{f.state='rejected'})},abort)
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').sessions[0].provider.enrollment.signal.aborted)).toBe(true)
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 await page.evaluate(()=>Reflect.get(globalThis,'f').release());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>Reflect.get(globalThis,'f').raw().then((v:any)=>v.owner))).toBeUndefined()
})

test('ProductStartup_CancelledLateSupplierCannotPublish_007',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f'),open=f.observed.openExisting;f.observed.openExisting=async(...args:any[])=>{const s=await open(...args);const read=s.provider.readApproved;return{...s,provider:{enrollment:s.provider.enrollment,readApproved:async(signal:AbortSignal)=>{const value=await read(signal);return new Promise(r=>{f.release=()=>r(value)})}}}};f.reader=f.read();f.state='pending';f.promise=f.reader(f.controller.signal).then(()=>{f.state='ready'},()=>{f.state='rejected'})})
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').release)).toBe('function')
 await page.evaluate(()=>Reflect.get(globalThis,'f').controller.abort());expect(await page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 await page.evaluate(()=>Reflect.get(globalThis,'f').release());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').opens)).toBe(1)
})

test('ProductStartup_FormalModuleReadsInstalledProductDatabase_008',async({page,context})=>{
 await route(context);await prepare(page);await install(page);expect(await productRead(page)).toBe('ready')
 await page.evaluate(()=>Reflect.get(globalThis,'f').controller.abort())
})

test('ProductStartup_SourceCancellationRetiresOriginalStoreAfterSocket_009',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{
  const f=Reflect.get(globalThis,'f');f.inputs=await f.read()(f.controller.signal);f.setup=new AbortController();f.closes=0
  globalThis.WebSocket=class extends EventTarget{readyState=0;bufferedAmount=0;binaryType='arraybuffer';constructor(){super();f.socket=this}close(){this.readyState=2;f.closes++}send(){throw Error('unexpected send')}} as any
  f.state='pending';f.open=f.api.browserConnectionSource(f.inputs).open(f.setup.signal).then(()=>{f.state='ready'},()=>{f.state='rejected'})
 })
 await expect.poll(()=>page.evaluate(()=>!!Reflect.get(globalThis,'f').socket)).toBe(true)
 await page.evaluate(()=>Reflect.get(globalThis,'f').setup.abort())
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').inputs.enrollment.signal.aborted)).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{state:f.state,owner:!!(await f.raw()).owner,closes:f.closes}})).toEqual({state:'pending',owner:true,closes:1})
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.socket.readyState=3;f.socket.dispatchEvent(new Event('close'))})
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{owner:(await f.raw()).owner,status:await f.store.status(),originalCancelled:f.controller.signal.aborted}})).toEqual({owner:undefined,status:{revision:'1',state:'active'},originalCancelled:false})
})

test('ProductStartup_AdmissionFailureRetiresWithoutRevokingRecord_010',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),inputs=await f.read()(f.controller.signal),source=f.api.browserConnectionSource(inputs);let rejected=false;try{await source.open(new AbortController().signal,0)}catch{rejected=true}return{rejected,owner:(await f.raw()).owner,aborted:inputs.enrollment.signal.aborted,status:await f.store.status()}})).toEqual({rejected:true,owner:undefined,aborted:true,status:{revision:'1',state:'active'}})
})

test('ProductStartup_InvalidSupplierJoinsOriginalRetirement_011',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f'),open=f.observed.openExisting;f.observed.openExisting=async(...args:any[])=>{const session=await open(...args);session.provider.enrollment.retain(new Promise<void>(r=>{f.release=r}));const read=session.provider.readApproved;return{...session,provider:{enrollment:session.provider.enrollment,readApproved:async(signal:AbortSignal)=>{const original=await read(signal);return{...original,configuration:new TextEncoder().encode('{"version":1,"privateKey":"PUBLIC_TEST_ONLY"}')}}}}};f.reader=f.read();f.state='pending';f.promise=f.reader(f.controller.signal).then(()=>{f.state='ready'},()=>{f.state='rejected'})})
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').sessions[0]?.provider.enrollment.signal.aborted)).toBe(true)
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 await page.evaluate(()=>Reflect.get(globalThis,'f').release());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{owner:(await f.raw()).owner,opens:f.opens,reads:f.reads}})).toEqual({owner:undefined,opens:1,reads:1})
})

test('ProductStartup_MissingOriginalReadIsCached_012',async({page,context})=>{
 await route(context);await prepare(page)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),read=f.read();return{first:await read(f.controller.signal),second:await read(new AbortController().signal),opens:f.opens,reads:f.reads}})).toEqual({first:undefined,second:undefined,opens:1,reads:0})
})

test('ProductStartup_PreReadyFailureWaitsForOriginalOwnerCleanup_013',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(()=>{
  const f=Reflect.get(globalThis,'f'),put=IDBObjectStore.prototype.put
  IDBObjectStore.prototype.put=function(value,key){const store=this,request=put.call(store,value,key);if(value.owner)store.transaction.addEventListener('complete',()=>f.controller.abort(),{once:true});else if(value.state==='active')request.addEventListener('success',()=>{let released=false;f.release=()=>{released=true};const keep=()=>{const q=store.get('current');q.onsuccess=()=>{if(!released)keep()}};keep()});return request}
  f.restore=()=>{IDBObjectStore.prototype.put=put}
  f.state='pending';f.reader=f.read();f.promise=f.reader(f.controller.signal).then(()=>{f.state='ready'},()=>{f.state='rejected'})
 })
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').release)).toBe('function')
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 await page.evaluate(()=>Reflect.get(globalThis,'f').release());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.restore();return{owner:(await f.raw()).owner,sessions:f.sessions.length,opens:f.opens}})).toEqual({owner:undefined,sessions:0,opens:1})
})

test('ProductStartup_RelayConstructorFailureRetainsOriginalPhysicalSocket_014',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{
  const f=Reflect.get(globalThis,'f'),inputs=await f.read()(f.controller.signal),now=performance.now.bind(performance);f.closes=0
  globalThis.WebSocket=class extends EventTarget{readyState=0;bufferedAmount=0;binaryType='arraybuffer';constructor(){super();f.socket=this;Object.defineProperty(performance,'now',{configurable:true,value:()=>now()+20000})}close(){this.readyState=2;f.closes++}send(){throw Error('unexpected send')}} as any
  f.state='pending';f.open=f.api.browserConnectionSource(inputs).open(new AbortController().signal).then(()=>{f.state='ready'},()=>{f.state='rejected'});Object.defineProperty(performance,'now',{configurable:true,value:now})
 })
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').sessions[0].provider.enrollment.signal.aborted)).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{state:f.state,closes:f.closes,owner:!!(await f.raw()).owner}})).toEqual({state:'pending',closes:1,owner:true})
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.socket.readyState=3;f.socket.dispatchEvent(new Event('close'))})
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>Reflect.get(globalThis,'f').raw().then((v:any)=>v.owner))).toBeUndefined()
})
