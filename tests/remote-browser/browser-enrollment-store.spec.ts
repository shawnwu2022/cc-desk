import {test,expect,type Page,type BrowserContext} from '@playwright/test'
import {build} from 'esbuild'
import {createECDH} from 'node:crypto'

let module:string,fixture:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 module=(await build({entryPoints:['remote/ui-terminal/src/browserEnrollmentStore.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
 const keys=[1,2,3].map(n=>{const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();return{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')}})
 fixture=`export * from '/store.mjs';export async function material(){const [tlsKey,grantKey,relayKey]=await Promise.all(${JSON.stringify(keys)}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));const b=(n,v)=>btoa(String.fromCharCode(...new Uint8Array(n).fill(v))).replace(/\\+/g,'-').replace(/\\//g,'_').replace(/=+$/,'');return{keys:{tlsKey,grantKey,relayKey},configuration:new TextEncoder().encode(JSON.stringify({version:1,profile:{origin:location.origin,rpId:location.hostname,credentialId:'Bw'},localSpki:b(91,1),hostSpki:b(91,2),trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:b(91,3),principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:location.origin,rpId:location.hostname,credentialId:'Bw',credentialSpki:b(91,4),backupEligible:false,allowBackedUp:false}},relay:{endpoint:'wss://relay.invalid/route',tunnel:b(16,9),keyId:b(16,8)},artifacts:{wasm:location.origin+'/fixture.wasm',glue:location.origin+'/fixture.js'}}))}};`
})
async function route(context:BrowserContext){await context.route('https://desk.invalid/**',async r=>{const p=new URL(r.request().url()).pathname;await r.fulfill({headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:p.endsWith('.mjs')?'text/javascript':'text/html',body:p==='/store.mjs'?module:p==='/fixture.mjs'?fixture:'<!doctype html><title>TEST_ONLY enrollment store</title>'})})}
async function prepare(page:Page){await page.goto('https://desk.invalid/');await page.evaluate(async()=>{const api=await import(location.origin+'/fixture.mjs');Reflect.set(globalThis,'f',{api,store:api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'PUBLIC_TEST_ONLY'}),signal:new AbortController().signal})})}
async function install(page:Page){return page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.installApproved(undefined,async(signal:AbortSignal)=>{if(signal.aborted)throw Error('aborted');return f.api.material()},f.signal)})}

test('EnrollmentStore_ExplicitCommitSurvivesNewPage_001',async({page,context})=>{
 await route(context);await prepare(page)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.openExisting(async()=>{},f.signal)})).toBeUndefined()
 expect(await install(page)).toEqual({revision:'1',state:'active'})
 await page.close();const next=await context.newPage();await prepare(next)
 const result=await next.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.session=await f.store.openExisting(async()=>{},f.signal);const material=await f.session.provider.readApproved(f.signal);let exported=false;try{await crypto.subtle.exportKey('jwk',material.handles.tlsKey);exported=true}catch{}return{metadata:await f.store.status(),keys:material.handles.tlsKey instanceof CryptoKey&&!material.handles.tlsKey.extractable&&!material.handles.grantKey.extractable,exported,configuration:new TextDecoder().decode(material.configuration)}})
 expect(result.metadata).toEqual({revision:'1',state:'active'});expect(result.keys).toBe(true);expect(result.exported).toBe(false);expect(JSON.parse(result.configuration).grant.scope.run).toBe('PUBLIC_TEST_ONLY')
 await next.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.session.close();await f.session.waitClosed()})
})

test('EnrollmentStore_CompetingPageAndDurableRevokeStayLocked_002',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.session=await f.store.openExisting(async()=>{},f.signal)})
 const other=await context.newPage();await prepare(other)
 expect(await other.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.openExisting(async()=>{},f.signal)})).toBeUndefined()
 expect(await other.evaluate(async()=>Reflect.get(globalThis,'f').store.revoke())).toEqual({revision:'1',state:'revoked'})
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.session.waitClosed();return f.session.provider.enrollment.signal.aborted})).toBe(true)
 const fresh=await context.newPage();await prepare(fresh)
 expect(await fresh.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.openExisting(async()=>{},f.signal)})).toBeUndefined()
 expect(await fresh.evaluate(async()=>Reflect.get(globalThis,'f').store.status())).toEqual({revision:'1',state:'revoked'})
})

test('EnrollmentStore_RevokeRetainsOriginalPendingOwner_003',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.session=await f.store.openExisting(async()=>{},f.signal);f.session.provider.enrollment.retain(new Promise<void>(r=>{f.release=r}))})
 const other=await context.newPage();await prepare(other)
 await other.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.state='pending';f.promise=f.store.revoke().then(()=>{f.state='revoked'})})
 await expect.poll(()=>other.evaluate(async()=>Reflect.get(globalThis,'f').store.status())).toEqual({revision:'1',state:'revoking'})
 expect(await other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 await page.evaluate(()=>Reflect.get(globalThis,'f').release())
 await expect.poll(()=>other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('revoked')
})

test('EnrollmentStore_ReplacementNeedsExactRevokedRevision_004',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');let loads=0;await f.store.installApproved('1',async()=>{loads++;return f.api.material()},f.signal).catch(()=>{});return loads})).toBe(0)
 await page.evaluate(async()=>Reflect.get(globalThis,'f').store.revoke())
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.installApproved(undefined,()=>f.api.material(),f.signal).then(()=>false,()=>true)})).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.installApproved('1',()=>f.api.material(),f.signal)})).toEqual({revision:'2',state:'active'})
})

test('EnrollmentStore_RequestSuccessDoesNotCommitAbortedTransaction_005',async({page,context})=>{
 await route(context);await prepare(page)
 const result=await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),put=IDBObjectStore.prototype.put;let requestSucceeded=false;IDBObjectStore.prototype.put=function(...args){const request=put.apply(this,args);request.addEventListener('success',()=>{requestSucceeded=true;this.transaction.abort()});return request};try{const rejected=await f.store.installApproved(undefined,()=>f.api.material(),f.signal).then(()=>false,()=>true);return{rejected,requestSucceeded,status:await f.store.status()}}finally{IDBObjectStore.prototype.put=put}})
 expect(result).toEqual({rejected:true,requestSucceeded:true,status:undefined})
})

test('EnrollmentStore_CancelRetainsOriginalLoaderAndCannotCommitLate_006',async({page,context})=>{
 await route(context);await prepare(page)
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.controller=new AbortController();f.state='pending';f.promise=f.store.installApproved(undefined,async()=>{const value=await f.api.material();return new Promise(r=>{f.release=()=>r(value)})},f.controller.signal).then(()=>{f.state='committed'},()=>{f.state='rejected'})})
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').release)).toBe('function')
 await page.evaluate(()=>Reflect.get(globalThis,'f').controller.abort())
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.openExisting(async()=>{},f.signal)})).toBeUndefined()
 await page.evaluate(()=>Reflect.get(globalThis,'f').release());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>Reflect.get(globalThis,'f').store.status())).toBeUndefined()
})

test('EnrollmentStore_ContextLossCannotClearRetirementMarker_007',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.session=await f.store.openExisting(async()=>{},f.signal);f.session.provider.enrollment.retain(new Promise<void>(()=>{}))})
 const other=await context.newPage();await prepare(other)
 await other.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.state='pending';f.promise=f.store.revoke().then(()=>{f.state='revoked'},()=>{f.state='uncertain'})})
 await expect.poll(()=>other.evaluate(async()=>Reflect.get(globalThis,'f').store.status())).toEqual({revision:'1',state:'revoking'})
 await page.close();await expect.poll(()=>other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('uncertain')
 expect(await other.evaluate(async()=>Reflect.get(globalThis,'f').store.status())).toEqual({revision:'1',state:'revoking'})
 expect(await other.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return f.store.installApproved('1',()=>f.api.material(),f.signal).then(()=>false,()=>true)})).toBe(true)
})

test('EnrollmentStore_PersistentFenceClosesInitializationWithoutBroadcast_008',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const result=await page.evaluate(async()=>{
  const f=Reflect.get(globalThis,'f'),db=await new Promise<IDBDatabase>((r,j)=>{const q=indexedDB.open('PUBLIC_TEST_ONLY',1);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error)}),put=IDBObjectStore.prototype.put
  let injected=false
  IDBObjectStore.prototype.put=function(value,key){const request=put.call(this,value,key);if(value.state==='active'&&value.owner&&!injected){injected=true;request.addEventListener('success',()=>{const tx=db.transaction('enrollment','readwrite');put.call(tx.objectStore('enrollment'),{version:1,revision:value.revision,state:'revoking',owner:value.owner},'current')})}return request}
  try{const rejected=await f.store.openExisting(async()=>{},f.signal).then(()=>false,()=>true);return{injected,rejected}}finally{IDBObjectStore.prototype.put=put;db.close()}
 })
 expect(result).toEqual({injected:true,rejected:true})
 expect(await page.evaluate(async()=>Reflect.get(globalThis,'f').store.revoke())).toEqual({revision:'1',state:'revoked'})
})

test('EnrollmentStore_LateOldRevokerCannotOverwriteReplacement_009',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const other=await context.newPage();await prepare(other)
 await other.evaluate(()=>{const f=Reflect.get(globalThis,'f'),request=navigator.locks.request.bind(navigator.locks);Object.defineProperty(navigator.locks,'request',{value:async(...args:any[])=>{f.waiting=true;await new Promise<void>(r=>{f.release=r});return Reflect.apply(request,navigator.locks,args)}});f.state='pending';f.promise=f.store.revoke().then(()=>{f.state='revoked'},()=>{f.state='stale'})})
 await expect.poll(()=>other.evaluate(()=>Reflect.get(globalThis,'f').waiting)).toBe(true)
 await page.evaluate(async()=>Reflect.get(globalThis,'f').store.revoke())
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.store.installApproved('1',()=>f.api.material(),f.signal);f.session=await f.store.openExisting(async()=>{},f.signal);const channel=new BroadcastChannel('cc-desk-enrollment:PUBLIC_TEST_ONLY');channel.postMessage({revision:'1'});channel.close()})
 await other.evaluate(()=>Reflect.get(globalThis,'f').release())
 // Let the stale finalizer acquire the lock only after the new original owner retires.
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').session.provider.enrollment.signal.aborted)).toBe(false)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.session.close();await f.session.waitClosed()})
 await expect.poll(()=>other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('stale')
 expect(await page.evaluate(async()=>Reflect.get(globalThis,'f').store.status())).toEqual({revision:'2',state:'active'})
})

test('EnrollmentStore_AbortedClaimCannotPublishOrLeaveOwner_010',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const rejected=await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),put=IDBObjectStore.prototype.put;IDBObjectStore.prototype.put=function(value,key){const request=put.call(this,value,key);if(value.owner)request.addEventListener('success',()=>this.transaction.abort());return request};try{return await f.store.openExisting(async()=>{},f.signal).then(()=>false,()=>true)}finally{IDBObjectStore.prototype.put=put}})
 expect(rejected).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');const session=await f.store.openExisting(async()=>{},f.signal);if(!session)return false;session.close();await session.waitClosed();return true})).toBe(true)
})

test('EnrollmentStore_FailedOwnerCleanupCannotReleaseKnownRetirement_011',async({page,context})=>{
 await route(context);await prepare(page);await install(page)
 const rejected=await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.session=await f.store.openExisting(async()=>{},f.signal);const put=IDBObjectStore.prototype.put;IDBObjectStore.prototype.put=function(value,key){const request=put.call(this,value,key);if(value.state==='active'&&!value.owner)request.addEventListener('success',()=>this.transaction.abort());return request};try{f.session.close();return await f.session.waitClosed().then(()=>false,()=>true)}finally{IDBObjectStore.prototype.put=put}})
 expect(rejected).toBe(true)
 const other=await context.newPage();await prepare(other)
 await other.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.state='pending';f.promise=f.store.revoke().then(()=>{f.state='revoked'},()=>{f.state='uncertain'})})
 await expect.poll(()=>other.evaluate(async()=>Reflect.get(globalThis,'f').store.status())).toEqual({revision:'1',state:'revoking'});expect(await other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 await page.close();await expect.poll(()=>other.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('uncertain')
})
