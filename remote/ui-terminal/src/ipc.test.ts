import test from 'node:test';
import assert from 'node:assert/strict';
import {documentClientPort} from './ipc.ts';

test('document IPC port uses only original bootstrap bridge and bounded byte DTO',async()=>{
 const calls:any[]=[];(globalThis as any).window={__CC_DESK_DOCUMENT__:{instanceId:"doc-fixture",async invoke(command:string,body:any){calls.push({command,body});return {payload:'AQID'};}}};
 const port=documentClientPort(7);assert.deepEqual(await port.exchange(new Uint8Array([1,2,3]),1000),new Uint8Array([1,2,3]));assert.deepEqual(calls[0],{command:'remote_client_exchange',body:{resourceId:7,budgetMs:1000,payload:'AQID'}});
 await assert.rejects(port.exchange(new Uint8Array(16271),1000));assert.equal(calls.length,1);
 delete (globalThis as any).window.__CC_DESK_DOCUMENT__;await assert.rejects(port.exchange(new Uint8Array([1]),1000),/document/);
});
test('document IPC snapshot and close validate response bounds without fallback',async()=>{
 let closed=false;(globalThis as any).window={__CC_DESK_DOCUMENT__:{instanceId:"doc-fixture",async invoke(command:string){if(command==='remote_client_close'){closed=true;return null;}return {stamp:{},parts:['AQ==']};}}};
 const port=documentClientPort(1);await assert.rejects(port.snapshot(new Uint8Array([1]),1));await port.close();assert.equal(closed,true);
});

test('closed document IPC port cannot issue or replay native requests',async()=>{
 let calls=0;(globalThis as any).window={__CC_DESK_DOCUMENT__:{instanceId:"doc-fixture",async invoke(command:string){calls++;return command==='remote_client_close'?null:{payload:'AQID'};}}};const port=documentClientPort(1);await port.close();await port.close();await assert.rejects(port.exchange(new Uint8Array([1]),1000));assert.equal(calls,1);
});

test('a port cannot silently rebind to a replacement document bridge',async()=>{
 (globalThis as any).window={__CC_DESK_DOCUMENT__:{instanceId:'original',async invoke(){return {payload:'AQ'};}}};const port=documentClientPort(1);let calls=0;(globalThis as any).window.__CC_DESK_DOCUMENT__={instanceId:'next',async invoke(){calls++;return {payload:'AQ'};}};await assert.rejects(port.exchange(new Uint8Array([1]),1000));assert.equal(calls,0);
});
