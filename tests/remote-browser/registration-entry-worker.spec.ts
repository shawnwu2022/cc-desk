import {test,expect} from '@playwright/test'
import {build} from 'esbuild'
import {createECDH} from 'node:crypto'
let pageModule:string,workerModule:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 const jwks=[1,2,3].map(n=>{const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();return{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')}})
 pageModule=(await build({stdin:{resolveDir:process.cwd(),contents:`
 import {createBrowserRegistrationBridge} from './remote/ui-terminal/src/browserRegistrationBridge.ts';
 import {serveBrowserTransport} from './remote/ui-terminal/src/browserTransport.ts';
 export async function start(mode){
  const [tlsKey,grantKey,relayKey]=await Promise.all(${JSON.stringify(jwks)}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));
  const controller=new AbortController(),now=performance.timeOrigin+performance.now(),bridge=createBrowserRegistrationBridge({origin:location.origin,keys:{tlsKey,grantKey,relayKey},signal:controller.signal,setupDeadline:now+3000,registrationDeadline:now+5000});
  const channel=new MessageChannel(),physical=new Promise(resolve=>{globalThis.releasePhysical=resolve}),wire=serveBrowserTransport(channel.port1,{read:async()=>null,write:async()=>{},close(){globalThis.closeRequested=true;if(mode!=='held')globalThis.releasePhysical()},waitClosed:()=>physical});
  const worker=new Worker('/worker.mjs',{type:'module'}),events=[],f={worker,bridge,wire,events,controller,mainRetired:false};globalThis.f=f;
  void bridge.waitClosed().then(()=>{f.mainRetired=true});worker.onmessage=e=>events.push(e.data);worker.onerror=e=>events.push({kind:'workerError',message:e.message});
  const init={kind:'start',origin:location.origin,artifacts:{wasm:location.origin+'/bad.wasm',glue:mode==='cross-origin'?'https://other.invalid/bad.js':location.origin+'/glue.js'},localSpki:new Uint8Array(91),hostSpki:new Uint8Array(91),bridge:bridge.worker,transport:channel.port2};
  worker.postMessage(init,[bridge.worker.port,bridge.worker.signer.port,channel.port2]);if(mode==='second-start')worker.postMessage({kind:'start'});return f;
 }
 `},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
 workerModule=(await build({entryPoints:['remote/ui-terminal/src/browserRegistrationEntry.worker.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
})
for(const mode of ['invalid-wasm','overflow','cross-origin','second-start','held'])test(`RegistrationEntryWorker_OriginalFailureRetires_${mode}_001`,async({page,context})=>{
 let requests=0
 await context.route('https://desk.invalid/**',async route=>{const path=new URL(route.request().url()).pathname;const headers:Record<string,string>={'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'};if(path==='/bad.wasm'){requests++;if(mode==='overflow')headers['content-length']='8388609'}await route.fulfill({headers,contentType:path.endsWith('.wasm')?'application/wasm':path.endsWith('.mjs')||path.endsWith('.js')?'text/javascript':'text/html',body:path==='/page.mjs'?pageModule:path==='/worker.mjs'?workerModule:path==='/bad.wasm'?Buffer.from([0]):path==='/glue.js'?'export function __wbg_set_wasm(){}':'<!doctype html><title>TEST_ONLY real Worker</title>'})})
 await page.goto('https://desk.invalid/');await page.evaluate(async mode=>{const api=await import(location.origin+'/page.mjs');await api.start(mode)},mode)
 if(mode==='held'){
  await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'closeRequested'))).toBe(true)
  await page.waitForTimeout(150)
  expect(await page.evaluate(()=>Reflect.get(globalThis,'f').events.some((e:{kind:string})=>e.kind==='retired'))).toBe(false)
  await page.evaluate(()=>Reflect.get(globalThis,'releasePhysical')())
 }
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').events.map((e:{kind:string})=>e.kind))).toContain('failed')
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').events)).toContainEqual({kind:'retired'})
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await Promise.all([f.bridge.waitClosed(),f.wire.waitClosed()]);f.worker.terminate();return{mainRetired:f.mainRetired,success:f.events.some((e:{kind:string})=>['outcome','completed','evidence'].includes(e.kind)),retirements:f.events.filter((e:{kind:string})=>e.kind==='retired').length}})).toEqual({mainRetired:true,success:false,retirements:1})
 expect(requests).toBeLessThanOrEqual(1)
})
