import {startBrowserTlsWorker,type BrowserTlsWorker} from './browserTlsWorker.ts';
import {serveBrowserClient} from './browserClient.ts';
import {browserTransport} from './browserTransport.ts';
import type {BrowserTlsWorkerBridge} from './browserTlsSigner.ts';
import type {BrowserGrantBridge} from './browserGrant.ts';
import type {BrowserClientTrust} from './browserTlsWorker.ts';
export type BrowserWorkerStart=Readonly<{kind:'start';origin:string;artifacts:Readonly<{wasm:string;glue:string}>;localSpki:Uint8Array;hostSpki:Uint8Array;trust:BrowserClientTrust;tls:BrowserTlsWorkerBridge;grant:BrowserGrantBridge;transport:MessagePort;application:MessagePort}>;
/** Controlled module Worker. Keys/presenter remain on the original page. */
export async function runBrowserEntryWorker(init:BrowserWorkerStart,signal:AbortSignal):Promise<void>{
 const wire=browserTransport(init.transport);let owner:BrowserTlsWorker|undefined,served=false;
 const controller=new AbortController(),cancel=()=>{controller.abort();owner?.close();wire.close();};signal.addEventListener('abort',cancel,{once:true});
 const timer=setTimeout(cancel,Math.max(0,(init.tls.setupDeadline??NaN)-performance.timeOrigin-performance.now()));
 try{
  if(init.kind!=='start'||init.origin!==location.origin||!globalThis.isSecureContext||!globalThis.crossOriginIsolated||typeof (globalThis as any).DedicatedWorkerGlobalScope!=='function'||!(globalThis instanceof (globalThis as any).DedicatedWorkerGlobalScope)||signal.aborted)throw Error('browser entry Worker unavailable');
  for(const address of [init.artifacts.wasm,init.artifacts.glue]){const url=new URL(address);if(url.href!==address||url.origin!==init.origin||url.username||url.password||url.search||url.hash)throw Error('browser artifact origin');}
  const response=await fetch(init.artifacts.wasm,{signal:controller.signal,credentials:'omit',redirect:'error',cache:'no-store'});if(!response.ok||!response.body||Number(response.headers.get('content-length'))>8388608)throw Error('browser WASM response');
  const reader=response.body.getReader(),storage=new Uint8Array(8388608);let length=0;
  try{for(;;){const value=await reader.read();if(value.done)break;if(value.value.length>storage.length-length)throw Error('browser WASM bounds');storage.set(value.value,length);length+=value.value.length;}}finally{await reader.cancel();reader.releaseLock();}
  if(controller.signal.aborted)throw Error('browser entry cancelled');const bytes=storage.slice(0,length);
  const glue=await import(/* @vite-ignore */ init.artifacts.glue);if(controller.signal.aborted)throw Error('browser entry cancelled');
  owner=await startBrowserTlsWorker({wasm:bytes,glue,bridge:init.tls,localSpki:init.localSpki,peerSpki:init.hostSpki,transport:wire,signal:controller.signal,clientTrust:init.trust,grantBridge:init.grant,application:true});
  if(!owner.application)throw Error('browser original application unavailable');
  serveBrowserClient(init.application,owner.application.port,owner.application.descriptor,owner.close,owner.waitClosed,owner.application.limitDeadline);
  served=true;void owner.application.descriptor.then(()=>clearTimeout(timer),cancel);
  await owner.waitClosed();
 }catch{/* Operation failure seals admission; retirement facts are collected below. */}
 finally{clearTimeout(timer);cancel();const facts=await Promise.allSettled([owner?.waitClosed(),wire.waitClosed()]);signal.removeEventListener('abort',cancel);const uncertain=facts.some(f=>f.status==='rejected');if(!served){init.application.postMessage({kind:'invalidated'});init.application.postMessage({kind:'retired',uncertain});init.application.close();}if(uncertain)throw Error('browser entry Worker retirement uncertain');}
}
const scope=globalThis as typeof globalThis&{DedicatedWorkerGlobalScope?:new()=>object};
if(typeof scope.DedicatedWorkerGlobalScope==='function'&&globalThis instanceof scope.DedicatedWorkerGlobalScope){
 const controller=new AbortController();let started=false;
 globalThis.addEventListener('message',event=>{const value=(event as MessageEvent).data;if(value?.kind==='close'){controller.abort();return;}if(started||value?.kind!=='start'){controller.abort();return;}started=true;
  void runBrowserEntryWorker(value,controller.signal).then(()=>globalThis.postMessage({kind:'retired'}),()=>globalThis.postMessage({kind:'retired',uncertain:true}));
 });
}
