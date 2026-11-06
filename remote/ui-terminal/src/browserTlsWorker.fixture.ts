import {parentPort,workerData} from 'node:worker_threads';
import {createConnection} from 'node:net';
import {startBrowserTlsWorker,type BrowserTlsTransport,type BrowserTlsWorker} from './browserTlsWorker.ts';
class DedicatedFixture {static [Symbol.hasInstance](value:unknown){return value===globalThis;}}
Object.defineProperties(globalThis,{DedicatedWorkerGlobalScope:{value:DedicatedFixture},isSecureContext:{value:true},crossOriginIsolated:{value:true},location:{value:{origin:'https://desk.invalid'}}});
const controller=new AbortController(),gate=new Map<number,()=>void>();let writeSequence=0,owner:BrowserTlsWorker|undefined,evidenced=false,retired=false,terminalCode:number|undefined,releaseLoad:(()=>void)|undefined,probeWrite:(()=>number)|undefined,signed=false,finalFlushWrite=0;
let tlsReported=false,managementPublished=false,bindFinalWrite=0;const bindFlushWrites=new Set<number>();
if(workerData.preAborted)controller.abort();
parentPort!.on('message',message=>{
 if(message.kind==='abort')controller.abort();
 if(message.kind==='close'){if(owner)owner.close();else controller.abort();}
 if(message.kind==='shutdown')owner?.shutdown();
 if(message.kind==='releaseWrite'){gate.get(message.sequence)?.();gate.delete(message.sequence);}
 if(message.kind==='releaseClose'){gate.get(0)?.();gate.delete(0);}
 if(message.kind==='releaseLoad')releaseLoad?.();
 if(message.kind==='releaseAll'){for(const release of gate.values())release();gate.clear();}
 if(message.kind==='probeWrite')parentPort!.postMessage({kind:'writeProbed',result:probeWrite?.()});
 if(message.kind==='observe')parentPort!.postMessage({kind:'observed',evidenced,retired,terminalCode,writeSequence,managementPublished,ownerStarted:owner!==undefined});
});
const nativeCompile=WebAssembly.compile,nativeInstantiate=WebAssembly.instantiate;
function holdLoad(){parentPort!.postMessage({kind:'loadHeld'});return new Promise<void>(resolve=>{releaseLoad=resolve;});}
if(workerData.holdLoad==='compile')WebAssembly.compile=async(bytes)=>{const module=await nativeCompile(bytes);await holdLoad();return module;};
WebAssembly.instantiate=(async(module:WebAssembly.Module,imports:WebAssembly.Imports)=>{
 const original=imports.cc_desk_browser_owner_v1 as Record<string,(...args:number[])=>void>;
 const signing=imports.cc_desk_browser_tls_v1 as Record<string,(...args:number[])=>number>;
 const adjusted={...imports,cc_desk_browser_tls_v1:{...signing,sign:(...args:number[])=>{const result=signing.sign(...args);if(result>=8)signed=true;return result;}},cc_desk_browser_owner_v1:{...original,flush:()=>{const result=original.flush() as unknown as number;if(signed&&result===-1)finalFlushWrite=writeSequence;if(tlsReported&&result===-1){bindFlushWrites.add(writeSequence);if(bindFlushWrites.size===2)bindFinalWrite=writeSequence;}return result;},write:(...args:number[])=>{probeWrite=()=>original.write(...args) as unknown as number;return original.write(...args);},management:(...args:number[])=>{
  if(workerData.cancelAtManagement)controller.abort();
  if(workerData.expireAtManagement)while(performance.timeOrigin+performance.now()<workerData.bridge.setupDeadline+1){}
  original.management(...args);
 },report:(...args:number[])=>{
  if(args[0]===1)tlsReported=true;
  if(args[0]===1&&workerData.cancelAtEvidence)controller.abort();
  if(args[0]===1&&workerData.expireAtEvidence)while(performance.timeOrigin+performance.now()<workerData.bridge.setupDeadline+1){}
  if(args[0]<0){terminalCode=args[0];parentPort!.postMessage({kind:'terminal',code:args[0]});}
  original.report(...args);
 }}};
 const instance=await nativeInstantiate(module,adjusted);
 if(workerData.holdLoad==='instantiate')await holdLoad();
 if(!workerData.traceRuntime&&!workerData.rejectStart)return instance;
 const poll=instance.exports.browser_tls_poll as ()=>void,start=instance.exports.browser_tls_start as (l:number,p:number,b:number)=>number;
 return {exports:{...instance.exports,browser_tls_start(l:number,p:number,b:number){return start(l,p,workerData.rejectStart?0:b);},browser_tls_poll(){try{poll();}catch(error){parentPort!.postMessage({kind:'runtimeTrace',trace:String((error as Error).stack).slice(0,4000)});throw error;}}}} as unknown as WebAssembly.Instance;
}) as typeof WebAssembly.instantiate;
function socketTransport(port:number):BrowserTlsTransport{
 const socket=createConnection({host:'127.0.0.1',port,highWaterMark:16384});socket.pause();let ended=false,failure=false;
 const physicalClosed=new Promise<void>(resolve=>socket.once('close',()=>{ended=true;resolve();}));
 const closed=physicalClosed.then(()=>workerData.holdClose?new Promise<void>(resolve=>{gate.set(0,resolve);parentPort!.postMessage({kind:'closeHeld'});}):undefined);
 socket.on('error',()=>{failure=true;});socket.on('end',()=>{ended=true;});
 return {
  read(maximum){return new Promise((resolve,reject)=>{
   function cleanup(){socket.off('readable',sample);socket.off('end',sample);socket.off('close',sample);socket.off('error',sample);}
   function sample(){if(socket.readableLength){const drip=workerData.dripAfterTls&&tlsReported;const bytes=socket.read(Math.min(maximum,socket.readableLength,drip?1:maximum)) as Uint8Array;cleanup();const deliver=()=>{parentPort!.postMessage({kind:'readDelivered',length:bytes.length});resolve(Uint8Array.from(bytes));};if(drip)setTimeout(deliver,5);else deliver();}else if(failure){cleanup();reject(Error('TEST_ONLY socket read'));}else if(ended||socket.destroyed){cleanup();resolve(null);}}
   socket.on('readable',sample);socket.on('end',sample);socket.on('close',sample);socket.on('error',sample);sample();
  });},
  write(bytes){const sequence=++writeSequence;parentPort!.postMessage({kind:'writeStarted',sequence,length:bytes.length});return new Promise<void>((resolve,reject)=>{const onClose=()=>{socket.off('close',onClose);reject(Error('TEST_ONLY socket closed'));};socket.once('close',onClose);socket.write(bytes,error=>{socket.off('close',onClose);if(error){reject(error);return;}const final=workerData.holdFinalWrite&&finalFlushWrite===sequence,bindFinal=workerData.holdBindConfirm&&bindFinalWrite===sequence;const settle=()=>workerData.failWrite===sequence||(final&&workerData.failFinalWrite)||(bindFinal&&workerData.failBindConfirm)?reject(Error('TEST_ONLY delayed write failure')):resolve();if(workerData.holdWrite===sequence||final||bindFinal){gate.set(sequence,settle);parentPort!.postMessage({kind:'writeHeld',sequence,bindFinal});}else settle();});}).finally(()=>parentPort!.postMessage({kind:'writeSettled',sequence}));},
  close(){socket.destroy();},waitClosed:()=>closed,
 };
}
let releaseSilent!:()=>void;
const silent:BrowserTlsTransport={read:()=>new Promise(resolve=>{releaseSilent=()=>resolve(null);}),write:async()=>{},close(){releaseSilent?.();},waitClosed:()=>workerData.holdClose?new Promise<void>(resolve=>{gate.set(0,resolve);parentPort!.postMessage({kind:'closeHeld'});}):Promise.resolve()};
try{
 owner=await startBrowserTlsWorker({wasm:workerData.wasm,glue:await import(workerData.glueUrl),bridge:workerData.bridge,localSpki:workerData.localSpki,peerSpki:workerData.peerSpki,transport:workerData.port?socketTransport(workerData.port):silent,localRole:workerData.localRole,peerRole:workerData.peerRole,signal:controller.signal,clientTrust:workerData.clientTrust});
 parentPort!.postMessage({kind:'started'});
 void owner.management?.then(value=>{managementPublished=true;parentPort!.postMessage({kind:'management',value});},error=>parentPort!.postMessage({kind:'managementError',error:(error as Error).message}));
 void owner.evidence.then(value=>{evidenced=true;parentPort!.postMessage({kind:'evidence',value});},error=>parentPort!.postMessage({kind:'error',error:(error as Error).message}));
 void owner.waitClosed().then(()=>{retired=true;parentPort!.postMessage({kind:'retired'});},()=>parentPort!.postMessage({kind:'retireFailed'}));
}catch(error){parentPort!.postMessage({kind:'startupError',error:(error as Error).message});}
