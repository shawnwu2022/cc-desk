import {test,expect,type Page,type BrowserContext} from '@playwright/test'
import {build} from 'esbuild'
import {createECDH,createPublicKey} from 'node:crypto'

// 合成 Worker 通知与 socket 用于入口生命周期测试，不声称真实 TLS/认证器证据。
let module:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 const keys=[1,2,3,4].map(n=>{const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();const jwk={kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')};return{jwk,spki:createPublicKey({key:jwk,format:'jwk'}).export({format:'der',type:'spki'}).toString('base64url')}})
 module=(await build({stdin:{resolveDir:process.cwd(),contents:`
 import {createBrowserRegistrationEntry} from './remote/ui-terminal/src/browserRegistrationEntry.ts';
 import {controlledBrowserRegistrationSource} from './remote/ui-terminal/src/browserRegistrationSource.ts';
 import {createBrowserEnrollmentStore} from './remote/ui-terminal/src/browserEnrollmentStore.ts';
 import {browserTransport} from './remote/ui-terminal/src/browserTransport.ts';
 export {createBrowserRegistrationEntry,controlledBrowserRegistrationSource,createBrowserEnrollmentStore};
 export async function fixture(){
  const data=${JSON.stringify(keys)};
  const [tlsKey,grantKey,relayKey]=await Promise.all(data.slice(0,3).map(({jwk})=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));
  const configuration={version:1,profile:{origin:location.origin,rpId:location.hostname,credentialId:'Bw'},localSpki:data[0].spki,hostSpki:data[3].spki,trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:data[1].spki,principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:location.origin,rpId:location.hostname,credentialId:'Bw',credentialSpki:data[3].spki,backupEligible:false,allowBackedUp:false}},relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:'CQkJCQkJCQkJCQkJCQkJCQ',keyId:'CAgICAgICAgICAgICAgICA'},artifacts:{wasm:location.origin+'/terminal.wasm',glue:location.origin+'/terminal.js'}};
  const template=structuredClone(configuration);delete template.profile.credentialId;delete template.grant.profile.credentialId;delete template.grant.profile.credentialSpki;delete template.grant.profile.backupEligible;
  const approvedPlan={databaseName:'TEST_ONLY_ENTRY',expectedRevision:null,configuration:template,relaySpki:data[2].spki,requestedUserHandle:'AQID'};
  const inputs={keys:{tlsKey,grantKey,relayKey},approvedPlan,artifacts:{wasm:location.origin+'/registration.wasm',glue:location.origin+'/registration.js'}};
  const f={inputs,configuration,reads:0,sockets:0,workers:0,terminated:0,holdSocket:false,holdWorker:false,skipCompleted:false,reject:false,wrongOutcome:false};
  f.outcome={kind:'BrowserInstallationApproved',receipt:'1',scope:structuredClone(configuration.grant.scope),installation:{...structuredClone(approvedPlan),configuration:JSON.stringify(configuration)}};
  const controller=new AbortController();f.controller=controller;f.lifetime={signal:controller.signal,setupDeadline:performance.timeOrigin+performance.now()+5000,registrationDeadline:performance.timeOrigin+performance.now()+20000};
  class Socket extends EventTarget{
   readyState=1;bufferedAmount=0;protocol='cc-desk-relay-rendezvous-v1';binaryType='arraybuffer';
   constructor(){super();f.sockets++;if(f.expireDuringSocket){const original=performance.now.bind(performance);Object.defineProperty(performance,'now',{value:()=>original()+11000})}queueMicrotask(()=>this.receive(new Uint8Array([1,0,...new Uint8Array(64)])));}
   receive(bytes){this.dispatchEvent(new MessageEvent('message',{data:bytes.buffer}));}
   send(bytes){if(bytes.length===83)queueMicrotask(()=>this.receive(new Uint8Array([1,2])));if(bytes.length===18&&bytes[0]===2)queueMicrotask(()=>this.receive(new Uint8Array([2,1,...bytes.slice(2)])));}
   close(){if(this.readyState!==1)return;this.readyState=2;f.releaseSocket=()=>{this.readyState=3;this.dispatchEvent(new Event('close'))};if(!f.holdSocket)queueMicrotask(f.releaseSocket);}
  }
  class SyntheticWorker extends EventTarget{
   constructor(){super();f.workers++;}
   terminate(){f.terminated++;}
   postMessage(d){if(d.kind!=='start')return;f.start=d;const wire=browserTransport(d.transport);const p=d.bridge.port;Atomics.store(new Int32Array(d.bridge.cancel),1,1);
    const retired=new Promise(resolve=>{p.onmessage=e=>{if(e.data==='retired'){p.close();d.bridge.signer.port.close();resolve()}};p.start()});
    const send=value=>this.dispatchEvent(new MessageEvent('message',{data:value}));
    queueMicrotask(async()=>{send({kind:'evidence',value:{protocol:772}});send({kind:'outcome',value:f.reject?{kind:'Rejected'}:f.wrongOutcome?{...f.outcome,installation:{...f.outcome.installation,databaseName:'OTHER'}}:f.outcome});if(!f.skipCompleted)send({kind:'completed'});else send({kind:'failed'});wire.close();p.postMessage('retired');await Promise.all([retired,wire.waitClosed()]);f.releaseWorker=()=>send({kind:'retired'});if(!f.holdWorker)f.releaseWorker();});
   }
  }
  globalThis.WebSocket=Socket;globalThis.Worker=SyntheticWorker;
  f.store=createBrowserEnrollmentStore({origin:location.origin,databaseName:'TEST_ONLY_ENTRY'});globalThis.f=f;return f;
 }
 `},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
})
async function prepare(page:Page,context:BrowserContext,provider='export async function readApprovedBrowserRegistrationInputs(){globalThis.f.reads++;return globalThis.f.inputs}'){
 await context.route('https://desk.invalid/**',async r=>{const path=new URL(r.request().url()).pathname;await r.fulfill({status:path==='/browser-registration.js'&&provider==='missing'?404:200,headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:path.endsWith('.js')||path.endsWith('.mjs')?'text/javascript':'text/html',body:path==='/entry.mjs'?module:path==='/browser-registration.js'?provider:'<!doctype html><title>TEST_ONLY entry</title>'})})
 await page.goto('https://desk.invalid/');await page.evaluate(async()=>{const api=await import(location.origin+'/entry.mjs');await api.fixture();Reflect.get(globalThis,'f').api=api})
}
test('RegistrationEntry_OriginalPlanAndReload_001',async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');const source=f.api.controlledBrowserRegistrationSource();await source.prepare(f.controller.signal);const owner=await source.register(f.lifetime);const result=await owner.install();await owner.waitClosed();return{result,reads:f.reads,sockets:f.sockets,workers:f.workers,terminated:f.terminated,hostPin:Array.from(f.start.hostSpki),expectedPin:Array.from(Uint8Array.from(atob(f.inputs.approvedPlan.configuration.hostSpki.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0))),replay:await source.register(f.lifetime).then(()=>false,()=>true)}})).toMatchObject({result:{revision:'1',state:'active'},reads:1,sockets:1,workers:1,terminated:1,replay:true})
 expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');const expected=Uint8Array.from(atob(f.inputs.approvedPlan.configuration.hostSpki.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));return expected.length===f.start.hostSpki.length&&expected.every((value,index)=>value===f.start.hostSpki[index])})).toBe(true)
 await page.reload()
 expect(await page.evaluate(async()=>{const api=await import(location.origin+'/entry.mjs');crypto.subtle.importKey=()=>{throw Error('TEST_ONLY reload must not import keys')};const store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'TEST_ONLY_ENTRY'}),signal=new AbortController().signal,session=await store.openExisting(async()=>{},signal),material=await session.provider.readApproved(signal);const ok=[material.handles.tlsKey,material.handles.grantKey].every(k=>k instanceof CryptoKey&&!k.extractable);session.close();await session.waitClosed();return ok})).toBe(true)
})
for(const change of ['cancel','wallBack'])test(`RegistrationEntry_SourceHoldsOriginalProvider_${change}_007`,async({page,context})=>{
 await prepare(page,context,'export async function readApprovedBrowserRegistrationInputs(){globalThis.f.reads++;await new Promise(resolve=>{globalThis.f.releaseProvider=resolve});return globalThis.f.inputs}')
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.source=f.api.controlledBrowserRegistrationSource();f.state='pending';void f.source.prepare(f.controller.signal).then(()=>{f.state='resolved'},()=>{f.state='rejected'})})
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').releaseProvider)).toBe('function')
 await page.evaluate(change=>{const f=Reflect.get(globalThis,'f');if(change==='cancel')f.controller.abort();else{const original=Date.now.bind(Date);Date.now=()=>original()-2000}},change)
 await page.waitForTimeout(150)
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('pending')
 await page.evaluate(()=>Reflect.get(globalThis,'f').releaseProvider());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{reads:f.reads,sockets:f.sockets,replay:await f.source.register(f.lifetime).then(()=>false,()=>true)}})).toEqual({reads:1,sockets:0,replay:true})
})
for(const failure of ['missing','reject','undefined','shape'])test(`RegistrationEntry_SourceLockedAndNoReplay_${failure}_002`,async({page,context})=>{
 await prepare(page,context,failure==='missing'?'missing':failure==='reject'?"export async function readApprovedBrowserRegistrationInputs(){globalThis.f.reads++;throw Error('TEST_ONLY denied')}":failure==='undefined'?'export async function readApprovedBrowserRegistrationInputs(){globalThis.f.reads++;return undefined}':'export const wrong=true')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),source=f.api.controlledBrowserRegistrationSource();await source.prepare(f.controller.signal).catch(()=>{});const replay=await f.api.controlledBrowserRegistrationSource().register(f.lifetime).then(()=>false,()=>true);return{replay,sockets:f.sockets,workers:f.workers,reads:f.reads}})).toMatchObject({replay:true,sockets:0,workers:0})
})
for(const failure of ['reject','wrongOutcome','skipCompleted'])test(`RegistrationEntry_RefusesInstall_${failure}_003`,async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async failure=>{const f=Reflect.get(globalThis,'f');f[failure]=true;const owner=f.api.createBrowserRegistrationEntry(f.inputs,f.lifetime);const rejected=await owner.install().then(()=>false,()=>true);await owner.waitClosed();return{rejected,status:await f.store.status()}},failure)).toEqual({rejected:true,status:undefined})
})
test('RegistrationEntry_RelayConstructorFailureRetainsSocket_008',async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.expireDuringSocket=true;f.holdSocket=true;f.owner=f.api.createBrowserRegistrationEntry(f.inputs,f.lifetime);f.done=false;void f.owner.install().then(()=>{f.done=true},()=>{f.done=true})})
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').releaseSocket)).toBe('function')
 expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{done:f.done,sockets:f.sockets,workers:f.workers}})).toEqual({done:false,sockets:1,workers:0})
 await page.evaluate(()=>Reflect.get(globalThis,'f').releaseSocket());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').done)).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.owner.waitClosed();return f.store.status()})).toBeUndefined()
})
for(const held of ['holdSocket','holdWorker'])test(`RegistrationEntry_JoinsOriginalRetirement_${held}_004`,async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(held=>{const f=Reflect.get(globalThis,'f');f[held]=true;f.owner=f.api.createBrowserRegistrationEntry(f.inputs,f.lifetime);f.done=false;f.promise=f.owner.install().then(()=>{f.done=true})},held)
 await expect.poll(()=>page.evaluate(held=>typeof Reflect.get(globalThis,'f')[held==='holdSocket'?'releaseSocket':'releaseWorker'],held)).toBe('function')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{done:f.done,terminated:f.terminated,status:await f.store.status()}})).toEqual({done:false,terminated:0,status:undefined})
 await page.evaluate(held=>{const f=Reflect.get(globalThis,'f');f[held==='holdSocket'?'releaseSocket':'releaseWorker']()},held)
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').done)).toBe(true)
})
for(const name of ['tlsKey','grantKey','relayKey'])test(`RegistrationEntry_KeyMismatchBeforeSocket_${name}_005`,async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async name=>{const f=Reflect.get(globalThis,'f');f.inputs.keys[name]=(await crypto.subtle.generateKey({name:'ECDSA',namedCurve:'P-256'},false,['sign','verify'])).privateKey;const owner=f.api.createBrowserRegistrationEntry(f.inputs,f.lifetime);const rejected=await owner.install().then(()=>false,()=>true);await owner.waitClosed();return{rejected,sockets:f.sockets,workers:f.workers,status:await f.store.status()}},name)).toEqual({rejected:true,sockets:0,workers:0,status:undefined})
})
for(const stop of ['cancel','wallBack','wallForward','setup-expiry'])test(`RegistrationEntry_RetainsPreflight_${stop}_006`,async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(stop=>{const f=Reflect.get(globalThis,'f'),original=crypto.subtle.sign.bind(crypto.subtle);crypto.subtle.sign=async(...args)=>{f.signing=true;const value=await original(...args);await new Promise(r=>{f.release=r});return value};if(stop==='setup-expiry')f.lifetime.setupDeadline=performance.timeOrigin+performance.now()+700;f.owner=f.api.createBrowserRegistrationEntry(f.inputs,f.lifetime);f.done=false;void f.owner.install().then(()=>{f.done=true},()=>{f.done=true})},stop)
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').release)).toBe('function')
 await page.evaluate(stop=>{const f=Reflect.get(globalThis,'f');if(stop==='cancel')f.controller.abort();else if(stop!=='setup-expiry'){const original=Date.now.bind(Date);Date.now=()=>original()+(stop==='wallBack'?-2000:2000)}},stop)
 await page.waitForTimeout(stop==='setup-expiry'?800:150)
 expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{done:f.done,sockets:f.sockets,terminated:f.terminated}})).toEqual({done:false,sockets:0,terminated:0})
 await page.evaluate(()=>Reflect.get(globalThis,'f').release());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').done)).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.owner.waitClosed();return{status:await f.store.status(),sockets:f.sockets}})).toEqual({status:undefined,sockets:0})
})

test('RegistrationEntry_ApprovalSnapshot_009',async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async()=>{
  const f=Reflect.get(globalThis,'f'),source=f.api.controlledBrowserRegistrationSource()
  f.inputs={keys:{...f.inputs.keys},approvedPlan:JSON.parse(JSON.stringify(f.inputs.approvedPlan)),artifacts:{...f.inputs.artifacts}}
  const expected=f.inputs.approvedPlan.configuration.hostSpki
  await source.prepare(f.controller.signal)
  f.inputs.approvedPlan.configuration.hostSpki=f.inputs.approvedPlan.configuration.localSpki
  f.inputs.keys.tlsKey=f.inputs.keys.grantKey
  f.inputs.artifacts.wasm='https://desk.invalid/changed.wasm'
  const owner=await source.register(f.lifetime);const result=await owner.install();await owner.waitClosed()
  return {revision:result.revision,exactPin:JSON.stringify(Array.from(f.start.hostSpki))===JSON.stringify(Array.from(Uint8Array.from(atob(expected.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0))))}
 })).toMatchObject({revision:'1',exactPin:true})
})
