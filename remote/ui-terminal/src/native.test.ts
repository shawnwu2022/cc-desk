import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {nativeConnection,type NativeClientPort} from './native.ts';
import {command} from './wire.ts';
const fixture=()=>JSON.parse(readFileSync(new URL('../../../protocol/remote/inner/v1/test-vectors/projection-snapshot.json',import.meta.url),'utf8'));
const encoder=new TextEncoder();
test('native port adapter delivers complete snapshot bytes and exact request without inventing authority',async()=>{
  const f=fixture();let calls=0,closed=0;const sent:Uint8Array[]=[];
  const port:NativeClientPort={async snapshot(p,b){calls++;assert.equal(b,1000);sent.push(p);return{stamp:f.body.stamp,parts:[encoder.encode(JSON.stringify(f))]};},async exchange(p){return p;},async close(){closed++;}};
  const native=nativeConnection(port);let begin=0,part=0;
  await native.snapshot(command('Snapshot','1'),{begin(s){assert.deepEqual(s,f.body.stamp);begin++;},part(p){assert.deepEqual(JSON.parse(new TextDecoder().decode(p)),f);part++;}});
  assert.equal(calls,1);assert.equal(begin,1);assert.equal(part,1);assert.deepEqual(sent[0],command('Snapshot','1'));
  native.close();native.close();await Promise.resolve();assert.equal(closed,1);
});
test('native port adapter fences late completion and refuses concurrent queue',async()=>{
  let finish!:(p:Uint8Array)=>void,closed=0;
  const native=nativeConnection({async snapshot(){throw Error('unused');},exchange(){return new Promise(r=>{finish=r;});},async close(){closed++;}});
  const pending=native.window(command('Window','1'));
  await assert.rejects(native.window(command('Window','2')));
  native.close();finish(encoder.encode('{}'));await assert.rejects(pending);assert.equal(closed,1);
});
test('native port malformed/oversized output closes original owner without delivering callbacks',async()=>{
  for(const oversized of [false,true]){
    const f=fixture();let closed=0,delivered=0;
    const native=nativeConnection({async snapshot(){return{stamp:f.body.stamp,parts:[oversized?new Uint8Array(32769):encoder.encode('{}')]};},async exchange(){return new Uint8Array();},async close(){closed++;}});
    await assert.rejects(native.snapshot(command('Snapshot','1'),{begin(){delivered++;},part(){delivered++;}}));
    assert.equal(closed,1);assert.equal(delivered,0);
  }
});
test('native port rejects completion past monotonic deadline even before timer callback can run',async()=>{
  let closed=0;
  const native=nativeConnection({async snapshot(){throw Error('unused');},async exchange(){const end=performance.now()+10;while(performance.now()<end){}return encoder.encode(JSON.stringify({message:'WindowGranted',profile:1,request:'1',window:'09'.repeat(32),notAfterHostMs:'2000'}));},async close(){closed++;}},1);
  await assert.rejects(native.window(command('Window','1')));assert.equal(closed,1);
});
