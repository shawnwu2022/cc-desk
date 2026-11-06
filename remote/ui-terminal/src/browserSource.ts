import {approvedBrowserInputs,type ApprovedBrowserInputs} from './browserEntry.ts';
import type {TerminalConnectionSource} from './connectionSource.ts';
import {createBrowserRelay} from './browserRelay.ts';
import {serveBrowserTransport} from './browserTransport.ts';
import {createBrowserTlsSignerBroker} from './browserTlsSigner.ts';
import {createBrowserGrantBroker} from './browserGrant.ts';
import {browserClientPort} from './browserClient.ts';
import {nativeConnection} from './native.ts';
export function browserConnectionSource(inputs:ApprovedBrowserInputs|undefined):TerminalConnectionSource{
 const approved=inputs===undefined?undefined:approvedBrowserInputs(inputs);let occupied=false;
 return Object.freeze({async open(signal:AbortSignal,deadline=performance.timeOrigin+performance.now()+10000){
  if(!approved)return undefined;
  if(occupied||signal.aborted||typeof Window==='undefined'||!(globalThis instanceof Window)||globalThis.top!==globalThis.self||!globalThis.isSecureContext||!globalThis.crossOriginIsolated||location.origin!==approved.profile.origin||typeof Document==='undefined'||!(document instanceof Document)||document.visibilityState!=='visible'||typeof Worker!=='function')throw Error('browser entry unavailable');
  if(!Number.isFinite(deadline)||deadline<=performance.timeOrigin+performance.now()||deadline>performance.timeOrigin+performance.now()+10000)throw Error('browser entry deadline');
  occupied=true;const controller=new AbortController(),source=document;
  let relay:ReturnType<typeof createBrowserRelay>|undefined,transport:ReturnType<typeof serveBrowserTransport>|undefined,tls:ReturnType<typeof createBrowserTlsSignerBroker>|undefined,grant:ReturnType<typeof createBrowserGrantBroker>|undefined,worker:Worker|undefined,port:ReturnType<typeof browserClientPort>|undefined,closing:Promise<void>|undefined,setupTimer:ReturnType<typeof setTimeout>|undefined;
  let resolveRetired!:()=>void,rejectRetired!:(e:Error)=>void;const retired=new Promise<void>((a,b)=>{resolveRetired=a;rejectRetired=b;});void retired.catch(()=>{});
  const lifecycle=()=>{if(document!==source||source.visibilityState!=='visible')void close();};
  const abort=()=>{void close();};
  function close(){if(closing)return closing;if(setupTimer!==undefined)clearTimeout(setupTimer);controller.abort();tls?.close();grant?.close();relay?.close();transport?.close();try{worker?.postMessage({kind:'close'});}catch{rejectRetired(Error('browser Worker retirement uncertain'));}
   closing=Promise.resolve().then(async()=>{
    const facts=await Promise.allSettled([tls?.waitClosed(),grant?.waitClosed(),relay?.waitClosed(),transport?.waitClosed(),worker?retired:undefined]);
    worker?.terminate();source.removeEventListener('visibilitychange',lifecycle);source.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);signal.removeEventListener('abort',abort);occupied=false;
    if(facts.some(f=>f.status==='rejected')){occupied=true;throw Error('browser entry retirement uncertain');}
   });void closing.catch(()=>{});return closing;
  }
  signal.addEventListener('abort',abort,{once:true});source.addEventListener('visibilitychange',lifecycle);source.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);
  const expire=()=>{if(performance.timeOrigin+performance.now()<deadline){setupTimer=setTimeout(expire,Math.max(0,deadline-performance.timeOrigin-performance.now()));return;}void close();};setupTimer=setTimeout(expire,Math.max(0,deadline-performance.timeOrigin-performance.now()));
  try{
   relay=createBrowserRelay({origin:approved.relay.endpoint,tunnel:approved.relay.tunnel,identity:approved.relay.identity,setupDeadline:deadline,ioBudgetMs:30000,socket:new WebSocket(approved.relay.endpoint),signal:controller.signal});
   const wire=await relay.ready;if(controller.signal.aborted)throw Error('browser entry cancelled');
   tls=createBrowserTlsSignerBroker(approved.profile,{timeoutMs:10000,setupDeadline:deadline,signal:controller.signal});
   grant=createBrowserGrantBroker(approved.profile,{configuration:approved.grant,present:approved.present,signal:controller.signal});
   const io=new MessageChannel(),application=new MessageChannel();transport=serveBrowserTransport(io.port1,wire);
   worker=new Worker(new URL('./browserEntry.worker.ts',import.meta.url),{type:'module',name:'cc-desk-remote'});
   worker.addEventListener('message',event=>{if(event.data?.kind==='retired'){if(event.data.uncertain)rejectRetired(Error('browser Worker retirement uncertain'));else resolveRetired();}else{rejectRetired(Error('browser Worker control'));void close();}});
   worker.addEventListener('error',()=>{rejectRetired(Error('browser Worker failed'));void close();});worker.addEventListener('messageerror',()=>{rejectRetired(Error('browser Worker message'));void close();});
   port=browserClientPort(application.port1,()=>{void close();},()=>retired,grant.worker.cancel!);
   worker.postMessage({kind:'start',origin:approved.profile.origin,artifacts:approved.artifacts,localSpki:approved.localSpki,hostSpki:approved.hostSpki,trust:approved.trust,tls:tls.worker,grant:grant.worker,transport:io.port2,application:application.port2},[tls.worker.port,grant.worker.port,io.port2,application.port2]);
   const descriptor=await port.descriptor;if(controller.signal.aborted||performance.timeOrigin+performance.now()>=deadline)throw Error('browser entry cancelled');if(setupTimer!==undefined)clearTimeout(setupTimer);setupTimer=undefined;
   const original=port,connection=nativeConnection({...original,close:async()=>{let error:unknown;try{await original.close();}catch(e){error=e;}try{await close();}catch(e){error??=e;}if(error)throw error;}});
   return Object.freeze({connection,target:descriptor.target,binding:descriptor.binding,role:descriptor.role==='Controller'?'Controller' as const:'Viewer' as const});
  }catch{port?.close().catch(()=>{});await close();throw Error('browser entry unavailable');}
 }});
}
