import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,statSync} from 'node:fs';
import {decodeReply} from './wire.ts';

const vector=process.env.CC_DESK_TEST_ONLY_CURSOR_JSONL;
test('actual Rust cursor encoder bytes decode without schema or nullable-field drift',{skip:!vector},()=>{
  assert.ok(vector);assert.ok(statSync(vector).size<=4096);
  const lines=new TextDecoder('utf-8',{fatal:true}).decode(readFileSync(vector)).trim().split(/\r?\n/);
  assert.equal(lines.length,2);
  const [next,pending]=lines.map(line=>decodeReply(new TextEncoder().encode(line)));
  assert.deepEqual(next,{message:'InputCursorState',profile:1,request:'1',nextSequence:'42',pending:null});
  assert.deepEqual(pending,{message:'InputCursorState',profile:1,request:'2',nextSequence:null,pending:{operation:'02'.repeat(16),sequence:'41',outcome:'Unknown',proof:null}});
});
