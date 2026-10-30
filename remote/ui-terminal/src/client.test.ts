import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {decodeRecord, encodeRecord, command, input, parseJson} from './wire.ts';
import {TerminalClient, type NativeConnection} from './client.ts';

const fixture=()=>JSON.parse(readFileSync(new URL('../../../protocol/remote/inner/v1/test-vectors/projection-snapshot.json',import.meta.url),'utf8'));
const utf8=new TextEncoder();
test('Rust fixed framing oracle and command/input encoders preserve exact canonical bytes',()=>{
  assert.deepEqual([...encodeRecord(2,0,utf8.encode('{}'))],[0,0,0,6,0,2,0,0,123,125]);
  assert.equal(new TextDecoder().decode(command('Snapshot','1')),'{"message":"Snapshot","profile":1,"request":"1"}');
  assert.equal(new TextDecoder().decode(input('07'.repeat(16),'1','08'.repeat(32),'6000',utf8.encode('PUBLIC_TEST_ONLY\r'))),
    '{"message":"InputChunk","profile":1,"operation":"'+ '07'.repeat(16)+'","sequence":"1","window":"'+'08'.repeat(32)+'","notAfterHostMs":"6000","bytes":"UFVCTElDX1RFU1RfT05MWQ0"}');
  for(const n of ['0','01','+1','18446744073709551616'])assert.throws(()=>command('Window',n));
  assert.throws(()=>input('00'.repeat(16),'1','08'.repeat(32),'1',new Uint8Array([1])));
  assert.throws(()=>input('07'.repeat(16),'1','08'.repeat(32),'1',new Uint8Array(8193)));
});
test('bounded record and JSON decoding rejects malformed UTF8, duplicates, depth, minor and trailing bytes',()=>{
  assert.deepEqual(parseJson(utf8.encode('{"a":[1,true,null,"x"]}'),100),{a:[1,true,null,'x']});
  for(const text of ['{"profile":1,"profile":2}','{"a":{"x":1,"\\u0078":2}}','[1,]','{"a":01}','['.repeat(25)+'0'+']'.repeat(25)])assert.throws(()=>parseJson(utf8.encode(text),32768));
  assert.throws(()=>parseJson(new Uint8Array([0xff]),100));
  assert.throws(()=>decodeRecord(encodeRecord(2,0,new Uint8Array()),1));
  const wire=encodeRecord(3,1,utf8.encode('{}'));
  assert.equal(decodeRecord(wire,1).kind,3);
  for(let i=0;i<wire.length;i++)assert.throws(()=>decodeRecord(wire.slice(0,i),1));
  assert.throws(()=>decodeRecord(new Uint8Array([...wire,0]),1));
  assert.throws(()=>encodeRecord(2,1,new Uint8Array(16385)));
});

function setup(deliver?:()=>Promise<void>){
  const f=fixture(), sent:string[]=[];
  const native:NativeConnection={
    async snapshot(bytes, sink){sent.push(new TextDecoder().decode(bytes));sink.begin(f.body.stamp);sink.part(utf8.encode(JSON.stringify(f)));await deliver?.();},
    async acknowledge(bytes){sent.push(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'ProjectionAcknowledged',profile:1,stamp:JSON.parse(new TextDecoder().decode(bytes)).stamp}));},
    async window(bytes){sent.push(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'WindowGranted',profile:1,request:JSON.parse(new TextDecoder().decode(bytes)).request,window:'08'.repeat(32),notAfterHostMs:'6000'}));},
    async input(bytes){const v=JSON.parse(new TextDecoder().decode(bytes));sent.push(JSON.stringify(v));return utf8.encode(JSON.stringify({message:'InputReceipt',profile:1,operation:v.operation,sequence:v.sequence,outcome:'HostWritten',proof:'aa'.repeat(32)}));},
    async query(bytes){const v=JSON.parse(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'ReceiptStatus',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,status:'Known',outcome:'HostWritten',proof:'aa'.repeat(32)}));},
    async settle(bytes){const v=JSON.parse(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'ReceiptSettled',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,outcome:v.outcome,disposition:'Advanced'}));},
    close(){},
  };
  const client=new TerminalClient(f.body.stamp.target,f.body.stamp.binding);
  return {client,native,sent,f};
}
test('snapshot completion precedes ACK and window, native input result requires settlement',async()=>{
  let finish!:()=>void;const {client,native,sent}=setup(()=>new Promise<void>(r=>{finish=r;}));
  assert.equal(client.phase,'NotConnected');await assert.rejects(client.snapshot());
  client.connect(native);const pending=client.snapshot();
  assert.equal(sent.length,1);assert.equal(client.phase,'Busy');await assert.rejects(client.window());
  finish();await pending;assert.equal(client.phase,'Synced');
  assert.equal(JSON.parse(sent[1]).message,'ProjectionAck');assert.equal(JSON.parse(sent[1]).stamp.revision,'9007199254740993');
  await client.window();assert.equal(client.phase,'Ready');
  assert.equal((await client.submit('07'.repeat(16),'1',utf8.encode('hello'))).outcome,'HostWritten');
  assert.equal(client.phase,'NeedsSettlement');await assert.rejects(client.window());await assert.rejects(client.submit('09'.repeat(16),'2',utf8.encode('again')));
});
test('disconnect fences delayed delivery and reconnect requires fresh snapshot without old ACK',async()=>{
  let finish!:()=>void;const {client,native,sent}=setup(()=>new Promise<void>(r=>{finish=r;}));
  client.connect(native);const pending=client.snapshot();client.disconnect();finish();await assert.rejects(pending);
  assert.equal(sent.length,1);assert.equal(client.phase,'NotConnected');assert.ok(client.frame);
  client.connect(native);await assert.rejects(client.window());
  // Reusing the old barrier fails even on a new transport.
  await assert.rejects(client.snapshot());assert.equal(sent.filter(s=>JSON.parse(s).message==='ProjectionAck').length,0);
});
test('malformed projection and rejected ACK cannot open an input window',async()=>{
  for(const fault of ['part','ack']){
    const {client,native}=setup();if(fault==='part')native.snapshot=async(bytes,sink)=>{const f=fixture();sink.begin(f.body.stamp);sink.part(utf8.encode('{"profile":1,"profile":1}'));};
    else native.acknowledge=async()=>{throw Error('native Host rejected');};
    client.connect(native);await assert.rejects(client.snapshot());await assert.rejects(client.window());
  }
});
test('installed frame is rendered through the existing text-only renderer',async()=>{
  const {client,native}=setup();client.connect(native);await client.snapshot();
  const document={createElement:()=>({textContent:null as string|null,className:'',style:{},children:[] as any[],attrs:{} as Record<string,string>,setAttribute(k:string,v:string){this.attrs[k]=v;},appendChild(c:any){this.children.push(c);}})};
  const root=client.render(document) as ReturnType<typeof document.createElement>;
  assert.equal(root.children[0].textContent,'<');assert.equal(root.attrs['aria-label'],'< ');
});


test('only exact final receipt settlement reopens snapshot admission',async()=>{
  const {client,native}=setup();client.connect(native);await client.snapshot();await client.window();
  await client.submit('07'.repeat(16),'1',utf8.encode('hello'));
  await client.query();await client.settle();assert.equal(client.phase,'Idle');
});
test('wrong window request, receipt identity and settlement outcomes never unblock input',async()=>{
  for(const fault of ['window','receipt','settled']){
    const {client,native}=setup();client.connect(native);await client.snapshot();
    if(fault==='window'){
      native.window=async()=>utf8.encode(JSON.stringify({message:'WindowGranted',profile:1,request:'999',window:'08'.repeat(32),notAfterHostMs:'6000'}));
      await assert.rejects(client.window());continue;
    }
    await client.window();
    if(fault==='receipt'){
      native.input=async()=>utf8.encode(JSON.stringify({message:'InputReceipt',profile:1,operation:'09'.repeat(16),sequence:'1',outcome:'HostWritten',proof:'aa'.repeat(32)}));
      await assert.rejects(client.submit('07'.repeat(16),'1',utf8.encode('hello')));
    }else{
      await client.submit('07'.repeat(16),'1',utf8.encode('hello'));
      native.settle=async()=>utf8.encode(JSON.stringify({message:'ReceiptSettled',profile:1,request:'999',operation:'07'.repeat(16),sequence:'1',outcome:'HostWritten',disposition:'Advanced'}));
      await assert.rejects(client.settle());
    }
    client.connect(native);assert.equal(client.phase,'NeedsSettlement');await assert.rejects(client.window());
  }
});

test('unknown, missing and rejected settlement never grant another input',async()=>{
  for(const fault of ['Unknown','Partial','Missing','TooOld','Rejected']){
    const {client,native}=setup();client.connect(native);await client.snapshot();await client.window();
    if(fault!=='Rejected')native.input=async()=>utf8.encode(JSON.stringify({message:'InputReceipt',profile:1,operation:'07'.repeat(16),sequence:'1',outcome:fault==='Partial'?'Partial':'Unknown',proof:null}));
    await client.submit('07'.repeat(16),'1',utf8.encode('hello'));
    if(fault==='Missing'||fault==='TooOld'){
      native.query=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'ReceiptStatus',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,status:fault,outcome:null,proof:null}));};await client.query();
    }
    if(fault==='Rejected'){
      native.settle=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'ReceiptSettled',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,outcome:v.outcome,disposition:'Rejected'}));};await client.settle();
    }else await assert.rejects(client.settle());
    assert.equal(client.phase,'NeedsSettlement');await assert.rejects(client.window());
  }
});
test('native timeout has finite wait and retains possible input effect across reconnect',async()=>{
  const {native,f}=setup(),client=new TerminalClient(f.body.stamp.target,f.body.stamp.binding,10);
  native.input=async()=>new Promise(()=>{});client.connect(native);await client.snapshot();await client.window();
  await assert.rejects(client.submit('07'.repeat(16),'1',utf8.encode('hello')),/deadline/);
  client.connect(native);assert.equal(client.phase,'NeedsSettlement');await assert.rejects(client.snapshot());
});
test('a mismatched exact projection ACK stamp cannot enable a window',async()=>{
  const {client,native,f}=setup();native.acknowledge=async()=>utf8.encode(JSON.stringify({message:'ProjectionAcknowledged',profile:1,stamp:{...f.body.stamp,revision:'1'}}));
  client.connect(native);await assert.rejects(client.snapshot());await assert.rejects(client.window());
});

test('unknown nested projection stamp fields cannot be installed or acknowledged',async()=>{
  const {client,native,f,sent}=setup();native.snapshot=async(bytes,sink)=>{sink.begin(f.body.stamp);f.body.stamp.target.extra=0;sink.part(utf8.encode(JSON.stringify(f)));};
  client.connect(native);await assert.rejects(client.snapshot());assert.equal(sent.length,0);await assert.rejects(client.window());
});
test('new native binding preserves historical display and requires a fresh matching snapshot',async()=>{
  const {client,native,f}=setup();client.connect(native);await client.snapshot();const old=client.frame;client.disconnect();
  f.body.stamp.binding='55'.repeat(32);f.body.stamp.syncEpoch='2';client.connect(native,f.body.stamp.binding);
  assert.equal(client.frame,old);await assert.rejects(client.window());await client.snapshot();assert.equal(client.frame?.stamp.binding,'55'.repeat(32));
});

test('binding changes retain revision, epoch and same-revision content continuity',async()=>{
  for(const fault of ['revision','sync','cells','state','epoch']){
    const {client,native,f,sent}=setup();if(fault==='epoch')f.body.stamp.dimensionEpoch='3';
    client.connect(native);await client.snapshot();const old=client.frame;client.disconnect();
    f.body.stamp.binding='55'.repeat(32);f.body.stamp.syncEpoch='2';
    if(fault==='revision')f.body.stamp.revision='1';
    if(fault==='sync')f.body.stamp.syncEpoch='0';
    if(fault==='cells')f.body.cells[0].c='X';
    if(fault==='state')f.body.state.cursor.column=1;
    if(fault==='epoch'){f.body.stamp.dimensionEpoch='2';f.body.stamp.revision='9007199254740994';}
    client.connect(native,f.body.stamp.binding);await assert.rejects(client.snapshot(),undefined,fault);
    assert.equal(client.frame,old);assert.equal(sent.filter(s=>JSON.parse(s).message==='ProjectionAck').length,1);await assert.rejects(client.window());
  }
});

test('lost settlement response retains exact final proof through TooOld or Missing query',async()=>{
  for(const status of ['TooOld','Missing']){
    const {native,f}=setup(),client=new TerminalClient(f.body.stamp.target,f.body.stamp.binding,10);
    client.connect(native);await client.snapshot();await client.window();await client.submit('07'.repeat(16),'1',utf8.encode('hello'));
    let firstAck:any;native.settle=async bytes=>{firstAck=JSON.parse(new TextDecoder().decode(bytes));return new Promise(()=>{});};
    await assert.rejects(client.settle(),/deadline/);
    native.query=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'ReceiptStatus',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,status,outcome:null,proof:null}));};
    native.settle=async bytes=>{
      const v=JSON.parse(new TextDecoder().decode(bytes));assert.equal(v.operation,firstAck.operation);assert.equal(v.sequence,firstAck.sequence);assert.equal(v.outcome,firstAck.outcome);assert.equal(v.proof,firstAck.proof);
      return utf8.encode(JSON.stringify({message:'ReceiptSettled',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,outcome:v.outcome,disposition:'AlreadySettled'}));
    };
    client.connect(native,'55'.repeat(32));await client.query();assert.equal(client.phase,'NeedsSettlement');await assert.rejects(client.window());
    await client.settle();assert.equal(client.phase,'Idle');
  }
});
test('contradictory Known receipt cannot replace a retained exact final proof',async()=>{
  for(const patch of [{outcome:'Rejected',proof:'aa'.repeat(32)},{outcome:'HostWritten',proof:'bb'.repeat(32)},{outcome:'Unknown',proof:null}]){
    const {client,native}=setup();client.connect(native);await client.snapshot();await client.window();await client.submit('07'.repeat(16),'1',utf8.encode('hello'));
    native.query=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));return utf8.encode(JSON.stringify({message:'ReceiptStatus',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,status:'Known',...patch}));};
    await assert.rejects(client.query());assert.equal(client.phase,'NotConnected');
    native.settle=async bytes=>{const v=JSON.parse(new TextDecoder().decode(bytes));assert.equal(v.outcome,'HostWritten');assert.equal(v.proof,'aa'.repeat(32));return utf8.encode(JSON.stringify({message:'ReceiptSettled',profile:1,request:v.request,operation:v.operation,sequence:v.sequence,outcome:v.outcome,disposition:'AlreadySettled'}));};
    client.connect(native);await client.settle();assert.equal(client.phase,'Idle');
  }
});
