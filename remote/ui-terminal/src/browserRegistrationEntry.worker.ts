import {startBrowserRegistrationWorker} from './browserRegistrationWorker.ts';
import type {BrowserRegistrationBridge} from './browserRegistrationBridge.ts';
import {browserTransport} from './browserTransport.ts';
import {readRegistrationArtifact,importRegistrationArtifact} from './browserRegistrationArtifacts.ts';

export type BrowserRegistrationWorkerStart=Readonly<{kind:'start';origin:string;artifacts:Readonly<{wasm:string;glue:string}>;localSpki:Uint8Array;hostSpki:Uint8Array;bridge:BrowserRegistrationBridge;transport:MessagePort}>;
export async function runBrowserRegistrationEntryWorker(init:BrowserRegistrationWorkerStart,signal:AbortSignal,report:(value:unknown)=>void):Promise<void>{
 const wire=browserTransport(init.transport),controller=new AbortController();
 let owner:Awaited<ReturnType<typeof startBrowserRegistrationWorker>>|undefined;
 const cancel=()=>{controller.abort();owner?.close();wire.close();};signal.addEventListener('abort',cancel,{once:true});
 const setup=init.bridge.signer.setupDeadline;
 const firstPerf=performance.now(),firstWall=Date.now(),epoch=performance.timeOrigin+firstPerf;
 let lastPerf=firstPerf,lastWall=firstWall,clockFailed=false,established=false;
 const clock=()=>{const p=performance.now(),w=Date.now();if(!Number.isFinite(p)||!Number.isFinite(w)||p<lastPerf||w<lastWall||Math.max(p-lastPerf,w-lastWall)>1000)clockFailed=true;lastPerf=p;lastWall=w;return clockFailed?Infinity:epoch+Math.max(p-firstPerf,w-firstWall);};
 const live=()=>{if(controller.signal.aborted||clock()>=(setup??NaN))throw Error('browser registration Worker expired');};
 const timer=setTimeout(cancel,Math.max(0,(setup??NaN)-performance.timeOrigin-performance.now()));
 const watchdog=setInterval(()=>{if(clock()>=(established?init.bridge.deadline:(setup??NaN)))cancel();},100);
 let fallback:Promise<void>|undefined;
 try{
  const scope=globalThis as typeof globalThis&{DedicatedWorkerGlobalScope?:new()=>object};
  if(init.kind!=='start'||init.origin!==location.origin||init.bridge.origin!==init.origin||!isSecureContext||!crossOriginIsolated||typeof scope.DedicatedWorkerGlobalScope!=='function'||!(globalThis instanceof scope.DedicatedWorkerGlobalScope)||signal.aborted||typeof setup!=='number'||!Number.isFinite(setup)||setup<=performance.timeOrigin+performance.now())throw Error('browser registration Worker unavailable');
  live();const wasm=await readRegistrationArtifact(init.artifacts.wasm,init.origin,8388608,controller.signal);live();
  const bytes=await readRegistrationArtifact(init.artifacts.glue,init.origin,1048576,controller.signal);live();
  const glue=await importRegistrationArtifact(bytes,controller.signal);live();
  owner=await startBrowserRegistrationWorker({wasm,glue,bridge:init.bridge,localSpki:init.localSpki,peerSpki:init.hostSpki,transport:wire,signal:controller.signal});
  const evidence=owner.evidence.then(value=>{established=true;clearTimeout(timer);report({kind:'evidence',value});});
  const outcome=owner.outcome.then(value=>report({kind:'outcome',value}));
  const completed=owner.completed.then(()=>report({kind:'completed'}));
  for(const work of [evidence,outcome,completed])void work.catch(()=>{});
  const facts=await Promise.allSettled([evidence,outcome,completed,owner.waitClosed()]);
  if(facts.slice(0,3).some(f=>f.status==='rejected'))report({kind:'failed'});
  if(facts[3]!.status==='rejected')throw Error('browser registration retirement uncertain');
 }catch{
  report({kind:'failed'});
  // 未建立 WASM owner 时仍加入原桥的退休握手；不把 Worker.terminate 当作证明。
  if(!owner){fallback=new Promise<void>((resolve,reject)=>{
   init.bridge.port.onmessage=e=>{if(e.data==='retired'||e.data==='retirement-failed'){init.bridge.port.close();init.bridge.signer.port.close();if(e.data==='retirement-failed')reject(Error('browser registration bridge retirement uncertain'));else resolve();}};
   init.bridge.port.onmessageerror=()=>reject(Error('browser registration bridge retirement uncertain'));init.bridge.port.start();init.bridge.port.postMessage('retired');
  });void fallback.catch(()=>{});}
  else throw Error('browser registration retirement uncertain');
 }finally{
  clearTimeout(timer);clearInterval(watchdog);cancel();
  const facts=await Promise.allSettled([owner?.waitClosed(),wire.waitClosed(),fallback]);
  signal.removeEventListener('abort',cancel);
  if(facts.some(f=>f.status==='rejected'))throw Error('browser registration retirement uncertain');
 }
}
const scope=globalThis as typeof globalThis&{DedicatedWorkerGlobalScope?:new()=>object};
if(typeof scope.DedicatedWorkerGlobalScope==='function'&&globalThis instanceof scope.DedicatedWorkerGlobalScope){
 const controller=new AbortController();let started=false;
 globalThis.addEventListener('message',event=>{
  const value=(event as MessageEvent).data;
  if(value?.kind==='close'){controller.abort();return;}
  if(started||controller.signal.aborted||value?.kind!=='start'){controller.abort();return;}started=true;
  void runBrowserRegistrationEntryWorker(value,controller.signal,value=>globalThis.postMessage(value)).then(()=>globalThis.postMessage({kind:'retired'}),()=>globalThis.postMessage({kind:'retired',uncertain:true}));
 });
}
