import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createBrowserRegistrationRelay} from './browserRelay.ts';
const now=()=>performance.timeOrigin+performance.now(),pause=(ms:number)=>new Promise(resolve=>setTimeout(resolve,ms));
const tunnel=new Uint8Array(16).fill(9),keyId=new Uint8Array(16).fill(7);
class Socket extends EventTarget {
 readyState=1;binaryType:BinaryType='blob';bufferedAmount=0;sent:Uint8Array[]=[];peer:Socket|undefined;
 send(value:unknown){const b=Uint8Array.from(value as Uint8Array);this.sent.push(b);if(b.length>=24&&b[0]===1&&b[1]===1&&b[2]===0&&b[3]===1)queueMicrotask(()=>this.peer?.receive(b));}
 receive(b:Uint8Array){this.dispatchEvent(new MessageEvent('message',{data:b.slice().buffer}));}
 close(){this.readyState=2;}
 retire(){this.readyState=3;this.dispatchEvent(new Event('close'));}
}
async function pair(total=2500,budget=1000){
 const end=now()+total,controller=new AbortController(),a=new Socket(),b=new Socket();a.peer=b;b.peer=a;
 const create=(socket:Socket)=>createBrowserRegistrationRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async()=>new Uint8Array(64).fill(5)},setupDeadline:now()+1000,registrationDeadline:end,ioBudgetMs:budget,socket,signal:controller.signal});
 const one=create(a),two=create(b);a.receive(new Uint8Array([1,0,...new Uint8Array(64)]));b.receive(new Uint8Array([1,0,...new Uint8Array(64)]));await pause(0);a.receive(new Uint8Array([1,2]));b.receive(new Uint8Array([1,2]));
 const x=await one.ready,y=await two.ready;
 return{a,b,one,two,x,y,end,controller,async dispose(){one.close();two.close();a.retire();b.retire();await Promise.all([one.waitClosed(),two.waitClosed()]);}};
}
test('RegistrationRelay_IdleKeepaliveHasNoTlsPayloadAndFixedEnd_001',async()=>{
 const f=await pair();try{
  let got:Uint8Array|null|undefined;const read=f.y.read(20).then(v=>{got=v;});
  await pause(1250);assert.equal(got,undefined);assert.equal(f.a.readyState,1);assert.equal(f.b.readyState,1);
  assert.ok(f.a.sent.some(b=>b.length===24));assert.ok(f.b.sent.some(b=>b.length===24));
  await f.x.write(new Uint8Array([3,4]));await read;assert.deepEqual(got,new Uint8Array([3,4]));
  await pause(Math.max(0,f.end-now()+30));assert.equal(f.a.readyState,2);assert.equal(f.b.readyState,2);
  let retired=false;void f.one.waitClosed().then(()=>{retired=true;});await pause(0);assert.equal(retired,false);
 }finally{await f.dispose();}
});
test('RegistrationRelay_OutgoingDoesNotRefreshInbound_002',async()=>{
 const f=await pair(4000);try{f.b.peer=undefined;await pause(1100);assert.equal(f.a.readyState,2);assert.equal(f.b.readyState,1);assert.ok(f.a.sent.some(b=>b.length===24));}finally{await f.dispose();}
});
test('RegistrationRelay_HeartbeatDoesNotCompeteWithOriginalWrite_003',async()=>{
 const f=await pair();try{
  f.a.bufferedAmount=131072;let done=false;const write=f.x.write(new Uint8Array([8,9])).then(()=>{done=true;});
  await pause(400);assert.equal(done,false);assert.equal(f.a.sent.length,1);
  f.a.bufferedAmount=0;await write;assert.deepEqual(await f.y.read(2),new Uint8Array([8,9]));
 }finally{await f.dispose();}
});
test('RegistrationRelay_AbortPreventsLaterHeartbeats_004',async()=>{
 const f=await pair();try{f.controller.abort();await pause(400);assert.equal(f.a.sent.length,1);assert.equal(f.b.sent.length,1);assert.equal(f.a.readyState,2);assert.equal(f.b.readyState,2);}finally{await f.dispose();}
});
test('RegistrationRelay_InvalidOriginalLifetimeBeforeSigning_005',()=>{
 for(const value of [NaN,Infinity,now()-1,now()+121000]){
  const socket=new Socket();let calls=0;
  assert.throws(()=>createBrowserRegistrationRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async()=>{calls++;return new Uint8Array(64);}},setupDeadline:now()+1000,registrationDeadline:value,ioBudgetMs:1000,socket,signal:new AbortController().signal}));assert.equal(calls,0);assert.equal(socket.sent.length,0);
 }
 const controller=new AbortController();controller.abort();
 assert.throws(()=>createBrowserRegistrationRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async()=>new Uint8Array(64)},setupDeadline:now()+1000,registrationDeadline:now()+1000,ioBudgetMs:1000,socket:new Socket(),signal:controller.signal}));
});
test('RegistrationRelay_TotalEndRetainsOriginalSignerAndSocket_006',async()=>{
 const socket=new Socket(),controller=new AbortController();let release!:(value:Uint8Array)=>void,original:AbortSignal|undefined;
 const owner=createBrowserRegistrationRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async(_r,s)=>{original=s;return new Promise(resolve=>{release=resolve;});}},setupDeadline:now()+1000,registrationDeadline:now()+100,ioBudgetMs:1000,socket,signal:controller.signal});void owner.ready.catch(()=>{});
 socket.receive(new Uint8Array([1,0,...new Uint8Array(64)]));await pause(150);assert.equal(original?.aborted,true);assert.equal(socket.readyState,2);
 let retired=false;void owner.waitClosed().then(()=>{retired=true;});socket.retire();await pause(0);assert.equal(retired,false);
 release(new Uint8Array(64));await owner.waitClosed();assert.equal(socket.sent.length,0);
});
test('RegistrationRelay_WallSuspendCannotExtendTotalDeadline_007',async()=>{
 const f=await pair(5000),original=Date.now;try{Date.now=()=>original()+10000;await assert.rejects(f.x.read(1));assert.equal(f.a.readyState,2);}finally{Date.now=original;await f.dispose();}
});
test('RegistrationRelay_HeartbeatSendFailureCloses_008',async()=>{
 const f=await pair();try{f.a.send=()=>{throw Error('TEST_ONLY socket send');};await pause(400);assert.equal(f.a.readyState,2);}finally{await f.dispose();}
});
test('RegistrationRelay_TransientClockFaultCannotBeSwallowedByWriteCap_009',async()=>{
 const f=await pair(),original=Date.now;let first=true;
 try{Date.now=()=>first?(first=false,original()-5000):original();await assert.rejects(f.x.write(new Uint8Array([42])));assert.equal(f.a.readyState,2);assert.equal(f.a.sent.length,1);}finally{Date.now=original;await f.dispose();}
});
