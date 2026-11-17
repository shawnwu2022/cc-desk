import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createBrowserDataKeepaliveRelay,type BrowserRelaySigningRequest} from './browserRelay.ts';
const clock=()=>performance.timeOrigin+performance.now();
const tunnel=new Uint8Array(16).fill(9),keyId=new Uint8Array(16).fill(7);
class Socket extends EventTarget {
 readyState=1;binaryType:BinaryType='blob';bufferedAmount=0;sent:Uint8Array[]=[];closeCalls=0;
 send(value:unknown){this.sent.push(Uint8Array.from(value as Uint8Array));this.dispatchEvent(new Event('sent'));}
 close(){this.closeCalls++;this.readyState=2;}
 receive(value:unknown){this.dispatchEvent(new MessageEvent('message',{data:value instanceof Uint8Array?value.slice().buffer:value}));}
 retire(){this.readyState=3;this.dispatchEvent(new Event('close'));}
}
function outer(payload:Uint8Array,id=tunnel){const value=new Uint8Array(24+payload.length);value.set([1,1,0,1]);value.set(id,4);new DataView(value.buffer).setUint32(20,payload.length);value.set(payload,24);return value;}
const flush=async()=>{for(let i=0;i<12;i++)await Promise.resolve();};
function time(t:any){let ms=1000,wall=1700000000000;t.mock.method(performance,'now',()=>ms);t.mock.method(Date,'now',()=>wall);t.mock.timers.enable({apis:['setTimeout']});return{async advance(n:number){ms+=n;wall+=n;t.mock.timers.tick(n);await flush();},rollback(){ms-=100;},skip(n:number){ms+=n;wall+=n;}};}
async function fixture(){const socket=new Socket(),now=Math.floor(clock()),end=now+4000;
 const {owner,activation}=createBrowserDataKeepaliveRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async()=>new Uint8Array(64)},setupDeadline:now+1000,maxDeadline:end,ioBudgetMs:2000,socket});
 socket.receive(new Uint8Array([1,0,...new Uint8Array(64)]));await flush();socket.receive(new Uint8Array([1,2]));const transport=await owner.ready;
 return{socket,owner,activation,transport,end,async dispose(){owner.close();socket.retire();await owner.waitClosed();}};
}

test('BrowserDataKeepalive_DormantThenFixedEndNoPayload_001',async(t)=>{const cl=time(t),f=await fixture();try{
 assert.equal(f.socket.sent.length,1);f.activation.activate(f.end);let readDone=false;const read=f.transport.read(1).then(()=>{readDone=true;},()=>{readDone=true;});
 for(let i=0;i<7;i++){await cl.advance(500);f.socket.receive(outer(new Uint8Array()));assert.equal(readDone,false);}
 assert.equal(f.socket.sent.length,8);for(const bytes of f.socket.sent.slice(1))assert.deepEqual(bytes,outer(new Uint8Array()));
 await cl.advance(500);await read;assert.equal(f.socket.closeCalls,1);let joined=false;void f.owner.waitClosed().then(()=>{joined=true;});await flush();assert.equal(joined,false);f.socket.retire();await f.owner.waitClosed();assert.equal(joined,true);
 }finally{await f.dispose();}});
test('BrowserDataKeepalive_FullApplicationQueueStillAcceptsEmpty_002',async(t)=>{time(t);const f=await fixture();try{f.activation.activate(f.end);for(let i=0;i<4;i++)f.socket.receive(outer(new Uint8Array([i])));for(let i=0;i<20;i++)f.socket.receive(outer(new Uint8Array()));assert.equal(f.socket.closeCalls,0);for(let i=0;i<4;i++)assert.deepEqual(await f.transport.read(1),new Uint8Array([i]));}finally{await f.dispose();}});
test('BrowserDataKeepalive_OneShotLateCrossInstanceAndClosed_003',async(t)=>{const cl=time(t);const f=await fixture(),other=await fixture();try{f.activation.activate(f.end);assert.throws(()=>f.activation.activate(f.end));assert.equal(f.socket.closeCalls,1);assert.equal(other.socket.closeCalls,0);await cl.advance(1000);assert.throws(()=>other.activation.activate(other.end));}finally{await f.dispose();await other.dispose();}});
test('BrowserDataKeepalive_BackpressureKeepsOriginalWriteCustody_004',async(t)=>{const cl=time(t),f=await fixture();try{f.activation.activate(f.end);const send=f.socket.send.bind(f.socket);f.socket.send=value=>{send(value);f.socket.bufferedAmount=24;};await cl.advance(500);assert.equal(f.socket.sent.length,2);const application=f.transport.write(new Uint8Array([1])).then(()=>true,()=>false);await assert.rejects(f.transport.write(new Uint8Array([2])));await cl.advance(500);assert.equal(f.socket.sent.length,2);f.owner.close();let joined=false;void f.owner.waitClosed().then(()=>{joined=true;});await flush();assert.equal(joined,false);f.socket.retire();await f.owner.waitClosed();assert.equal(await application,false);assert.equal(joined,true);}finally{await f.dispose();}});
test('BrowserDataKeepalive_RollbackAndUnsafeDeadlineFailClosed_005',async(t)=>{const cl=time(t),f=await fixture();try{f.activation.activate(f.end);cl.rollback();await assert.rejects(f.transport.read(1));assert.equal(f.socket.closeCalls,1);}finally{await f.dispose();}const other=await fixture();try{assert.throws(()=>other.activation.activate(Number.MAX_SAFE_INTEGER+1));assert.equal(other.socket.closeCalls,1);}finally{await other.dispose();}});
test('BrowserDataKeepalive_SkipsBusyTickNoCatchup_006',async(t)=>{const cl=time(t),f=await fixture();try{f.activation.activate(f.end);f.socket.bufferedAmount=1;await cl.advance(500);assert.equal(f.socket.sent.length,1);f.socket.bufferedAmount=0;await cl.advance(500);assert.equal(f.socket.sent.length,2);await cl.advance(2200);assert.equal(f.socket.closeCalls,1);assert.equal(f.socket.sent.length,2);}finally{await f.dispose();}});

test('BrowserDataKeepalive_DormantWriteTighteningDoesNotWaitTimer_007',async(t)=>{const cl=time(t),f=await fixture();try{f.socket.bufferedAmount=131072;const pending=f.transport.write(new Uint8Array([8]));const outcome=pending.then(()=>true,()=>false);await flush();f.activation.activate(Math.floor(clock())+100);cl.skip(200);f.socket.bufferedAmount=0;await cl.advance(5);assert.equal(await outcome,false);assert.equal(f.socket.sent.length,1);}finally{await f.dispose();}});

test('BrowserDataKeepalive_OneApplicationWaitsOriginalHeartbeat_008',async(t)=>{const cl=time(t),f=await fixture();try{f.activation.activate(f.end);const send=f.socket.send.bind(f.socket);f.socket.send=value=>{assert.equal(f.socket.bufferedAmount,0);send(value);f.socket.bufferedAmount=24;};await cl.advance(500);const app=f.transport.write(new Uint8Array([8]));const outcome=app.then(()=>true,()=>false);await assert.rejects(f.transport.write(new Uint8Array([9])));assert.equal(f.socket.sent.length,2);f.socket.bufferedAmount=0;await cl.advance(5);assert.equal(f.socket.sent.length,3);assert.deepEqual(f.socket.sent[2],outer(new Uint8Array([8])));f.socket.bufferedAmount=0;await cl.advance(5);assert.equal(await outcome,true);assert.equal(f.socket.closeCalls,0);}finally{await f.dispose();}});

test('BrowserDataKeepalive_QueuedApplicationCannotOutliveFixedEnd_009',async(t)=>{const cl=time(t),f=await fixture();try{f.activation.activate(Math.floor(clock())+750);const send=f.socket.send.bind(f.socket);f.socket.send=value=>{send(value);f.socket.bufferedAmount=24;};await cl.advance(500);const outcome=f.transport.write(new Uint8Array([8])).then(()=>true,()=>false);await cl.advance(250);assert.equal(await outcome,false);assert.equal(f.socket.closeCalls,1);assert.equal(f.socket.sent.length,2);f.socket.bufferedAmount=0;await cl.advance(500);assert.equal(f.socket.sent.length,2);}finally{await f.dispose();}});
