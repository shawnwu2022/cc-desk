import test from 'node:test';
import assert from 'node:assert/strict';
import * as wire from './wire.ts';
const w=wire as any,e=new TextEncoder();
const bytes=(v:any)=>e.encode(JSON.stringify(v));
const receipt={message:'InputReceipt',profile:1,operation:'07'.repeat(16),sequence:'1',outcome:'HostWritten',proof:'aa'.repeat(32)};

test('shared control reply contract validates exact outcomes, proof nullability and field shapes',()=>{
  assert.equal(typeof w.decodeReply,'function');
  assert.deepEqual(w.decodeReply(bytes(receipt)),receipt);
  for(const patch of [{profile:0},{proof:null},{proof:'00'.repeat(32)},{outcome:'Complete'},{sequence:'01'},{extra:0}])assert.throws(()=>w.decodeReply(bytes({...receipt,...patch})));
  for(const outcome of ['Partial','Unknown']){
    assert.equal(w.decodeReply(bytes({...receipt,outcome,proof:null})).outcome,outcome);
    assert.throws(()=>w.decodeReply(bytes({...receipt,outcome})));
  }
  const status={...receipt,message:'ReceiptStatus',request:'3',status:'Known'};
  assert.equal(w.decodeReply(bytes(status)).status,'Known');
  for(const state of ['Missing','TooOld']){
    assert.equal(w.decodeReply(bytes({...status,status:state,outcome:null,proof:null})).status,state);
    assert.throws(()=>w.decodeReply(bytes({...status,status:state})));
  }
  assert.throws(()=>w.decodeReply(bytes({...receipt,outcome:{HostWritten:null}})));
  assert.throws(()=>w.decodeReply(e.encode('{"message":"InputReceipt","profile":1,"profile":1}')));
  assert.throws(()=>w.decodeReply(new Uint8Array(16271)));
  assert.throws(()=>w.parseJson(new Uint8Array([239,187,191,123,125]),100));
});
test('receipt query and ACK commands retain exact operation, sequence and final proof',()=>{
  assert.equal(typeof w.receiptQuery,'function');assert.equal(typeof w.receiptAck,'function');
  assert.deepEqual(JSON.parse(new TextDecoder().decode(w.receiptQuery('3',receipt.operation,'1'))),{message:'ReceiptQuery',profile:1,request:'3',operation:receipt.operation,sequence:'1'});
  const ack=JSON.parse(new TextDecoder().decode(w.receiptAck('4',receipt)));
  assert.deepEqual(ack,{message:'ReceiptAck',profile:1,request:'4',operation:receipt.operation,sequence:'1',outcome:'HostWritten',proof:receipt.proof});
  assert.throws(()=>w.receiptAck('4',{...receipt,outcome:'Unknown',proof:null}));
});

test('integer schema fields reject decimal and exponent lexemes accepted by JavaScript Number',()=>{
  for(const lexeme of ['1.0','1e0','1E+0'])assert.throws(()=>w.decodeReply(e.encode(JSON.stringify(receipt).replace('"profile":1','"profile":'+lexeme))));
  const projection={profile:1,message:'SnapshotPart',body:{stamp:{target:{host:'11'.repeat(32),session:'22'.repeat(16),run:'fixture',generation:'1',incarnation:'1'},binding:'33'.repeat(32),barrier:'44'.repeat(16),revision:'1',dimensionEpoch:'0',modeEpoch:'0',structureEpoch:'0',syncEpoch:'1'},state:{rows:1,columns:2,buffer:'Normal',focusReporting:false,cursor:{row:0,column:0,shape:'Block',blinking:false}},start:0,cells:[{c:'x',r:'Single',w:false,f:{Palette:1},b:{Rgb:[0,1,2]},s:0}]}};
  const raw=JSON.stringify(projection);
  for(const [field,value] of [['profile',1],['rows',1],['columns',2],['row',0],['column',0],['start',0],['Palette',1],['s',0]] as const){
    for(const lexeme of [value+'.0',value+'e0'])assert.throws(()=>w.decodeProjection(e.encode(raw.replace('"'+field+'":'+value,'"'+field+'":'+lexeme))),field);
  }
  assert.throws(()=>w.decodeProjection(e.encode(raw.replace('[0,1,2]','[0,1e0,2]'))));
  // Additive projection fields retain ordinary JSON number compatibility.
  assert.doesNotThrow(()=>w.decodeProjection(e.encode(raw.replace('"start":0','"future":1.5,"start":0'))));
});
