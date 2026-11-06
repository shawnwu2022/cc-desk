import {test} from 'node:test';
import assert from 'node:assert/strict';
import {Worker} from 'node:worker_threads';
import {once,on} from 'node:events';
import {createPublicKey,verify} from 'node:crypto';
import {createBrowserTlsSignerBroker,createBrowserTlsWorkerSigner} from './browserTlsSigner.ts';

const origin='https://desk.invalid';
const message=()=>new Uint8Array([...new Uint8Array(64).fill(32),...new TextEncoder().encode('TLS 1.3, client CertificateVerify'),0,...new Uint8Array(32).fill(7)]);
// PUBLIC TEST_ONLY P256 scalar1。绝不可登记为生产身份；不创建生产凭据。
const b64=(hex:string)=>Buffer.from(hex,'hex').toString('base64url');
const jwk={kty:'EC',crv:'P-256',x:b64('6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296'),y:b64('4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5'),d:b64('00'.repeat(31)+'01')};
const publicKey=createPublicKey({key:{kty:jwk.kty,crv:jwk.crv,x:jwk.x,y:jwk.y},format:'jwk'});
async function profile(){
 const importKey=()=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 return {origin,rpId:'desk.invalid',credentialId:new Uint8Array([7]),tlsKey:await importKey(),grantKey:await importKey()};
}
function platform(){
 const saved=new Map<string,PropertyDescriptor|undefined>();
 class WindowFixture {static [Symbol.hasInstance](value:unknown){return value===globalThis;}}
 for(const [name,value]of Object.entries({Window:WindowFixture,isSecureContext:true,crossOriginIsolated:true,location:{origin},self:globalThis,top:globalThis})){
  saved.set(name,Object.getOwnPropertyDescriptor(globalThis,name));Object.defineProperty(globalThis,name,{configurable:true,value});
 }
 return ()=>{for(const [name,d]of saved){if(d)Object.defineProperty(globalThis,name,d);else Reflect.deleteProperty(globalThis,name);}};
}
async function fixture(timeoutMs=2000,isolated=true,signal?:AbortSignal){
 const broker=createBrowserTlsSignerBroker(await profile(),{timeoutMs,signal});
 const worker=new Worker(new URL('./browserTlsSigner.worker.fixture.ts',import.meta.url),{workerData:{bridge:broker.worker,isolated},transferList:[broker.worker.port as never]});
 const [ready]=await once(worker,'message');
 return {broker,worker,ready,async dispose(){broker.close();await broker.waitClosed();await worker.terminate();}};
}
async function reply(worker:Worker,command:unknown){const response=once(worker,'message');worker.postMessage(command);return (await response)[0];}
const pause=()=>new Promise<void>(resolve=>setImmediate(resolve));

test('BrowserTls_WorkerRealSubtleCryptoDerAndReuse_001',async()=>{
 const restore=platform();const f=await fixture();
 try{
  assert.equal(f.ready.ready,true);
  assert.match((await reply(f.worker,{kind:'duplicateAttach'})).error,/reused/i);
  for(let i=0;i<2;i++){
   const m=message(),result=await reply(f.worker,{message:m});
   assert.equal(result.error,undefined);assert.ok(result.signature.length>=8&&result.signature.length<=72);
   assert.equal(verify('sha256',m,publicKey,result.signature),true);
   assert.equal(verify('sha256',new Uint8Array([1]),publicKey,result.signature),false);
  }
 }finally{await f.dispose();restore();}
});

test('BrowserTls_WindowIsolationAndProfileFailClosed_002',async()=>{
 const p=await profile();assert.throws(()=>createBrowserTlsSignerBroker(p,{timeoutMs:100}));
 const restore=platform();
 try{
  const broker=createBrowserTlsSignerBroker(p,{timeoutMs:100});
  assert.throws(()=>createBrowserTlsWorkerSigner(broker.worker),/worker/i);
  broker.close();await broker.waitClosed();broker.worker.port.close();
  const f=await fixture(100,false);
  try{assert.match(f.ready.error,/isolation/i);}finally{await f.dispose();}
  Object.defineProperty(globalThis,'crossOriginIsolated',{configurable:true,value:false});
  assert.throws(()=>createBrowserTlsSignerBroker(p,{timeoutMs:100}),/isolation/i);
  Object.defineProperty(globalThis,'crossOriginIsolated',{configurable:true,value:true});
  const descriptor=Object.getOwnPropertyDescriptor(globalThis,'SharedArrayBuffer')!;
  try{
   Object.defineProperty(globalThis,'SharedArrayBuffer',{configurable:true,value:undefined});
   assert.throws(()=>createBrowserTlsSignerBroker(p,{timeoutMs:100}),/isolation/i);
  }finally{Object.defineProperty(globalThis,'SharedArrayBuffer',descriptor);}
  for(const timeoutMs of [0,10001,Infinity,NaN])assert.throws(()=>createBrowserTlsSignerBroker(p,{timeoutMs}));
 }finally{restore();}
});

test('BrowserTls_RejectWrongDomainAndBoundsWithoutSign_003',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;
 let calls=0;subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{calls++;return original.apply(subtle,args);};
 try{
  const server=message();server.set(new TextEncoder().encode('server'),64+9);
  for(const m of [server,new Uint8Array(10000),new Uint8Array([1,2]),message().subarray(1),new Uint8Array(131)]){
   const f=await fixture();try{assert.match((await reply(f.worker,{message:m})).error,/message|bounds/i);assert.match((await reply(f.worker,{message:message()})).error,/closed/i);}finally{await f.dispose();}
  }
  assert.equal(calls,0);
 }finally{subtle.sign=original;restore();}
});

test('BrowserTls_TimeoutBeforeBrokerEventCallsNoCrypto_004',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;let calls=0;
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{calls++;return original.apply(subtle,args);};
 const f=await fixture(50);
 try{
  const started=new SharedArrayBuffer(4),response=once(f.worker,'message');
  f.worker.postMessage({message:message(),started});
  // 只在测试线程阻塞事件队列，确保请求直到原 deadline 之后才交给 broker。
  Atomics.wait(new Int32Array(started),0,0,1000);
  const until=performance.now()+150;while(performance.now()<until){}
  assert.match((await response)[0].error,/timeout/i);await pause();assert.equal(calls,0);
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);
 }finally{await f.dispose();subtle.sign=original;restore();}
});

test('BrowserTls_CloseRetainsOriginalPromiseAndDiscardsLateResult_005',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;
 let release!:(value:ArrayBuffer)=>void,started!:()=>void,calls=0;
 const began=new Promise<void>(resolve=>{started=resolve;});
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{
  calls++;const actual=original.apply(subtle,args);
  const held=new Promise<ArrayBuffer>(resolve=>{release=async()=>resolve(await actual);});started();return held;
 };
 const f=await fixture();
 try{
  const response=once(f.worker,'message');f.worker.postMessage({message:message()});await began;
  f.broker.close();let retired=false;const closed=f.broker.waitClosed().then(()=>{retired=true;});await pause();assert.equal(retired,false);
  assert.match((await response)[0].error,/closed/i);release(new ArrayBuffer(0));await closed;assert.equal(retired,true);
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);assert.equal(calls,1);
 }finally{release?.(new ArrayBuffer(0));await f.dispose();subtle.sign=original;restore();}
});

test('BrowserTls_TimeoutRetainsPromiseAndNoLateReuse_006',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;
 let release!:()=>void,started!:()=>void,calls=0;
 const began=new Promise<void>(resolve=>{started=resolve;});
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{
  calls++;const actual=original.apply(subtle,args);
  const held=new Promise<ArrayBuffer>(resolve=>{release=async()=>resolve(await actual);});started();return held;
 };
 const f=await fixture(80);
 try{
  const response=once(f.worker,'message');f.worker.postMessage({message:message()});await began;
  assert.match((await response)[0].error,/timeout|closed/i);let retired=false;const closed=f.broker.waitClosed().then(()=>{retired=true;});await pause();assert.equal(retired,false);
  release();await closed;assert.equal(retired,true);assert.equal(calls,1);
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);
 }finally{release?.();await f.dispose();subtle.sign=original;restore();}
});

test('BrowserTls_WrongSequenceDuplicateAndOversizedBrokerFrames_007',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;let calls=0;
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{calls++;return original.apply(subtle,args);};
 try{
  for(const command of [{seq:2,message:message()},{seq:1,message:new Uint8Array(10000)},{seq:1,message:message(),override:{generation:'wrong'}},{seq:1,message:message(),override:{extra:1}},{seq:1,message:message(),override:{deadline:Infinity}},{seq:1,message:message(),override:{deadline:0}},{seq:1,message:message(),override:{deadline:performance.timeOrigin+performance.now()+60000}}]){
   const f=await fixture();try{f.worker.postMessage({kind:'raw',...command});await f.broker.waitClosed();assert.equal(calls,0);}finally{await f.dispose();}
  }
  const f=await fixture();try{
   assert.ok((await reply(f.worker,{message:message()})).signature);assert.equal(calls,1);
   f.worker.postMessage({kind:'raw',seq:1,message:message()});await f.broker.waitClosed();assert.equal(calls,1);
  }finally{await f.dispose();}
 }finally{subtle.sign=original;restore();}
});

test('BrowserTls_ConcurrentRequestsCloseWithoutQueue_008',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;let calls=0,release!:()=>void,started!:()=>void;
 const began=new Promise<void>(resolve=>{started=resolve;});
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{
  calls++;const actual=original.apply(subtle,args);
  const held=new Promise<ArrayBuffer>(resolve=>{release=async()=>resolve(await actual);});started();return held;
 };
 const f=await fixture();
 try{
  f.worker.postMessage({kind:'raw',seq:1,message:message()});await began;
  f.worker.postMessage({kind:'raw',seq:2,message:message()});
  // worker 的后续命令提供一次跨线程往返，第二帧已发出后才观察关闭标记。
  await reply(f.worker,{kind:'duplicateAttach'});
  const until=performance.now()+2000;
  while(Atomics.load(new Int32Array(f.broker.worker.shared),0)===0&&performance.now()<until)await pause();
  assert.equal(Atomics.load(new Int32Array(f.broker.worker.shared),0),1);
  let retired=false;const closed=f.broker.waitClosed().then(()=>{retired=true;});await pause();assert.equal(retired,false);
  release();await closed;assert.equal(calls,1);
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);
 }finally{release?.();await f.dispose();subtle.sign=original;restore();}
});

test('BrowserTls_CancelCopiesSharedInputAndDropsResult_009',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign,abort=new AbortController();
 let calls=0,release!:()=>void,started!:()=>void,signed:Uint8Array|undefined;
 const began=new Promise<void>(resolve=>{started=resolve;});
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{
  calls++;signed=new Uint8Array(args[2] as ArrayBuffer).slice();const actual=original.apply(subtle,args);
  const held=new Promise<ArrayBuffer>(resolve=>{release=async()=>resolve(await actual);});started();return held;
 };
 const f=await fixture(2000,true,abort.signal);
 try{
  const source=new Uint8Array(new SharedArrayBuffer(message().length));source.set(message());
  const response=once(f.worker,'message');f.worker.postMessage({message:source});await began;
  source.fill(0);assert.deepEqual(signed,message());abort.abort();
  assert.match((await response)[0].error,/closed/i);
  let retired=false;const closed=f.broker.waitClosed().then(()=>{retired=true;});await pause();assert.equal(retired,false);
  release();await closed;assert.equal(calls,1);
  assert.match((await reply(f.worker,{message:message()})).error,/closed/i);
 }finally{release?.();await f.dispose();subtle.sign=original;restore();}
});

test('BrowserTls_InvalidCryptoOutputNeverBecomesSignature_010',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;
 try{
  for(const raw of [new ArrayBuffer(63),new ArrayBuffer(64),new Uint8Array(64).fill(255).buffer]){
   subtle.sign=async()=>raw;const f=await fixture();
   try{assert.match((await reply(f.worker,{message:message()})).error,/closed/i);await f.broker.waitClosed();}finally{await f.dispose();}
  }
 }finally{subtle.sign=original;restore();}
});

test('BrowserTls_CloseBeforeNativeWaitCannotLoseWake_011',async(t)=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;
 let release!:()=>void,started!:()=>void;
 const began=new Promise<void>(resolve=>{started=resolve;});
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{
  const actual=original.apply(subtle,args);
  const held=new Promise<ArrayBuffer>(resolve=>{release=async()=>resolve(await actual);});started();return held;
 };
 const f=await fixture(600),gate=new Int32Array(new SharedArrayBuffer(4));
 const events=on(f.worker,'message',{signal:AbortSignal.timeout(5000)});
 try{
  f.worker.postMessage({message:message(),waitGate:gate.buffer});
  const before=(await events.next()).value![0];assert.equal(before.kind,'beforeWait');await began;
  f.broker.close();let retired=false;const closed=f.broker.waitClosed().then(()=>{retired=true;});
  await pause();assert.equal(retired,false);release();await closed;await pause();
  // 所有 broker 关闭/settle 通知已结束后才放行，不能靠后续 notify 偶然补醒。
  Atomics.store(gate,0,1);Atomics.notify(gate,0);
  const result=(await events.next()).value![0];t.diagnostic(JSON.stringify(result));assert.equal(result.kind,'waitResult');
  assert.equal(result.outcome,'not-equal');assert.ok(result.elapsedMs<result.timeoutMs/2);
  assert.match((await events.next()).value![0].error,/closed/i);
 }finally{
  Atomics.store(gate,0,1);Atomics.notify(gate,0);release?.();await events.return!();
  await f.dispose();subtle.sign=original;restore();
 }
});
