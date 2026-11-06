import {parentPort, workerData} from 'node:worker_threads';
import {createBrowserTlsWorkerSigner} from './browserTlsSigner.ts';

// 仅模拟浏览器平台识别；线程、端口、共享内存与密码运算仍是真实 Node 实现。
class DedicatedFixture {static [Symbol.hasInstance](value:unknown){return value===globalThis;}}
Object.defineProperties(globalThis,{
 DedicatedWorkerGlobalScope:{value:DedicatedFixture,configurable:true},
 isSecureContext:{value:true,configurable:true},
 crossOriginIsolated:{value:workerData.isolated!==false,configurable:true},
 location:{value:{origin:'https://desk.invalid'},configurable:true},
});
try{
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
