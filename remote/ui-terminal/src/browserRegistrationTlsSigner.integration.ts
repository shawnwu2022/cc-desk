import {test} from 'node:test';
import assert from 'node:assert/strict';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
import {createPublicKey,verify} from 'node:crypto';
import {createBrowserRegistrationTlsSignerBroker} from './browserTlsSigner.ts';

// PUBLIC TEST_ONLY scalar 1；只有内存内已知测试 key，不创建 credential 或登记 profile。
const b64=(hex:string)=>Buffer.from(hex,'hex').toString('base64url');
const jwk={kty:'EC',crv:'P-256',x:b64('6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296'),y:b64('4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5'),d:b64('00'.repeat(31)+'01')};
const publicKey=createPublicKey({key:{kty:jwk.kty,crv:jwk.crv,x:jwk.x,y:jwk.y},format:'jwk'});
const message=()=>new Uint8Array([...new Uint8Array(64).fill(32),...new TextEncoder().encode('TLS 1.3, client CertificateVerify'),0,...new Uint8Array(32).fill(7)]);
function platform(){
 const saved=new Map<string,PropertyDescriptor|undefined>();
 class WindowFixture{static [Symbol.hasInstance](value:unknown){return value===globalThis;}}
 for(const[name,value]of Object.entries({Window:WindowFixture,isSecureContext:true,crossOriginIsolated:true,location:{origin:'https://desk.invalid'},self:globalThis,top:globalThis})){
  saved.set(name,Object.getOwnPropertyDescriptor(globalThis,name));Object.defineProperty(globalThis,name,{configurable:true,value});
 }
 return ()=>{for(const[name,d]of saved){if(d)Object.defineProperty(globalThis,name,d);else Reflect.deleteProperty(globalThis,name);}};
}
async function fixture(budget=3000){
 const controller=new AbortController();
 const key=await crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 const setupDeadline=performance.timeOrigin+performance.now()+budget;
 const broker=createBrowserRegistrationTlsSignerBroker(key,'https://desk.invalid',{setupDeadline,signal:controller.signal});
 const worker=new Worker(new URL('./browserTlsSigner.worker.fixture.ts',import.meta.url),{workerData:{bridge:broker.worker},transferList:[broker.worker.port as never]});
 const [ready]=await once(worker,'message');assert.equal(ready.ready,true);
 return {controller,broker,worker,async dispose(){broker.close();await broker.waitClosed();await worker.terminate();}};
}
async function reply(worker:Worker,command:unknown){const response=once(worker,'message');worker.postMessage(command);return (await response)[0];}
const pause=()=>new Promise<void>(resolve=>setImmediate(resolve));

test('RegistrationSigner_ActualWorkerExistingKeyDer_001',async()=>{
 const restore=platform(),f=await fixture();
 try{
  assert.match((await reply(f.worker,{kind:'duplicateAttach'})).error,/reused/i);
  for(let i=0;i<2;i++){
   const bytes=message(),result=await reply(f.worker,{message:bytes});
   assert.equal(result.error,undefined);assert.equal(verify('sha256',bytes,publicKey,result.signature),true);
  }
  f.controller.abort();await f.broker.waitClosed();
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);
 }finally{await f.dispose();restore();}
});

test('RegistrationSigner_RejectGenericAndServerSigning_002',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;let calls=0;
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{calls++;return original.apply(subtle,args);};
 try{
  const server=message();server.set(new TextEncoder().encode('server'),64+9);
  for(const bytes of [new Uint8Array([1,2]),server,new Uint8Array(10000)]){
   const f=await fixture();try{
    assert.match((await reply(f.worker,{message:bytes})).error,/domain|bounds/i);
    await f.broker.waitClosed();assert.equal(calls,0);
   }finally{await f.dispose();}
  }
 }finally{subtle.sign=original;restore();}
});

for(const cause of ['abort','deadline'] as const)test(`RegistrationSigner_HeldCrypto_${cause}_003`,async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;
 let release!:()=>void,started!:()=>void,calls=0;
 const began=new Promise<void>(resolve=>{started=resolve;});
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{
  calls++;const actual=original.apply(subtle,args);
  const held=new Promise<ArrayBuffer>((resolve,reject)=>{release=()=>{void actual.then(resolve,reject);};});started();return held;
 };
 const f=await fixture(cause==='deadline'?500:3000);
 try{
  const response=once(f.worker,'message');f.worker.postMessage({message:message()});await began;
  if(cause==='abort')f.controller.abort();
  assert.match((await response)[0].error,/closed|timeout/i);
  let retired=false;const closed=f.broker.waitClosed().then(()=>{retired=true;});await pause();assert.equal(retired,false);
  release();await closed;assert.equal(calls,1);
  assert.equal(new Uint8Array(f.broker.worker.shared,20).some(v=>v!==0),false);
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);
 }finally{release?.();await f.dispose();subtle.sign=original;restore();}
});

test('RegistrationSigner_OriginalDeadlineSurvivesSuccessfulSign_004',async()=>{
 const restore=platform(),f=await fixture(600);
 try{
  assert.ok((await reply(f.worker,{message:message()})).signature);
  await new Promise(resolve=>setTimeout(resolve,Math.max(0,f.broker.worker.setupDeadline!-performance.timeOrigin-performance.now()+20)));
  assert.equal(Atomics.load(new Int32Array(f.broker.worker.shared),0),0);
  assert.match((await reply(f.worker,{message:message()})).error,/setup timeout/i);
  await f.broker.waitClosed();
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);
 }finally{await f.dispose();restore();}
});
