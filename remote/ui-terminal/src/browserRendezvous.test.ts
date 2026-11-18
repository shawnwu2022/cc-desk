import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createBrowserRelay} from './browserRelay.ts';
const clock=()=>performance.timeOrigin+performance.now();
const tunnel=new Uint8Array(16).fill(9),keyId=new Uint8Array(16).fill(7);
const tick=()=>new Promise<void>(resolve=>setImmediate(resolve));
class Socket extends EventTarget {
 readyState=1;binaryType:BinaryType='arraybuffer';bufferedAmount=0;protocol='cc-desk-relay-rendezvous-v1';sent:Uint8Array[]=[];
 send(value:unknown){this.sent.push(Uint8Array.from(value as Uint8Array));}
 close(){this.readyState=2;}
 receive(value:Uint8Array){this.dispatchEvent(new MessageEvent('message',{data:value.slice().buffer}));}
 retire(){this.readyState=3;this.dispatchEvent(new Event('close'));}
}
async function waiting(protocol='cc-desk-relay-rendezvous-v1',budget=1000){
 const socket=new Socket();socket.protocol=protocol;
 const relay=createBrowserRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async()=>new Uint8Array(64)},setupDeadline:clock()+budget,ioBudgetMs:1000,socket,mode:'rendezvous-v1'} as Parameters<typeof createBrowserRelay>[0]);
 socket.receive(new Uint8Array([1,0,...new Uint8Array(64)]));await tick();socket.receive(new Uint8Array([1,2]));await tick();
 return{socket,relay,dispose:async()=>{relay.close();socket.retire();await relay.waitClosed();}};
}
test('BrowserRendezvous_AcceptedDoesNotExposeTlsUntilMatchingReady_001',async()=>{
 const f=await waiting();try{let opened=false;void f.relay.ready.then(()=>{opened=true;},()=>{});await tick();assert.equal(opened,false);
 assert.deepEqual(f.socket.sent[1],new Uint8Array([2,0,...tunnel]));
 f.socket.receive(new Uint8Array([2,1,...tunnel]));const wire=await f.relay.ready;await wire.write(new Uint8Array([5]));assert.equal(f.socket.sent[2]![0],1);
 }finally{await f.dispose();}
});
test('BrowserRendezvous_MissingOrWrongEchoNeverUsesSigner_002',async()=>{
 for(const protocol of ['', 'other']){const socket=new Socket();socket.protocol=protocol;let signs=0;const relay=createBrowserRelay({origin:'wss://relay.invalid/route',tunnel,identity:{keyId,sign:async()=>{signs++;return new Uint8Array(64);}},setupDeadline:clock()+1000,ioBudgetMs:1000,socket,mode:'rendezvous-v1'} as Parameters<typeof createBrowserRelay>[0]);try{socket.receive(new Uint8Array([1,0,...new Uint8Array(64)]));await assert.rejects(relay.ready);assert.equal(signs,0);assert.equal(socket.sent.length,0);}finally{relay.close();socket.retire();await relay.waitClosed();}}
});
test('BrowserRendezvous_InvalidReadyAndPayloadRejectWithoutData_003',async()=>{
 for(const value of [new Uint8Array([2,1]),new Uint8Array([2,1,...new Uint8Array(16).fill(8)]),new Uint8Array([1,1,0,0]),new Uint8Array([1,2])]){const f=await waiting();try{f.socket.receive(value);await assert.rejects(f.relay.ready);assert.equal(f.socket.sent.length,2);}finally{await f.dispose();}}
});
test('BrowserRendezvous_OriginalSetupDeadlineAndCancellationRemainEffective_004',async()=>{
 const f=await waiting('cc-desk-relay-rendezvous-v1',80);try{await assert.rejects(f.relay.ready);assert.equal(f.socket.readyState,2);}finally{await f.dispose();}
 const g=await waiting();try{g.relay.close();await assert.rejects(g.relay.ready);g.socket.receive(new Uint8Array([2,1,...tunnel]));assert.equal(g.socket.sent.length,2);}finally{await g.dispose();}
});
