import {test} from 'node:test';import assert from 'node:assert/strict';
import {createBrowserNativeAdapter} from './browserNative.ts';
type PullPort={snapshotPull(request:bigint,budget:number):Promise<unknown>};
function fixture(publication?:{prepare:()=>Promise<void>;settled:()=>void}){
 const memory=new WebAssembly.Memory({initial:1});let now=10000,closed=false;
 const adapter=createBrowserNativeAdapter(()=>memory,()=>closed,()=>{},()=>{closed=true;},async()=>{},()=>now,()=>20000,publication);
 const rpc=adapter.imports.cc_desk_browser_client_v1;
 return{adapter,rpc,memory,pull:adapter.port as unknown as PullPort,set:(n:number)=>{now=n;},closed:()=>closed,close:()=>{adapter.terminal();},header:()=>new DataView(memory.buffer,4096)};
}
test('BrowserSnapshotPull_RpcSharesPendingSequenceAndOriginalDeadline_001',async()=>{
 const f=fixture();assert.equal(f.adapter.limitDeadline(10500),true);
 const work=f.pull.snapshotPull(18446744073709551615n,1000);void work.catch(()=>{});
 assert.equal(f.rpc.request(4096,32768),28);const h=f.header();
 assert.equal(h.getUint32(0,true),1);assert.equal(h.getUint32(4,true),5);
 assert.equal(h.getFloat64(12,true),10500);assert.equal(h.getBigUint64(20,true),18446744073709551615n);
 await assert.rejects(f.pull.snapshotPull(2n,1000));await assert.rejects(f.adapter.port.exchange(new Uint8Array([1]),1000));
 f.close();await assert.rejects(work);await assert.rejects(f.pull.snapshotPull(3n,1000));
});
test('BrowserSnapshotPull_PublicationBlocksNewRpc_002',async()=>{
 const f=fixture({prepare:async()=>{},settled:()=>{}});
 await assert.rejects(f.pull.snapshotPull(1n,1000));assert.equal(f.rpc.request(4096,32768),-1);
});
test('BrowserSnapshotPull_InvalidRequestDoesNotConsumeSequence_003',async()=>{
 const f=fixture();
 for(const n of [0n,-1n,18446744073709551616n,1 as unknown as bigint])await assert.rejects(f.pull.snapshotPull(n,1000));
 assert.equal(f.rpc.request(4096,32768),-1);
 const old=f.adapter.port.exchange(new Uint8Array([1]),1000);void old.catch(()=>{});
 assert.equal(f.rpc.request(4096,32768),21);assert.equal(f.header().getUint32(0,true),1);assert.equal(f.header().getUint32(4,true),2);
 f.close();await assert.rejects(old);
});
test('BrowserSnapshotPull_LateReplyClosesOriginalOwner_004',async()=>{
 const f=fixture();const work=f.pull.snapshotPull(1n,100);void work.catch(()=>{});
 assert.equal(f.rpc.request(4096,32768),28);f.set(10100);f.rpc.reply(1,1,0,0);
 await assert.rejects(work);assert.equal(f.closed(),true);
});
import {serveBrowserClient,browserClientPort} from './browserClient.ts';
import type {NativeClientPort} from './native.ts';
const turn=()=>new Promise<void>(resolve=>setImmediate(resolve));
function workerFixture(supported=true){
 const c=new MessageChannel();let finish!:()=>void,closed=false;const retirement=new Promise<void>(r=>{finish=r;});
 let release!:(v:{stamp:any;parts:Uint8Array[]})=>void;const calls:bigint[]=[],deadlines:number[]=[],messages:any[]=[];
 const client:NativeClientPort&Partial<PullPort>={snapshot:async()=>{throw Error('no fallback');},exchange:async()=>new Uint8Array([2]),close:async()=>{}};
 if(supported)client.snapshotPull=async n=>{calls.push(n);return await new Promise(r=>{release=r;});};
 c.port2.addEventListener('message',e=>messages.push(e.data));c.port2.start();
 serveBrowserClient(c.port1,client as any,Promise.resolve({target:{} as any,binding:'a'.repeat(64),role:'Controller',fence:'1'}),()=>{closed=true;},()=>retirement,n=>{deadlines.push(n);return n===10500;});
 const send=(seq:number,payload:Uint8Array,deadline=10500)=>c.port2.postMessage({kind:'snapshotPull',seq,payload,budgetMs:1000,deadline});
 return{raw:(m:unknown)=>c.port2.postMessage(m),calls,deadlines,messages,send,closed:()=>closed,release:()=>release({stamp:{},parts:[new Uint8Array([1])]}),cleanup:async()=>{finish();await turn();await turn();c.port1.close();c.port2.close();}};
}
const pullBytes=(n=1n)=>{const b=new Uint8Array(8);new DataView(b.buffer).setBigUint64(0,n,true);return b;};
test('BrowserSnapshotPull_WorkerUsesOriginalSinglePendingAndRetirement_005',async()=>{
 const f=workerFixture();try{f.send(1,pullBytes(7n));await turn();await turn();assert.deepEqual(f.calls,[7n]);assert.deepEqual(f.deadlines,[10500]);
 f.send(2,pullBytes(8n));await turn();await turn();assert.equal(f.closed(),true);assert.deepEqual(f.calls,[7n]);f.release();await turn();assert.equal(f.messages.some(m=>m.kind==='result'&&m.ok),false);
 }finally{await f.cleanup();}
});
test('BrowserSnapshotPull_WorkerRejectsMalformedSequenceDeadlineAndOldPort_006',async()=>{
 for(const mode of ['length','zero','sequence','deadline','old']){
  const f=workerFixture(mode!=='old');try{
   f.send(mode==='sequence'?2:1,mode==='length'?new Uint8Array(9):pullBytes(mode==='zero'?0n:1n),mode==='deadline'?10501:10500);
   await turn();await turn();assert.equal(f.closed(),true,mode);assert.deepEqual(f.calls,[],mode);
  }finally{await f.cleanup();}
 }
});
test('BrowserSnapshotPull_SuccessThenLegacySequenceAndClose_007',async()=>{
 const f=fixture();const work=f.pull.snapshotPull(1n,1000);
 assert.equal(f.rpc.request(4096,32768),28);
 const part=new TextEncoder().encode(JSON.stringify({body:{stamp:{request:'1'}}}));new Uint8Array(f.memory.buffer).set(part,8192);
 f.rpc.reply(1,0,8192,part.length);f.rpc.reply(1,1,0,0);assert.deepEqual(await work,{stamp:{request:'1'},parts:[part]});
 const old=f.adapter.port.snapshot(new Uint8Array([9]),1000);void old.catch(()=>{});
 assert.equal(f.rpc.request(4096,32768),21);assert.equal(f.header().getUint32(0,true),2);assert.equal(f.header().getUint32(4,true),1);
 await f.adapter.port.close();assert.equal(f.closed(),true);f.close();await assert.rejects(old);await assert.rejects(f.pull.snapshotPull(2n,1000));
});
test('BrowserSnapshotPull_WorkerRejectsExtraFieldsAndBudget_008',async()=>{
 for(const invalid of [{extra:true},{budgetMs:0},{budgetMs:1001},{budgetMs:1.5}]){
  const f=workerFixture();try{f.raw({kind:'snapshotPull',seq:1,payload:pullBytes(),budgetMs:1000,deadline:10500,...invalid});await turn();await turn();assert.equal(f.closed(),true);assert.deepEqual(f.calls,[]);}finally{await f.cleanup();}
 }
});
test('BrowserSnapshotPull_WindowToWorkerOriginalBridgeRoundtrip_009',async()=>{
 class TestDocument extends EventTarget{visibilityState='visible';}
 const doc=new TestDocument(),events=new EventTarget(),saved=new Map<string,PropertyDescriptor|undefined>();
 for(const [key,value]of Object.entries({Document:TestDocument,document:doc,addEventListener:events.addEventListener.bind(events),removeEventListener:events.removeEventListener.bind(events)})){saved.set(key,Object.getOwnPropertyDescriptor(globalThis,key));Object.defineProperty(globalThis,key,{configurable:true,value});}
 const c=new MessageChannel(),memory=new WebAssembly.Memory({initial:1});let finished!:()=>void,closed=false;
 const retired=new Promise<void>(r=>{finished=r;}),clock=()=>Math.floor(performance.timeOrigin+performance.now());
 const adapter=createBrowserNativeAdapter(()=>memory,()=>closed,()=>{},()=>{closed=true;adapter.terminal();finished();},()=>retired,clock,()=>clock()+1000);
 const rpc=adapter.imports.cc_desk_browser_client_v1;
 serveBrowserClient(c.port1,adapter.port,adapter.descriptor,()=>{closed=true;adapter.terminal();finished();},()=>retired,adapter.limitDeadline);
 const app=browserClientPort(c.port2,()=>{closed=true;adapter.terminal();finished();},()=>retired,new SharedArrayBuffer(4));
 try{
  const descriptor={target:{session:'s',run:'r',generation:'1',pane:'p',terminal:'t'},binding:'a'.repeat(64),role:'Controller',fence:'1'};
  const raw=new TextEncoder().encode(JSON.stringify(descriptor));new Uint8Array(memory.buffer).set(raw);rpc.descriptor(0,raw.length);assert.deepEqual(await app.descriptor,descriptor);
  const work=app.snapshotPull(9n,1000);void work.catch(()=>{});await turn();await turn();
  assert.equal(rpc.request(4096,32768),28);const header=new DataView(memory.buffer,4096);assert.equal(header.getUint32(4,true),5);assert.equal(header.getBigUint64(20,true),9n);
  const part=new TextEncoder().encode(JSON.stringify({body:{stamp:{request:'9'}}}));new Uint8Array(memory.buffer).set(part,8192);rpc.reply(1,0,8192,part.length);rpc.reply(1,1,0,0);
  assert.deepEqual(await work,{stamp:{request:'9'},parts:[part]});
  const old=app.exchange(new Uint8Array([2]),1000);void old.catch(()=>{});await turn();await turn();assert.equal(rpc.request(4096,32768),21);assert.equal(header.getUint32(0,true),2);assert.equal(header.getUint32(4,true),2);
  new Uint8Array(memory.buffer)[8192]=7;rpc.reply(2,0,8192,1);rpc.reply(2,1,0,0);assert.deepEqual(await old,new Uint8Array([7]));
  await app.close();assert.equal(closed,true);await assert.rejects(app.snapshotPull(10n,1000));
 }finally{closed=true;adapter.terminal();finished();c.port1.close();c.port2.close();for(const[key,value]of saved){if(value)Object.defineProperty(globalThis,key,value);else Reflect.deleteProperty(globalThis,key);}}
});