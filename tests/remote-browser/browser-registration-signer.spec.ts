import {test,expect,type BrowserContext,type Page} from '@playwright/test'
import {build} from 'esbuild'
import {createPublicKey,verify} from 'node:crypto'

// 公开 scalar 1 只在测试内存中导入，不调用真实凭据、存储或注册 API。
const b64=(hex:string)=>Buffer.from(hex,'hex').toString('base64url')
const jwk={kty:'EC',crv:'P-256',x:b64('6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296'),y:b64('4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5'),d:b64('00'.repeat(31)+'01')}
const publicKey=createPublicKey({key:{kty:jwk.kty,crv:jwk.crv,x:jwk.x,y:jwk.y},format:'jwk'})
let module:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{module=(await build({entryPoints:['remote/ui-terminal/src/browserTlsSigner.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text})
const worker=`import {createBrowserTlsWorkerSigner} from '/signer.mjs';
onmessage=event=>{
 try{
  const signer=createBrowserTlsWorkerSigner(event.data);
  onmessage=event=>{try{postMessage({signature:Array.from(signer.sign(new Uint8Array(event.data)))});}catch(error){postMessage({error:error.message});}};
  postMessage({ready:true});
 }catch(error){postMessage({error:error.message});}
};`
const fixture=`import {createBrowserRegistrationTlsSignerBroker} from '/signer.mjs';
export async function setup(){
 const key=await crypto.subtle.importKey('jwk',${JSON.stringify(jwk)},{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 const f={key,calls:0,generated:0,credentials:0,originalKey:true,hold:false,retired:false};globalThis.f=f;
 crypto.subtle.generateKey=()=>{f.generated++;throw Error('TEST_ONLY generation forbidden');};
 for(const method of ['create','get'])Object.defineProperty(navigator.credentials,method,{configurable:true,value:()=>{f.credentials++;throw Error('TEST_ONLY credential forbidden');}});
 const original=crypto.subtle.sign.bind(crypto.subtle);
 crypto.subtle.sign=(algorithm,used,bytes)=>{f.calls++;f.originalKey&&=used===key;const actual=original(algorithm,used,bytes);return f.hold?new Promise((resolve,reject)=>{f.release=()=>actual.then(resolve,reject);}):actual;};
 f.message=()=>new Uint8Array([...new Uint8Array(64).fill(32),...new TextEncoder().encode('TLS 1.3, client CertificateVerify'),0,...new Uint8Array(32).fill(7)]);
 f.start=async budget=>{
  f.controller=new AbortController();f.deadline=performance.timeOrigin+performance.now()+budget;
  f.broker=createBrowserRegistrationTlsSignerBroker(key,location.origin,{setupDeadline:f.deadline,signal:f.controller.signal});
  f.worker=new Worker('/worker.mjs',{type:'module'});
  const ready=new Promise((resolve,reject)=>{f.worker.onmessage=e=>e.data.ready?resolve():reject(Error(e.data.error));f.worker.onerror=reject;});
  f.worker.postMessage(f.broker.worker,[f.broker.worker.port]);await ready;
  void f.broker.waitClosed().then(()=>{f.retired=true;});
 };
 f.sign=bytes=>new Promise(resolve=>{f.worker.onmessage=e=>resolve(e.data);f.worker.postMessage(Array.from(bytes));});
 f.dispose=async()=>{f.broker.close();await f.broker.waitClosed();f.worker.terminate();};
 return f;
}`
async function prepare(page:Page,context:BrowserContext,isolated=true){
 await context.route('https://desk.invalid/**',r=>{const p=new URL(r.request().url()).pathname;return r.fulfill({headers:isolated?{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'}:{},contentType:p.endsWith('.mjs')?'text/javascript':'text/html',body:p==='/signer.mjs'?module:p==='/worker.mjs'?worker:p==='/fixture.mjs'?fixture:'<!doctype html><title>TEST_ONLY registration signer</title>'})})
 await page.goto('https://desk.invalid/')
 await page.evaluate(async()=>{const api=await import(location.origin+'/fixture.mjs');await api.setup()})
}

test('RegistrationSigner_EdgeActualWorkerDer_001',async({page,context})=>{
 await prepare(page,context)
 const result=await page.evaluate(async()=>{
  const f=Reflect.get(globalThis,'f');await f.start(5000)
  try{const bytes=f.message(),signed=await f.sign(bytes);return {bytes:Array.from(bytes) as number[],...signed,isolated:crossOriginIsolated,secure:isSecureContext,extractable:f.key.extractable,originalKey:f.originalKey,calls:f.calls,generated:f.generated,credentials:f.credentials,exactDeadline:f.broker.worker.setupDeadline===f.deadline}}
  finally{await f.dispose()}
 })
 expect(result).toMatchObject({isolated:true,secure:true,extractable:false,originalKey:true,calls:1,generated:0,credentials:0,exactDeadline:true})
 expect(verify('sha256',Buffer.from(result.bytes),publicKey,Buffer.from(result.signature))).toBe(true)
})

for(const kind of ['generic','server'] as const)test(`RegistrationSigner_EdgeReject_${kind}_002`,async({page,context})=>{
 await prepare(page,context)
 const result=await page.evaluate(async kind=>{
  const f=Reflect.get(globalThis,'f');await f.start(5000)
  try{const bytes=kind==='generic'?new Uint8Array([1]):f.message();if(kind==='server')bytes.set(new TextEncoder().encode('server'),73);const result=await f.sign(bytes);await f.broker.waitClosed();return{...result,calls:f.calls,generated:f.generated,credentials:f.credentials}}
  finally{await f.dispose()}
 },kind)
 expect(result.error).toMatch(/bounds|domain/);expect(result).toMatchObject({calls:0,generated:0,credentials:0})
})

for(const cause of ['abort','deadline'] as const)test(`RegistrationSigner_EdgeHeldCrypto_${cause}_003`,async({page,context})=>{
 await prepare(page,context)
 await page.evaluate(async cause=>{const f=Reflect.get(globalThis,'f');f.hold=true;await f.start(cause==='deadline'?1500:5000);f.work=f.sign(f.message()).then((result:unknown)=>{f.result=result})},cause)
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').release)).toBe('function')
 if(cause==='abort')await page.evaluate(()=>Reflect.get(globalThis,'f').controller.abort())
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').result?.error)).toMatch(/closed|timeout/)
 expect(await page.evaluate(()=>Reflect.get(globalThis,'f').retired)).toBe(false)
 await page.evaluate(()=>Reflect.get(globalThis,'f').release())
 await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').retired)).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');const repeated=await f.sign(f.message());await f.dispose();return{error:repeated.error,calls:f.calls,generated:f.generated,credentials:f.credentials,outputZero:new Uint8Array(f.broker.worker.shared,20).every(v=>v===0)}})).toMatchObject({error:expect.stringMatching(/closed/),calls:1,generated:0,credentials:0,outputZero:true})
})

test('RegistrationSigner_EdgeNoIsolationFailClosed_004',async({page,context})=>{
 await prepare(page,context,false)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{rejected:await f.start(3000).then(()=>false,(error:Error)=>/isolation/.test(error.message)),calls:f.calls,isolated:crossOriginIsolated}})).toEqual({rejected:true,calls:0,isolated:false})
})
