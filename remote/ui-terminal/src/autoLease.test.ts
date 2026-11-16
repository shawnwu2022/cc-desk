import {test} from 'node:test';import assert from 'node:assert/strict';import {readFileSync} from 'node:fs';
import {TerminalClient,type NativeConnection} from './client.ts';
const encode=(v:unknown)=>new TextEncoder().encode(JSON.stringify(v));
const flush=async()=>{for(let i=0;i<16;i++)await Promise.resolve();};
function setup(){
 const frame=JSON.parse(readFileSync(new URL('../../../protocol/remote/inner/v1/test-vectors/projection-snapshot.json',import.meta.url),'utf8')),sent:Record<string,any>[]=[];let release!:()=>void,closed=false;
 const retired=new Promise<void>(r=>{release=r});
 const native:NativeConnection&{waitClosed():Promise<void>}={
  async query(bytes){const v=JSON.parse(new TextDecoder().decode(bytes));sent.push(v);return encode(v.message==='LeaseState'?{message:'LeaseStateReply',profile:1,request:v.request,lease:'11'.repeat(16),fence:'3',notAfterHostMs:'20000',nextRenewalSequence:'1'}:{message:'LeaseRenewed',profile:1,request:v.request,lease:v.lease,fence:v.fence,renewalSequence:v.renewalSequence,notAfterHostMs:String(20000+Number(v.renewalSequence)*5000)});},
  async snapshot(bytes,sink){sent.push(JSON.parse(new TextDecoder().decode(bytes)));sink.begin(frame.body.stamp);sink.part(encode(frame));},
  async acknowledge(bytes){const v=JSON.parse(new TextDecoder().decode(bytes));sent.push(v);return encode({message:'ProjectionAcknowledged',profile:1,stamp:v.stamp});},
  async window(){throw Error('unused');},async input(){throw Error('unused');},async settle(){throw Error('unused');},close(){closed=true;},waitClosed(){return retired;},
 };
 const client=new TerminalClient(frame.body.stamp.target,frame.body.stamp.binding);client.connect(native);
 return{client,native,sent,closed:()=>closed,release};
}
function time(t:any){let now=0;t.mock.method(performance,'now',()=>now);t.mock.timers.enable({apis:['setTimeout']});return{async advance(ms:number){now+=ms;t.mock.timers.tick(ms);await flush();},set(v:number){now=v;}};}
test('AutoLease_ExplicitStartAndOwnedStop_001',async(t)=>{
 const clock=time(t),f=setup();assert.equal((f.client as any).autoRenewalState,'Disabled');await assert.rejects((f.client as any).startAutoRenewal());await f.client.leaseState();await (f.client as any).startAutoRenewal();assert.equal((f.client as any).autoRenewalState,'Waiting');assert.equal(f.sent.filter(v=>v.message==='LeaseRenew').length,1);await clock.advance(5000);assert.equal(f.sent.filter(v=>v.message==='LeaseRenew').length,2);
 let done=false;const stopped=(f.client as any).stopAutoRenewal().then(()=>{done=true;});assert.equal(f.closed(),true);assert.equal(f.client.phase,'NotConnected');await flush();assert.equal(done,false);f.release();await stopped;assert.equal((f.client as any).autoRenewalState,'Stopped');await clock.advance(10000);assert.equal(f.sent.filter(v=>v.message==='LeaseRenew').length,2);
});
test('AutoLease_DueWaitsCompleteSnapshotAckBeforeNextRequest_002',async(t)=>{
 const clock=time(t),f=setup();await f.client.leaseState();await (f.client as any).startAutoRenewal();let release!:()=>void;const original=f.native.snapshot;f.native.snapshot=async(b,s)=>{await original(b,s);await new Promise<void>(r=>{release=r;});};await clock.advance(4500);const snapshot=f.client.snapshot();await flush();await clock.advance(500);assert.equal(f.sent.filter(v=>v.message==='LeaseRenew').length,1);release();await snapshot;await flush();assert.deepEqual(f.sent.map(v=>v.message),['LeaseState','LeaseRenew','Snapshot','ProjectionAck','LeaseRenew']);const stopping=(f.client as any).stopAutoRenewal();f.release();await stopping;
});
test('AutoLease_SevereDelayClosesWithoutCatchup_003',async(t)=>{
 const clock=time(t),f=setup();await f.client.leaseState();await (f.client as any).startAutoRenewal();await clock.advance(9000);assert.equal(f.closed(),true);assert.equal(f.sent.filter(v=>v.message==='LeaseRenew').length,1);f.release();await (f.client as any).stopAutoRenewal();
});

test('AutoLease_MissingRetirementRejectsBeforeFirstRenew_004',async()=>{
 const f=setup();delete (f.native as any).waitClosed;await f.client.leaseState();await assert.rejects((f.client as any).startAutoRenewal());assert.equal(f.sent.filter(v=>v.message==='LeaseRenew').length,0);assert.equal(f.client.phase,'Idle');f.client.disconnect();f.release();
});
test('AutoLease_StopInsideFirstCallRetainsLateOriginalPromise_005',async(t)=>{
 const clock=time(t),f=setup();await f.client.leaseState();let release!:(v:Uint8Array)=>void,request:any,stopped!:Promise<void>;
 f.native.query=bytes=>{request=JSON.parse(new TextDecoder().decode(bytes));stopped=(f.client as any).stopAutoRenewal();return new Promise(r=>{release=r});};
 const starting=(f.client as any).startAutoRenewal();void starting.catch(()=>{});let done=false;void stopped.then(()=>{done=true});f.release();await clock.advance(1000);await assert.rejects(starting);assert.equal(done,false);assert.equal((f.client as any).autoRenewalState,'Stopping');assert.throws(()=>f.client.connect(setup().native));
 release(encode({message:'LeaseRenewed',profile:1,request:request.request,lease:request.lease,fence:request.fence,renewalSequence:request.renewalSequence,notAfterHostMs:'25000'}));await stopped;assert.equal((f.client as any).autoRenewalState,'Stopped');
});
test('AutoLease_CloseReentryUsesOneOriginalRetirement_006',async(t)=>{
 time(t);const f=setup();let closes=0,joins=0;const close=f.native.close;f.native.close=()=>{closes++;close();void (f.client as any).stopAutoRenewal();f.client.disconnect();};f.native.waitClosed=function(){assert.equal(this,f.native);joins++;return Promise.resolve();};await f.client.leaseState();await (f.client as any).startAutoRenewal();await (f.client as any).stopAutoRenewal();assert.equal(closes,1);assert.equal(joins,1);assert.equal((f.client as any).autoRenewalState,'Stopped');
});
test('AutoLease_UnknownRetirementCannotBeOverwritten_007',async(t)=>{
 time(t);const f=setup();f.native.waitClosed=()=>Promise.reject(Error('TEST_ONLY unknown'));await f.client.leaseState();await (f.client as any).startAutoRenewal();await assert.rejects((f.client as any).stopAutoRenewal());assert.equal((f.client as any).autoRenewalState,'Unknown');assert.throws(()=>f.client.connect(setup().native));await assert.rejects((f.client as any).startAutoRenewal());f.release();
});
test('AutoLease_AdmissionDetectsDueBeforeTimerAndDoesNotStarve_008',async(t)=>{
 const clock=time(t),f=setup();await f.client.leaseState();await (f.client as any).startAutoRenewal();clock.set(5000);await assert.rejects(f.client.snapshot());await flush();assert.deepEqual(f.sent.map(v=>v.message),['LeaseState','LeaseRenew','LeaseRenew']);await f.client.snapshot();assert.equal(f.client.phase,'Synced');const stopped=(f.client as any).stopAutoRenewal();f.release();await stopped;
});
test('AutoLease_DueCannotSplitHeldAck_009',async(t)=>{
 const clock=time(t),f=setup();await f.client.leaseState();await (f.client as any).startAutoRenewal();let release!:()=>void;const original=f.native.acknowledge;f.native.acknowledge=async bytes=>{const reply=await original(bytes);await new Promise<void>(r=>{release=r});return reply;};await clock.advance(4500);const snapshot=f.client.snapshot();await flush();await clock.advance(500);assert.equal(f.sent.filter(v=>v.message==='LeaseRenew').length,1);release();await snapshot;await flush();assert.deepEqual(f.sent.slice(-2).map(v=>v.message),['ProjectionAck','LeaseRenew']);const stopped=(f.client as any).stopAutoRenewal();f.release();await stopped;
});
test('AutoLease_MonotonicRollbackClosesInsteadOfRearming_010',async(t)=>{
 const clock=time(t),f=setup();await f.client.leaseState();await (f.client as any).startAutoRenewal();await clock.advance(1000);clock.set(-1);await assert.rejects(f.client.snapshot());assert.equal(f.closed(),true);f.release();await (f.client as any).stopAutoRenewal();
});

test('AutoLease_NeedsSettlementSurvivesAutomaticTick_011',async(t)=>{
 const clock=time(t),f=setup();f.native.window=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));return encode({message:'WindowGranted',profile:1,request:v.request,window:'08'.repeat(32),notAfterHostMs:'20000'});};f.native.input=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));return encode({message:'InputReceipt',profile:1,operation:v.operation,sequence:v.sequence,outcome:'HostWritten',proof:'aa'.repeat(32)});};f.native.settle=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));return encode({message:'ReceiptSettled',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,outcome:v.outcome,disposition:'Advanced'});};await f.client.leaseState();await f.client.startAutoRenewal();await f.client.snapshot();await f.client.window();await f.client.submit('07'.repeat(16),'1',new Uint8Array([65]));await clock.advance(5000);assert.equal(f.client.phase,'NeedsSettlement');await assert.rejects(f.client.window());await f.client.settle();assert.equal(f.client.phase,'Idle');const stopped=f.client.stopAutoRenewal();f.release();await stopped;
});
test('AutoLease_CapturesOriginalJoinMethodAndFirstFailure_012',async(t)=>{
 time(t);const f=setup();await f.client.leaseState();f.native.query=async()=>{throw Error('TEST_ONLY original rejection');};const start=f.client.startAutoRenewal();await assert.rejects(start,/original rejection/);let replacement=0;f.native.waitClosed=()=>{replacement++;return Promise.resolve();};let done=false;const stopped=f.client.stopAutoRenewal().then(()=>{done=true});await flush();assert.equal(done,false);f.release();await stopped;assert.equal(replacement,0);assert.equal(f.client.autoRenewalState,'Stopped');
});

test('AutoLease_FirstReplyAfterPauseCannotBeatTimer_013',async(t)=>{
 const clock=time(t),f=setup();await f.client.leaseState();let release!:(value:Uint8Array)=>void,request:any;f.native.query=bytes=>{request=JSON.parse(new TextDecoder().decode(bytes));return new Promise(r=>{release=r});};const start=f.client.startAutoRenewal();clock.set(9000);release(encode({message:'LeaseRenewed',profile:1,request:request.request,lease:request.lease,fence:request.fence,renewalSequence:request.renewalSequence,notAfterHostMs:'25000'}));await assert.rejects(start,/late/);assert.equal(f.closed(),true);f.release();await f.client.stopAutoRenewal();
});
