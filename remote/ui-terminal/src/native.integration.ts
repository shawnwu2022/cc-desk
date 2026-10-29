import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {ProjectionReceiver} from './reducer.ts';
test('actual Rust Core encoder parts atomically reconstruct the Host wide-cell view',()=>{
  const path=process.env.CC_DESK_TEST_ONLY_PROJECTION_JSONL;
  assert.ok(path,'explicit TEST_ONLY Rust encoder artifact required');
  const parts=readFileSync(path,'utf8').trim().split('\n').map(line=>JSON.parse(line));
  assert.equal(parts.length,188);
  const first=parts[0];const r=new ProjectionReceiver(first.body.stamp.target,first.body.stamp.binding);
  r.begin(first.body.stamp);
  parts.forEach((p,i)=>{
    assert.equal(r.accept(p),i===parts.length-1?'Installed':'Partial');
    if(i<parts.length-1)assert.equal(r.ack(),undefined);
  });
  assert.equal(r.frame?.cells.length,24000);assert.equal(r.frame?.cells[0].c,'中');
  assert.equal(r.frame?.cells[0].r,'WideLead');assert.equal(r.frame?.cells[1].r,'WideSpacer');
  assert.equal(r.frame?.cells[2].c,'<');assert.equal(r.frame?.cells[3].c,'&');
  assert.equal(r.ack()?.modeEpoch,'1');assert.equal(r.ack()?.revision,'7');
  assert.equal(r.accept(first),'Gap');assert.equal(r.synchronized,false);
});
