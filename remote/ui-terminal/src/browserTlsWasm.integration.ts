import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
import {createPublicKey,verify} from 'node:crypto';
import {createBrowserTlsSignerBroker} from './browserTlsSigner.ts';
const wasm=readFileSync(process.env.CC_DESK_TEST_ONLY_TLS_WASM??'MISSING_EXPLICIT_TEST_ONLY_WASM');
const glueUrl=pathToFileURL(process.env.CC_DESK_TEST_ONLY_TLS_WASM_GLUE??'MISSING_EXPLICIT_TEST_ONLY_WASM_GLUE').href;
const hex=(value:string)=>Buffer.from(value,'hex');
const point='6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5';
const spki=hex('3059301306072a8648ce3d020106082a8648ce3d03010703420004'+point);
const publicKey=createPublicKey({key:spki,format:'der',type:'spki'});
const message=Buffer.concat([Buffer.alloc(64,32),Buffer.from('TLS 1.3, client CertificateVerify\0'),Buffer.alloc(32,7)]);
function platform(){
 const saved=new Map<string,PropertyDescriptor|undefined>();
 class WindowFixture {static [Symbol.hasInstance](v:unknown){return v===globalThis;}}
 for(const [name,value]of Object.entries({Window:WindowFixture,isSecureContext:true,crossOriginIsolated:true,location:{origin:'https://desk.invalid'},self:globalThis,top:globalThis})){
  saved.set(name,Object.getOwnPropertyDescriptor(globalThis,name));Object.defineProperty(globalThis,name,{configurable:true,value});
 }
 return ()=>{for(const [n,d]of saved){if(d)Object.defineProperty(globalThis,n,d);else Reflect.deleteProperty(globalThis,n);}};
}
async function fixture(timeoutMs=2000,publicSpki:Uint8Array=spki,fault?:string){
 // PUBLIC TEST_ONLY scalar1 导入非导出 key；不创建真实身份。
 const jwk={kty:'EC',crv:'P-256',x:hex(point.slice(0,64)).toString('base64url'),y:hex(point.slice(64)).toString('base64url'),d:hex('00'.repeat(31)+'01').toString('base64url')};
 const key=()=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 const setupDeadline=performance.timeOrigin+performance.now()+timeoutMs;
 const broker=createBrowserTlsSignerBroker({origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:new Uint8Array([7]),tlsKey:await key(),grantKey:await key()},{timeoutMs,setupDeadline});
 const worker=new Worker(new URL('./browserTlsSigner.worker.fixture.ts',import.meta.url),{workerData:{wasm,glueUrl,fault,spki:publicSpki,bridge:broker.worker},transferList:[broker.worker.port as never]});
 const [ready]=await once(worker,'message');
 return {broker,worker,ready,setupDeadline,async dispose(){broker.close();await broker.waitClosed();await worker.terminate();}};
}
async function reply(worker:Worker,command:unknown){const result=once(worker,'message');worker.postMessage(command);return (await result)[0];}
test('BrowserTlsWasm_ActualWasmOriginalIdentityAndDer_001',async()=>{
 const restore=platform(),f=await fixture();
 try{
  assert.equal(f.ready.ready,true,JSON.stringify(f.ready));
  assert.ok(f.ready.imports.every((v:{module:string})=>['cc_desk_browser_tls_v1','./fixture_bg.js'].includes(v.module)));
  assert.equal((await reply(f.worker,{kind:'init'})).result,1);
  assert.match((await reply(f.worker,{kind:'duplicateFactory'})).error,/owner already claimed/);
  const result=await reply(f.worker,{kind:'sign'});assert.ok(result.result>=8,JSON.stringify(result));
  assert.equal(verify('sha256',message,publicKey,result.signature),true);
  assert.equal((await reply(f.worker,{kind:'grow'})).changed,true);
  const again=await reply(f.worker,{kind:'sign'});assert.equal(verify('sha256',message,publicKey,again.signature),true);
 }finally{await f.dispose();restore();}
});

test('BrowserTlsWasm_RejectInvalidDomainAndAbiFences_002',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;let calls=0;
 subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{calls++;return original.apply(subtle,args);};
 try{
  for(const kind of [1,2,3]){
   const f=await fixture();try{assert.equal((await reply(f.worker,{kind:'init'})).result,1);assert.equal((await reply(f.worker,{case:kind})).result,-1);assert.equal((await reply(f.worker,{})).result,-1);}finally{await f.dispose();}
  }
  for(const fault of ['sequence','pointer','capacity','overlap','reenter']){
   const f=await fixture(2000,spki,fault);try{assert.equal((await reply(f.worker,{kind:'init'})).result,1);assert.equal((await reply(f.worker,{})).result,-1);}finally{await f.dispose();}
  }
  assert.equal(calls,0);
 }finally{subtle.sign=original;restore();}
});

test('BrowserTlsWasm_CanonicalIdentityAndReturnedSignatureChecked_003',async()=>{
 const restore=platform();
 try{
  for(const fault of ['length','signature']){
   const f=await fixture(2000,spki,fault);try{assert.equal((await reply(f.worker,{kind:'init'})).result,1);assert.equal((await reply(f.worker,{})).result,-1);assert.equal((await reply(f.worker,{})).result,-1);}finally{await f.dispose();}
  }
  const mismatch=await fixture();try{assert.equal((await reply(mismatch.worker,{kind:'init',wrongIdentity:1})).result,-1);}finally{await mismatch.dispose();}
  const opposite=Buffer.from(spki);opposite.set(hex('b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a'),59);
  const wrongKey=await fixture(2000,opposite);
  try{assert.equal((await reply(wrongKey.worker,{kind:'init',wrongIdentity:2})).result,1);assert.equal((await reply(wrongKey.worker,{})).result,-1);}finally{await wrongKey.dispose();}
 }finally{restore();}
});

test('BrowserTlsWasm_RejectReboundAndSharedMemory_004',async()=>{
 const restore=platform();
 try{
  for(const shared of [false,true]){
   const f=await fixture();
   try{assert.equal((await reply(f.worker,{kind:'init'})).result,1);await reply(f.worker,{kind:'swapMemory',shared});assert.equal((await reply(f.worker,{})).result,-1);}finally{await f.dispose();}
  }
  const f=await fixture();try{await reply(f.worker,{kind:'swapMemory',shared:true});assert.equal((await reply(f.worker,{kind:'init'})).result,-1);}finally{await f.dispose();}
 }finally{restore();}
});

test('BrowserTlsWasm_RejectChangedOrNonfiniteSetupDeadline_005',async()=>{
 const restore=platform();
 try{
  for(const value of [NaN,Infinity,0,'extend']){
   const f=await fixture();try{assert.equal((await reply(f.worker,{kind:'init',deadline:value==='extend'?f.setupDeadline+1:value})).result,-1);}finally{await f.dispose();}
  }
  const f=await fixture();try{
   assert.equal((await reply(f.worker,{kind:'init'})).result,1);
   assert.equal((await reply(f.worker,{kind:'init',deadline:f.setupDeadline+1000})).result,-1);
   assert.equal(verify('sha256',message,publicKey,(await reply(f.worker,{})).signature),true);
   await reply(f.worker,{kind:'close'});assert.equal((await reply(f.worker,{})).result,-1);
   assert.match((await reply(f.worker,{kind:'duplicateFactory'})).error,/owner already claimed/);
  }finally{await f.dispose();}
 }finally{restore();}
});

test('BrowserTlsWasm_OriginalDeadlineAndCloseRetainLateCrypto_006',async()=>{
 const restore=platform(),subtle=crypto.subtle,original=subtle.sign;
 try{
  for(const mode of ['deadline','close']){
   let release!:()=>void,started!:()=>void;
   const began=new Promise<void>(resolve=>{started=resolve;});
   subtle.sign=(...args:Parameters<SubtleCrypto['sign']>)=>{
    const actual=original.apply(subtle,args);
    const held=new Promise<ArrayBuffer>(resolve=>{release=async()=>resolve(await actual);});started();return held;
   };
   const f=await fixture(800);
   try{
    assert.equal((await reply(f.worker,{kind:'init'})).result,1);
    // 请求只剩原setup的150ms；重新给800ms将被完成时间断言捕获。
    if(mode==='deadline')await new Promise<void>(resolve=>setTimeout(resolve,Math.max(0,f.setupDeadline-(performance.timeOrigin+performance.now())-150)));
    const response=reply(f.worker,{});await began;
    if(mode==='close')f.broker.close();
    assert.equal((await response).result,-1);
    if(mode==='deadline')assert.ok(performance.timeOrigin+performance.now()<f.setupDeadline+250);
    let retired=false;const closed=f.broker.waitClosed().then(()=>{retired=true;});
    await new Promise<void>(resolve=>setImmediate(resolve));assert.equal(retired,false);
    release();await closed;assert.equal((await reply(f.worker,{})).result,-1);
   }finally{release?.();await f.dispose();}
  }
 }finally{subtle.sign=original;restore();}
});
