import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createBrowserRelay,type BrowserRelaySigningRequest} from './browserRelay.ts';
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
const tick=()=>new Promise<void>(resolve=>setImmediate(resolve));
async function admitted(sign?:(r:BrowserRelaySigningRequest,s:AbortSignal)=>Promise<Uint8Array>){
 const socket=new Socket(),requests:BrowserRelaySigningRequest[]=[];
 const relay=createBrowserRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:sign??(async r=>{requests.push(r);return new Uint8Array(64).fill(5);})},setupDeadline:clock()+1000,ioBudgetMs:1000,socket});
 void relay.ready.catch(()=>{});socket.receive(new Uint8Array([1,0,...new Uint8Array(64).fill(3)]));await tick();
 socket.receive(new Uint8Array([1,2]));const transport=await relay.ready;
 return{socket,relay,transport,requests,dispose:async()=>{relay.close();socket.retire();await relay.waitClosed();}};
}
test('BrowserRelay_OriginalAdmissionTranscriptAndProof_001',async()=>{
 const f=await admitted();try{assert.equal(f.socket.binaryType,'arraybuffer');assert.equal(f.requests.length,1);assert.equal(f.requests[0].role,2);assert.equal(f.requests[0].origin,'wss://relay.invalid/route');
 const origin=new TextEncoder().encode(f.requests[0].origin),prefix=new TextEncoder().encode('CC_DESK_RELAY_ADMISSION\0\x01');
 assert.deepEqual(f.requests[0].transcript,new Uint8Array([...prefix,0,origin.length,...origin,...new Uint8Array(64).fill(3),...keyId,2]));
 assert.deepEqual(f.socket.sent,[new Uint8Array([1,1,...keyId,2,...new Uint8Array(64).fill(5)])]);
 }finally{await f.dispose();}
});
test('BrowserRelay_OriginalMuxBidirectionalAndPartialReads_002',async()=>{
 const f=await admitted();try{f.socket.receive(outer(new Uint8Array([4,5,6])));assert.deepEqual(await f.transport.read(2),new Uint8Array([4,5]));assert.deepEqual(await f.transport.read(2),new Uint8Array([6]));await f.transport.write(new Uint8Array([8,9]));assert.deepEqual(f.socket.sent[1],outer(new Uint8Array([8,9])));}finally{await f.dispose();}
});
for(const [name,value]of Object.entries({Text:'x',WrongTunnel:outer(new Uint8Array([1]),new Uint8Array(16).fill(8)),Trailing:new Uint8Array([...outer(new Uint8Array()),0]),Oversize:new Uint8Array(65537),Duplicate:new Uint8Array([1,1,0,2,...outer(new Uint8Array()).subarray(4),...outer(new Uint8Array()).subarray(4)])}))test('BrowserRelay_Rejects_'+name,async()=>{
 const f=await admitted();try{f.socket.receive(value);await assert.rejects(f.transport.read(8));assert.equal(f.socket.closeCalls,1);}finally{await f.dispose();}
});
test('BrowserRelay_BoundedQueueAndSingleRead_003',async()=>{
 const f=await admitted();try{const first=f.transport.read(1);await assert.rejects(f.transport.read(1));f.socket.receive(outer(new Uint8Array([1])));assert.deepEqual(await first,new Uint8Array([1]));for(let i=0;i<5;i++)f.socket.receive(outer(new Uint8Array([1])));await assert.rejects(f.transport.read(1));}finally{await f.dispose();}
});
test('BrowserRelay_CloseRetainsOriginalSignerAndSocket_004',async()=>{
 let release!:(v:Uint8Array)=>void;const socket=new Socket(),controller=new AbortController();let signal:AbortSignal|undefined;
 const relay=createBrowserRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async(_r,s)=>{signal=s;return new Promise(resolve=>{release=resolve;});}},setupDeadline:clock()+1000,ioBudgetMs:1000,socket,signal:controller.signal});void relay.ready.catch(()=>{});
 socket.receive(new Uint8Array([1,0,...new Uint8Array(64)]));await tick();controller.abort();assert.equal(signal?.aborted,true);
 let retired=false;void relay.waitClosed().then(()=>{retired=true;});socket.retire();await tick();assert.equal(retired,false);release(new Uint8Array(64));await relay.waitClosed();await assert.rejects(relay.ready);assert.equal(socket.sent.length,0);
});
test('BrowserRelay_BackpressureRetainsSingleWrite_005',async()=>{
 const f=await admitted();try{f.socket.bufferedAmount=131072;const bytes=new Uint8Array([8]),write=f.transport.write(bytes);bytes[0]=99;await assert.rejects(f.transport.write(new Uint8Array([9])));assert.equal(f.socket.sent.length,1);f.socket.bufferedAmount=0;await write;assert.deepEqual(f.socket.sent[1],outer(new Uint8Array([8])));}finally{await f.dispose();}
});
test('BrowserRelay_WriteCompletesOnlyAfterOriginalBufferDrains_006',async()=>{const f=await admitted();try{const send=f.socket.send.bind(f.socket);f.socket.send=value=>{send(value);f.socket.bufferedAmount=30;};let done=false;const write=f.transport.write(new Uint8Array([8])).then(()=>{done=true;});await tick();assert.equal(done,false);f.socket.bufferedAmount=0;await write;}finally{await f.dispose();}});
test('BrowserRelay_CloseBeforeSignerDispatchDoesNotUseKey_007',async()=>{const socket=new Socket();let calls=0;const relay=createBrowserRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async()=>{calls++;return new Uint8Array(64);}},setupDeadline:clock()+1000,ioBudgetMs:1000,socket});void relay.ready.catch(()=>{});socket.receive(new Uint8Array([1,0,...new Uint8Array(64)]));relay.close();socket.retire();await relay.waitClosed();assert.equal(calls,0);});
