import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readRegistrationArtifact} from './browserRegistrationArtifacts.ts';
test('RegistrationArtifacts_RejectsOriginsWithoutFetch_001',async()=>{
 const original=globalThis.fetch;let calls=0;globalThis.fetch=async()=>{calls++;throw Error('TEST_ONLY unexpected fetch');};
 try{for(const address of ['http://desk.invalid/a','https://other.invalid/a','https://desk.invalid/a#b','https://desk.invalid/a?q=1'])await assert.rejects(readRegistrationArtifact(address,'https://desk.invalid',16,new AbortController().signal));assert.equal(calls,0);}finally{globalThis.fetch=original;}
});
test('RegistrationArtifacts_BoundsChunkedAndDeclaredBytes_002',async()=>{
 const original=globalThis.fetch;
 try{for(const declared of [false,true]){let cancelled=false;globalThis.fetch=async(_url,init)=>{assert.equal(init?.redirect,'error');assert.equal(init?.credentials,'omit');return new Response(new ReadableStream({start(c){c.enqueue(new Uint8Array(17));},cancel(){cancelled=true}}),{headers:declared?{'content-length':'17'}:{}});};await assert.rejects(readRegistrationArtifact('https://desk.invalid/a','https://desk.invalid',16,new AbortController().signal));if(!declared)assert.equal(cancelled,true);}}finally{globalThis.fetch=original;}
});
test('RegistrationArtifacts_HeldFetchRetainsCancellation_003',async()=>{
 const original=globalThis.fetch,controller=new AbortController();let release!:(r:Response)=>void;
 globalThis.fetch=async()=>new Promise(r=>{release=r;});
 try{let done=false;const work=readRegistrationArtifact('https://desk.invalid/a','https://desk.invalid',16,controller.signal).finally(()=>{done=true});void work.catch(()=>{});controller.abort();await Promise.resolve();assert.equal(done,false);release(new Response(new Uint8Array([1])));await assert.rejects(work);}finally{globalThis.fetch=original;}
});
