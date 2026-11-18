import {test} from 'node:test';import assert from 'node:assert/strict';import {readFileSync} from 'node:fs';
import * as explicit from './browserPullConnection.ts';
import {nativeConnection} from './native.ts';
import type {BrowserNativeClientPort} from './browserNative.ts';import {command} from './wire.ts';
const encode=(v:unknown)=>new TextEncoder().encode(JSON.stringify(v));
const source=()=>JSON.parse(readFileSync(new URL('../../../protocol/remote/inner/v1/test-vectors/projection-snapshot.json',import.meta.url),'utf8'));
function fixture(){const f=source(),requests:bigint[]=[],old:Uint8Array[]=[];let closed=0;
 const batch={stamp:f.body.stamp,parts:[encode(f)]};
 const port:BrowserNativeClientPort={snapshot:async p=>{old.push(p);return batch;},snapshotPull:async(n,b)=>{assert.equal(b,1000);requests.push(n);return batch;},exchange:async()=>{throw Error('unused');},close:async()=>{closed++;}};
 return{port,batch,requests,old,closed:()=>closed};}
const sink=()=>({begin(){},part(){}});
test('BrowserPullConnection_ExplicitCanonicalRequestAndOldDefault_001',async()=>{
 const f=fixture(),connection=explicit.browserPullConnection(f.port);
 await connection.snapshot(command('Snapshot','1'),sink());await connection.snapshot(command('Snapshot','18446744073709551615'),sink());
 assert.deepEqual(f.requests,[1n,18446744073709551615n]);assert.deepEqual(f.old,[]);connection.close();await connection.waitClosed();assert.equal(f.closed(),1);
 const old=fixture(),legacy=nativeConnection(old.port);await legacy.snapshot(command('Snapshot','2'),sink());assert.deepEqual(old.requests,[]);assert.deepEqual(old.old,[command('Snapshot','2')]);legacy.close();await legacy.waitClosed();
});
test('BrowserPullConnection_RejectsNoncanonicalWithoutSending_002',async()=>{
 for(const text of ['{"message":"Snapshot","profile":1,"request":"1","extra":true}','{"message":"Snapshot","profile":1,"request":"1","request":"2"}','{"message":"Snapshot","profile":1,"request":"01"}','{"message":"Window","profile":1,"request":"1"}',' {"message":"Snapshot","profile":1,"request":"1"}','null']){
  const f=fixture(),connection=explicit.browserPullConnection(f.port);await assert.rejects(connection.snapshot(new TextEncoder().encode(text),sink()));await connection.waitClosed();assert.deepEqual(f.requests,[]);assert.deepEqual(f.old,[]);assert.equal(f.closed(),1);
 }
});
test('BrowserPullConnection_NoCapabilityNeverFallsBack_003',async()=>{
 const f=fixture();f.port.snapshotPull=async()=>{throw Error('authenticated fact missing');};const connection=explicit.browserPullConnection(f.port);
 await assert.rejects(connection.snapshot(command('Snapshot','1'),sink()));await connection.waitClosed();assert.deepEqual(f.old,[]);assert.equal(f.closed(),1);
});
test('BrowserPullConnection_OriginalPendingAndLateClose_004',async()=>{
 const f=fixture();let release!:(v:typeof f.batch)=>void;f.port.snapshotPull=async()=>await new Promise(r=>{release=r;});
 const connection=explicit.browserPullConnection(f.port),pending=connection.snapshot(command('Snapshot','1'),sink());void pending.catch(()=>{});
 await assert.rejects(connection.snapshot(command('Snapshot','2'),sink()));connection.close();release(f.batch);await assert.rejects(pending);await connection.waitClosed();assert.equal(f.closed(),1);
});
test('BrowserPullConnection_PreservesMethodReceiverExchangeAndInvalidation_005',async()=>{
 const f=fixture();let invalidated!:()=>void,notified=0;const sent:Uint8Array[]=[];
 f.port.onInvalidate=function(listener){assert.equal(this,f.port);invalidated=listener;return()=>{};};
 f.port.snapshotPull=async function(request){assert.equal(this,f.port);assert.equal(request,1n);return f.batch;};
 f.port.exchange=async function(payload){assert.equal(this,f.port);sent.push(payload);return encode({message:'WindowGranted',profile:1,request:'2',window:'09'.repeat(32),notAfterHostMs:'2000'});};
 const close=f.port.close;f.port.close=async function(){assert.equal(this,f.port);return close();};
 const connection=explicit.browserPullConnection(f.port);assert.ok(connection.onInvalidate);connection.onInvalidate(()=>{notified++;});
 await connection.snapshot(command('Snapshot','1'),sink());await connection.query(command('Window','2'));assert.deepEqual(sent,[command('Window','2')]);
 invalidated();await connection.waitClosed();assert.equal(f.closed(),1);assert.equal(notified,1);await assert.rejects(connection.snapshot(command('Snapshot','3'),sink()));
});