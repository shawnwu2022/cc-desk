import {test,expect,type BrowserContext,type Page} from '@playwright/test'
import {build} from 'esbuild'
import {createECDH,createPublicKey} from 'node:crypto'

// 这里只替代跨 Worker 的会话通知，持久化与密钥操作均使用真实 Edge 实现；不是通道全链证据。
let module:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 const keys=[1,2,3,4].map(n=>{const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();const jwk={kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')};return{jwk,spki:createPublicKey({key:jwk,format:'jwk'}).export({format:'der',type:'spki'}).toString('base64url')}})
 module=(await build({stdin:{resolveDir:process.cwd(),contents:`
 export {createBrowserRegistrationInstallation} from './remote/ui-terminal/src/browserRegistrationInstallation.ts';
 export {createBrowserEnrollmentStore} from './remote/ui-terminal/src/browserEnrollmentStore.ts';
 export const deferred=()=>{let resolve,reject;const promise=new Promise((r,j)=>{resolve=r;reject=j});return{promise,resolve,reject}};
 export async function inputs(){
  const data=${JSON.stringify(keys)},[tlsKey,grantKey,relayKey]=await Promise.all(data.slice(0,3).map(({jwk})=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));
  const configuration={version:1,profile:{origin:location.origin,rpId:location.hostname,credentialId:'Bw'},localSpki:data[0].spki,hostSpki:data[3].spki,trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:data[1].spki,principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:location.origin,rpId:location.hostname,credentialId:'Bw',credentialSpki:data[3].spki,backupEligible:false,allowBackedUp:false}},relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:'CQkJCQkJCQkJCQkJCQkJCQ',keyId:'CAgICAgICAgICAgICAgICA'},artifacts:{wasm:location.origin+'/fixture.wasm',glue:location.origin+'/fixture.js'}};
  const template=structuredClone(configuration);delete template.profile.credentialId;delete template.grant.profile.credentialId;delete template.grant.profile.credentialSpki;delete template.grant.profile.backupEligible;
  const approvedPlan={databaseName:'PUBLIC_TEST_ONLY_INSTALLATION',expectedRevision:null,configuration:template,relaySpki:data[2].spki,requestedUserHandle:'AQID'};
  const outcome={kind:'BrowserInstallationApproved',receipt:'1',scope:structuredClone(configuration.grant.scope),installation:{...approvedPlan,configuration:JSON.stringify(configuration)}};
  const controller=new AbortController(),session={outcome:Promise.resolve(outcome),completed:Promise.resolve(),close(){},waitClosed(){return Promise.resolve()}};
  return{options:{session,keys:{tlsKey,grantKey,relayKey},approvedPlan,signal:controller.signal,deadline:performance.timeOrigin+performance.now()+20000},outcome,controller,configuration};
 }
 `},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
})
async function prepare(page:Page,context:BrowserContext){
 await context.route('https://desk.invalid/**',async r=>{const isModule=new URL(r.request().url()).pathname==='/fixture.mjs';await r.fulfill({headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:isModule?'text/javascript':'text/html',body:isModule?module:'<!doctype html><title>TEST_ONLY installation composition</title>'})})
 await page.goto('https://desk.invalid/');await page.evaluate(async()=>{const api=await import(location.origin+'/fixture.mjs');Reflect.set(globalThis,'f',{api,...await api.inputs(),store:api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'PUBLIC_TEST_ONLY_INSTALLATION'})})})
}

test('RegistrationInstallation_OriginalApprovalCommitsAndReloads_001',async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');f.owner=f.api.createBrowserRegistrationInstallation(f.options);const result=await f.owner.install();await f.owner.waitClosed();return result})).toEqual({revision:'1',state:'active'})
 await page.reload()
 const loaded=await page.evaluate(async()=>{const api=await import(location.origin+'/fixture.mjs'),store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'PUBLIC_TEST_ONLY_INSTALLATION'}),signal=new AbortController().signal;const session=await store.openExisting(async()=>{},signal);const material=await session.provider.readApproved(signal);const result={metadata:await store.status(),configuration:JSON.parse(new TextDecoder().decode(material.configuration)),keys:[material.handles.tlsKey,material.handles.grantKey].every(k=>k instanceof CryptoKey&&!k.extractable)};session.close();await session.waitClosed();return result})
 expect(loaded.metadata).toEqual({revision:'1',state:'active'});expect(loaded.keys).toBe(true);expect(loaded.configuration.profile.credentialId).toBe('Bw');expect(loaded.configuration.grant.scope.run).toBe('PUBLIC_TEST_ONLY')
})

for(const kind of ['Rejected','RunLocalCandidateInstalled'])test(`RegistrationInstallation_NoPersistencePurpose_${kind}_002`,async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async kind=>{const f=Reflect.get(globalThis,'f');f.options.session.outcome=Promise.resolve(kind==='Rejected'?{kind}:{kind,receipt:'1',scope:f.outcome.scope});const original=crypto.subtle.sign.bind(crypto.subtle);let signs=0;crypto.subtle.sign=(...args)=>{signs++;return original(...args)};try{const owner=f.api.createBrowserRegistrationInstallation(f.options);const rejected=await owner.install().then(()=>false,()=>true);await owner.waitClosed();return{rejected,signs,status:await f.store.status()}}finally{crypto.subtle.sign=original}},kind)).toEqual({rejected:true,signs:0,status:undefined})
})

for(const field of ['origin','rpId','hostSpki','localSpki','issuerSpki','relaySpki','userHandle','database','revision','scope','role','budget','peerBudget','epoch','endpoint','relayMode','artifacts','allowBackedUp','extra'])test(`RegistrationInstallation_IndependentPlanMismatch_${field}_003`,async({page,context})=>{
 await prepare(page,context)
 const result=await page.evaluate(async field=>{const f=Reflect.get(globalThis,'f'),c=structuredClone(f.configuration),o=f.outcome;
  if(field==='origin')c.profile.origin=c.grant.profile.origin='https://other.desk.invalid';
  if(field==='rpId')c.profile.rpId=c.grant.profile.rpId='invalid';
  if(field==='hostSpki')c.hostSpki=c.localSpki;
  if(field==='localSpki')c.localSpki=c.hostSpki;
  if(field==='issuerSpki')c.grant.issuerSpki=c.hostSpki;
  if(field==='relaySpki')o.installation.relaySpki=c.hostSpki;
  if(field==='userHandle')o.installation.requestedUserHandle='BAUG';
  if(field==='database')o.installation.databaseName='PUBLIC_TEST_ONLY_OTHER';
  if(field==='revision')o.installation.expectedRevision='2';
  if(field==='scope')o.scope.run='OTHER';
  if(field==='role')c.grant.role='Observer';
  if(field==='budget')c.grant.budgetMs=9999;
  if(field==='peerBudget')c.grant.peerInstall.budgetMs=999;
  if(field==='epoch')c.trust.clientKeyEpoch='3';
  if(field==='endpoint')c.trust.origin=c.relay.endpoint='wss://other.invalid/route';
  if(field==='relayMode')delete c.relay.mode;
  if(field==='artifacts')c.artifacts.glue=location.origin+'/other.js';
  if(field==='allowBackedUp')c.grant.profile.allowBackedUp=true;
  if(field==='extra')c.extra=true;
  o.installation.configuration=JSON.stringify(c);const original=crypto.subtle.sign.bind(crypto.subtle);let signs=0;crypto.subtle.sign=(...args)=>{signs++;return original(...args)};
  try{const owner=f.api.createBrowserRegistrationInstallation(f.options),rejected=await owner.install().then(()=>false,()=>true);await owner.waitClosed();return{rejected,signs,status:await f.store.status()}}finally{crypto.subtle.sign=original}
 },field)
 expect(result).toEqual({rejected:true,signs:0,status:undefined})
})

for(const key of ['tlsKey','grantKey','relayKey'])test(`RegistrationInstallation_OriginalKeyMismatch_${key}_004`,async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async key=>{const f=Reflect.get(globalThis,'f'),pair=await crypto.subtle.generateKey({name:'ECDSA',namedCurve:'P-256'},false,['sign','verify']);f.options.keys[key]=pair.privateKey;const owner=f.api.createBrowserRegistrationInstallation(f.options);const rejected=await owner.install().then(()=>false,()=>true);await owner.waitClosed();return{rejected,status:await f.store.status()}},key)).toEqual({rejected:true,status:undefined})
})

for(const held of ['outcome','completed','retirement'])test(`RegistrationInstallation_HoldsOriginal_${held}_005`,async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(held=>{const f=Reflect.get(globalThis,'f');f.held=f.api.deferred();if(held==='retirement')f.options.session.waitClosed=()=>f.held.promise;else f.options.session[held]=f.held.promise;f.owner=f.api.createBrowserRegistrationInstallation(f.options);f.state='pending';f.promise=f.owner.install().then(()=>{f.state='installed'},()=>{f.state='rejected'});f.retired=false;void f.owner.waitClosed().then(()=>{f.retired=true})},held)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await new Promise(r=>setTimeout(r,150));return{state:f.state,retired:f.retired,status:await f.store.status()}})).toEqual({state:'pending',retired:false,status:undefined})
 await page.evaluate(held=>{const f=Reflect.get(globalThis,'f');f.held.resolve(held==='outcome'?f.outcome:undefined)},held)
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('installed')
})

for(const failed of ['completed','retirement'])test(`RegistrationInstallation_RejectsFailed_${failed}_006`,async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async failed=>{const f=Reflect.get(globalThis,'f'),failure=Promise.reject(Error('TEST_ONLY failure'));void failure.catch(()=>{});if(failed==='retirement')f.options.session.waitClosed=()=>failure;else f.options.session.completed=failure;const owner=f.api.createBrowserRegistrationInstallation(f.options),rejected=await owner.install().then(()=>false,()=>true);const retired=await owner.waitClosed().then(()=>true,()=>false);return{rejected,retired,status:await f.store.status()}},failed)).toEqual({rejected:true,retired:failed!=='retirement',status:undefined})
})

test('RegistrationInstallation_CancelWaitsForAllOriginalPromises_007',async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.held=f.api.deferred();f.options.session.completed=f.held.promise;f.closeCalls=0;f.options.session.close=()=>{f.closeCalls++};f.owner=f.api.createBrowserRegistrationInstallation(f.options);f.state='pending';f.promise=f.owner.install().then(()=>{f.state='installed'},()=>{f.state='rejected'});f.retired=false;void f.owner.waitClosed().then(()=>{f.retired=true});f.controller.abort()})
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await new Promise(r=>setTimeout(r,150));return{state:f.state,retired:f.retired,closeCalls:f.closeCalls,status:await f.store.status()}})).toEqual({state:'pending',retired:false,closeCalls:1,status:undefined})
 await page.evaluate(()=>Reflect.get(globalThis,'f').held.resolve());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.owner.waitClosed();return f.store.status()})).toBeUndefined()
})

for(const stop of ['deadline','close','hidden','wallForward','wallBackward','perfForward','perfBackward'])test(`RegistrationInstallation_StopRetainsOriginalKeyProof_${stop}_008`,async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(stop=>{const f=Reflect.get(globalThis,'f'),original=crypto.subtle.sign.bind(crypto.subtle);f.restoreSign=original;f.held=f.api.deferred();crypto.subtle.sign=async(...args)=>{const result=await original(...args);f.signStarted=true;await f.held.promise;return result};if(stop==='deadline')f.options.deadline=performance.timeOrigin+performance.now()+700;f.owner=f.api.createBrowserRegistrationInstallation(f.options);f.state='pending';f.promise=f.owner.install().then(()=>{f.state='installed'},()=>{f.state='rejected'});f.retired=false;void f.owner.waitClosed().then(()=>{f.retired=true})},stop)
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').signStarted)).toBe(true)
 await page.evaluate(stop=>{const f=Reflect.get(globalThis,'f');if(stop==='close')f.owner.close();if(stop==='hidden'){Object.defineProperty(document,'visibilityState',{value:'hidden',configurable:true});document.dispatchEvent(new Event('visibilitychange'))}if(stop.startsWith('wall')){const original=Date.now.bind(Date);Date.now=()=>original()+(stop==='wallForward'?2000:-2000)}if(stop.startsWith('perf')){const original=performance.now.bind(performance);Object.defineProperty(performance,'now',{value:()=>original()+(stop==='perfForward'?2000:-2000)})}},stop)
 await page.waitForTimeout(stop==='deadline'?800:200)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{state:f.state,retired:f.retired,status:await f.store.status()}})).toEqual({state:'pending',retired:false,status:undefined})
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');crypto.subtle.sign=f.restoreSign;f.held.resolve()});await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.owner.waitClosed();return f.store.status()})).toBeUndefined()
})

test('RegistrationInstallation_SingleUseSnapshotAndRevisionCAS_009',async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),owner=f.api.createBrowserRegistrationInstallation(f.options);f.options.approvedPlan.configuration.grant.role='Observer';f.options.signal=new AbortController().signal;f.options.deadline+=120000;const first=await owner.install();const replay=await owner.install().then(()=>false,()=>true);await owner.waitClosed();let sessionReplay=false;try{f.api.createBrowserRegistrationInstallation(f.options)}catch{sessionReplay=true}const next=await f.api.inputs(),conflict=f.api.createBrowserRegistrationInstallation(next.options);const conflictRejected=await conflict.install().then(()=>false,()=>true);await conflict.waitClosed();await f.store.revoke();const replacement=await f.api.inputs();replacement.options.approvedPlan.expectedRevision=replacement.outcome.installation.expectedRevision='1';const replacementOwner=f.api.createBrowserRegistrationInstallation(replacement.options),second=await replacementOwner.install();await replacementOwner.waitClosed();return{first,replay,sessionReplay,conflictRejected,second}})).toEqual({first:{revision:'1',state:'active'},replay:true,sessionReplay:true,conflictRejected:true,second:{revision:'2',state:'active'}})
})

for(const stopped of ['abort','deadline'])test(`RegistrationInstallation_StoreCommitRetainsHistoricalMetadata_${stopped}_010`,async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(stopped=>{const f=Reflect.get(globalThis,'f'),put=IDBObjectStore.prototype.put;f.originalPut=put;IDBObjectStore.prototype.put=function(...args){const tx=this.transaction,request=Reflect.apply(put,this,args);request.addEventListener('success',()=>{const original=tx.oncomplete!;tx.oncomplete=function(event){f.release=()=>original.call(tx,event);f.commitEvent=true}});return request};if(stopped==='deadline')f.options.deadline=performance.timeOrigin+performance.now()+700;f.owner=f.api.createBrowserRegistrationInstallation(f.options);f.state='pending';f.promise=f.owner.install().then((result:unknown)=>{f.state='installed';f.result=result},()=>{f.state='rejected'});f.retired=false;void f.owner.waitClosed().then(()=>{f.retired=true})},stopped)
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').commitEvent)).toBe(true)
 if(stopped==='deadline')await page.waitForTimeout(800)
 expect(await page.evaluate(stopped=>{const f=Reflect.get(globalThis,'f');if(stopped==='abort')f.controller.abort();return{state:f.state,retired:f.retired}},stopped)).toEqual({state:'pending',retired:false})
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');IDBObjectStore.prototype.put=f.originalPut;f.release()});await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('installed')
 // 截止后的 metadata 仅报告已提交历史；不能用取消掩盖真实落盘，也不授权连接。
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.owner.waitClosed();return{result:f.result,status:await f.store.status()}})).toEqual({result:{revision:'1',state:'active'},status:{revision:'1',state:'active'}})
})

for(const invalid of ['plan','allowBackedUp','credentialInPlan','completed','outcome','close','waitClosed','expired','extended','signal'])test(`RegistrationInstallation_MissingIndependentPrerequisite_${invalid}_011`,async({page,context})=>{
 await prepare(page,context)
 const result=await page.evaluate(async invalid=>{const f=Reflect.get(globalThis,'f');if(invalid==='plan')delete f.options.approvedPlan;if(invalid==='allowBackedUp')delete f.options.approvedPlan.configuration.grant.profile.allowBackedUp;if(invalid==='credentialInPlan')f.options.approvedPlan.configuration.profile.credentialId='Bw';if(['completed','outcome','close','waitClosed'].includes(invalid))delete f.options.session[invalid];if(invalid==='expired')f.options.deadline=performance.timeOrigin+performance.now()-1;if(invalid==='extended')f.options.deadline=performance.timeOrigin+performance.now()+120100;if(invalid==='signal')f.controller.abort();let rejected=false;try{const owner=f.api.createBrowserRegistrationInstallation(f.options);await owner.install();await owner.waitClosed()}catch{rejected=true}return{rejected,status:await f.store.status()}},invalid)
 expect(result).toEqual({rejected:true,status:undefined})
})

for(const retirement of ['undefined','throw','thenable'])test(`RegistrationInstallation_OriginalRetirementMustBePromise_${retirement}_012`,async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async retirement=>{const f=Reflect.get(globalThis,'f');f.options.session.waitClosed=()=>{if(retirement==='throw')throw Error('TEST_ONLY');return retirement==='thenable'?{then(resolve:()=>void){resolve()}}:undefined};const owner=f.api.createBrowserRegistrationInstallation(f.options),rejected=await owner.install().then(()=>false,()=>true),retired=await owner.waitClosed().then(()=>true,()=>false);return{rejected,retired,status:await f.store.status()}},retirement)).toEqual({rejected:true,retired:false,status:undefined})
})

test('RegistrationInstallation_OriginalAbortCannotBeReplaced_013',async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f.held=f.api.deferred();f.options.session.completed=f.held.promise;f.owner=f.api.createBrowserRegistrationInstallation(f.options);f.options.signal=new AbortController().signal;f.options.session.completed=Promise.resolve();f.state='pending';f.promise=f.owner.install().then(()=>{f.state='installed'},()=>{f.state='rejected'});f.controller.abort()})
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await new Promise(r=>setTimeout(r,150));return{state:f.state,status:await f.store.status()}})).toEqual({state:'pending',status:undefined})
 await page.evaluate(()=>Reflect.get(globalThis,'f').held.resolve());await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').state)).toBe('rejected')
})

test('RegistrationInstallation_AbortedOriginalCommitCannotReportInstalled_014',async({page,context})=>{
 await prepare(page,context)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f'),put=IDBObjectStore.prototype.put;let attempts=0;IDBObjectStore.prototype.put=function(...args){attempts++;const request=Reflect.apply(put,this,args);request.addEventListener('success',()=>this.transaction.abort());return request};try{const owner=f.api.createBrowserRegistrationInstallation(f.options),rejected=await owner.install().then(()=>false,()=>true);await owner.waitClosed();return{rejected,attempts,status:await f.store.status()}}finally{IDBObjectStore.prototype.put=put}})).toEqual({rejected:true,attempts:1,status:undefined})
})
