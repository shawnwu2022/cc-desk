import test from 'node:test';
import assert from 'node:assert/strict';
import * as ipc from './ipc.ts';

const ready=()=>({state:'Ready',resourceId:7,target:{host:'ab'.repeat(32),session:'cd'.repeat(16),run:'owned',generation:'1',incarnation:'2'},binding:'ef'.repeat(32),role:'Controller'});
function install(reply:unknown|(()=>Promise<unknown>)){
 (globalThis as any).window={__CC_DESK_DOCUMENT__:{instanceId:'original',async invoke(command:string,body:unknown){assert.equal(command,'remote_client_current');assert.deepEqual(body,{});return typeof reply==='function'?reply():reply;}}};
}
test('native session discovery is original-document only and remains locked without a native client',async()=>{
 assert.equal(typeof (ipc as any).documentClientStatus,'function');
 install({state:'Locked'});assert.deepEqual(await (ipc as any).documentClientStatus(),{state:'Locked'});
 delete (globalThis as any).window;await assert.rejects((ipc as any).documentClientStatus());
});
test('native session descriptor validates exact identity and returns an immutable safe copy',async()=>{
 install(ready());const result=await (ipc as any).documentClientStatus();assert.deepEqual(result,ready());assert.equal(Object.isFrozen(result.target),true);
 for(const value of [{...ready(),authority:true},{...ready(),resourceId:0},{...ready(),role:'Admin'},{...ready(),binding:'00'},{...ready(),target:{...ready().target,generation:'01'}},{state:'Locked',resourceId:1}]){
  install(value);await assert.rejects((ipc as any).documentClientStatus());
 }
});
test('late native discovery cannot move a client to a replacement document',async()=>{
 let release!:(v:unknown)=>void;install(()=>new Promise(resolve=>{release=resolve;}));const pending=(ipc as any).documentClientStatus();
 install(ready());release(ready());await assert.rejects(pending,/document/);
});
