import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,statSync} from 'node:fs';
import {decodeReply} from './wire.ts';

test('actual Rust control encoder bytes are consumable by the phone reply decoder',()=>{
  const path=process.env.CC_DESK_TEST_ONLY_CONTROL_JSONL;
  assert.ok(path,'explicit TEST_ONLY Rust control encoder artifact required');
  assert.ok(statSync(path).size<=262144,'bounded public encoder artifact');
  const lines=readFileSync(path,'utf8').trim().split('\n');assert.equal(lines.length,14);
  const records=lines.map(line=>decodeReply(new TextEncoder().encode(line)));
  const counts=(message:string)=>records.filter(r=>r.message===message).length;
  assert.equal(counts('WindowGranted'),1);assert.equal(counts('InputReceipt'),4);
  assert.equal(counts('InputNotAccepted'),1);assert.equal(counts('ReceiptStatus'),4);
  assert.equal(counts('ReceiptSettled'),3);assert.equal(counts('ProjectionAcknowledged'),1);
  const window=records.find(r=>r.message==='WindowGranted');assert.equal(window?.message,'WindowGranted');
  if(window?.message==='WindowGranted'){assert.equal(window.window,'03'.repeat(32));assert.equal(window.notAfterHostMs,'6000');}
  for(const record of records){
    if(record.message==='InputReceipt')assert.equal(record.proof===null,record.outcome==='Partial'||record.outcome==='Unknown');
    if(record.message==='ProjectionAcknowledged'){
      assert.equal(record.stamp.target.run,'TEST_ONLY_CONTROL');assert.equal(record.stamp.target.generation,'3');
      assert.equal(record.stamp.target.incarnation,'11');assert.equal(record.stamp.revision,'1');
    }
  }
  const unknown={...JSON.parse(lines[0]),message:'UnknownRequiredReply'};
  assert.throws(()=>decodeReply(new TextEncoder().encode(JSON.stringify(unknown))));
  assert.throws(()=>decodeReply(new TextEncoder().encode(lines[0].replace('{','{"profile":1,'))));
});
