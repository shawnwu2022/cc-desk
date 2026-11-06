import {parentPort, workerData} from 'node:worker_threads';
import {createBrowserTlsWorkerSigner} from './browserTlsSigner.ts';
import {createBrowserTlsWasmImports} from './browserTlsWasm.ts';

// 仅模拟浏览器平台识别；线程、端口、共享内存与密码运算仍是真实 Node 实现。
class DedicatedFixture {static [Symbol.hasInstance](value:unknown){return value===globalThis;}}
Object.defineProperties(globalThis,{
 DedicatedWorkerGlobalScope:{value:DedicatedFixture,configurable:true},
 isSecureContext:{value:true,configurable:true},
 crossOriginIsolated:{value:workerData.isolated!==false,configurable:true},
 location:{value:{origin:'https://desk.invalid'},configurable:true},
});
if(workerData.wasm){
 try{
  let memory:WebAssembly.Memory|undefined;
  const adapter=createBrowserTlsWasmImports(workerData.bridge,workerData.spki,()=>memory);
  const callbacks=adapter.imports.cc_desk_browser_tls_v1 as Record<string,(...args:number[])=>number>;
  const originalSign=callbacks.sign;
  let api:{memory:WebAssembly.Memory;fixture_init(d:number,k:number):number;fixture_sign(k:number):number;fixture_signature_byte(i:number):number;fixture_close():void;fixture_grow():number};
  if(workerData.fault)callbacks.sign=(...args:number[])=>{
   if(workerData.fault==='sequence')args[0]++;
   if(workerData.fault==='pointer')args[2]=0xffffffff;
   if(workerData.fault==='capacity')args[5]=73;
   if(workerData.fault==='overlap')args[4]=args[2];
   if(workerData.fault==='reenter'){api.fixture_sign(0);}
   const result=originalSign(...args);
   if(workerData.fault==='length'&&result>0)return 73;
   if(workerData.fault==='signature'&&result>0)new Uint8Array(memory!.buffer,args[4],72).fill(0);
   return result;
  };
  const glue=await import(workerData.glueUrl);
  const module=await WebAssembly.compile(workerData.wasm),instance=await WebAssembly.instantiate(module,{...adapter.imports,'./fixture_bg.js':glue});
  api=instance.exports as unknown as typeof api;
  memory=api.memory;glue.__wbg_set_wasm(instance.exports);
  (instance.exports.__wbindgen_start as ()=>void)();
  parentPort!.postMessage({ready:true,imports:WebAssembly.Module.imports(module)});
  parentPort!.on('message',command=>{
   try{
    if(command.kind==='duplicateFactory'){
     try{createBrowserTlsWasmImports(workerData.bridge,workerData.spki,()=>memory);parentPort!.postMessage({unexpected:true});}
     catch(error){parentPort!.postMessage({error:(error as Error).message});}return;
    }
    if(command.kind==='swapMemory'){memory=new WebAssembly.Memory(command.shared?{initial:1,maximum:1,shared:true}:{initial:1});parentPort!.postMessage({swapped:true});return;}
    if(command.kind==='init'){parentPort!.postMessage({result:api.fixture_init(command.deadline??workerData.bridge.setupDeadline,command.wrongIdentity??0)});return;}
    if(command.kind==='close'){api.fixture_close();parentPort!.postMessage({closed:true});return;}
    if(command.kind==='grow'){const previous=memory!.buffer;api.fixture_grow();parentPort!.postMessage({changed:previous!==memory!.buffer});return;}
    const length=api.fixture_sign(command.case??0);
    parentPort!.postMessage({result:length,signature:length>0?Uint8Array.from({length},(_,i)=>api.fixture_signature_byte(i)):undefined});
   }catch(error){parentPort!.postMessage({error:(error as Error).message});}
  });
 }catch(error){parentPort!.postMessage({error:(error as Error).message});}
}else try{
 const bridge=workerData.bridge;
 const signer=createBrowserTlsWorkerSigner(bridge);
 parentPort!.postMessage({ready:true});
 parentPort!.on('message',(command)=>{
  if(command.kind==='raw'){
   const control=new Int32Array(bridge.shared,0,5);
   Atomics.store(control,1,1);Atomics.store(control,2,command.seq);
   bridge.port.postMessage({generation:bridge.generation,seq:command.seq,deadline:performance.timeOrigin+performance.now()+bridge.timeoutMs,message:command.message,...command.override});
   return;
  }
  if(command.kind==='close'){signer.close();parentPort!.postMessage({closed:true});return;}
  if(command.kind==='duplicateAttach'){
   try{createBrowserTlsWorkerSigner(bridge);parentPort!.postMessage({unexpected:true});}
   catch(error){parentPort!.postMessage({error:(error as Error).message});}return;
  }
  if(command.started)Atomics.store(new Int32Array(command.started),0,1),Atomics.notify(new Int32Array(command.started),0);
  const nativeWait=Atomics.wait;
  if(command.waitGate){
   const gate=new Int32Array(command.waitGate);let intercepted=false;
   // 只控制真实 wait 前的调度缺口；生产等待仍调用原生 Atomics.wait。
   Atomics.wait=((array:Int32Array,index:number,value:number,timeout?:number)=>{
    if(!intercepted&&array.buffer===bridge.shared&&index===1){
     intercepted=true;parentPort!.postMessage({kind:'beforeWait',timeoutMs:timeout});
     if(nativeWait(gate,0,0,5000)==='timed-out')throw Error('fixture wait gate timeout');
     const start=performance.now(),outcome=nativeWait(array,index,value,timeout);
     parentPort!.postMessage({kind:'waitResult',outcome,elapsedMs:performance.now()-start,timeoutMs:timeout});return outcome;
    }
    return nativeWait(array,index,value,timeout);
   }) as typeof Atomics.wait;
  }
  try{
   const signature=signer.sign(command.message);
   parentPort!.postMessage({signature});
  }catch(error){parentPort!.postMessage({error:(error as Error).message});}
  finally{Atomics.wait=nativeWait;}
 });
}catch(error){parentPort!.postMessage({error:(error as Error).message});}
