import {approvedBrowserInputs,type ApprovedBrowserInputs} from './browserEntry.ts';
import type {TerminalConnectionSource} from './connectionSource.ts';
import {createBrowserRelay,createBrowserDataKeepaliveRelay,RENDEZVOUS_SUBPROTOCOL} from './browserRelay.ts';
import {serveBrowserTransport} from './browserTransport.ts';
import {createBrowserTlsSignerBroker} from './browserTlsSigner.ts';
import {createBrowserGrantBroker} from './browserGrant.ts';
import {browserClientPort} from './browserClient.ts';
import {nativeConnection} from './native.ts';
export function browserConnectionSource(inputs:ApprovedBrowserInputs|undefined):TerminalConnectionSource{
 const approved=inputs===undefined?undefined:approvedBrowserInputs(inputs);let occupied=false;
 return Object.freeze({async open(signal:AbortSignal,deadline=performance.timeOrigin+performance.now()+10000){
  if(!approved)return undefined;
  if(occupied)throw Error('browser entry unavailable');
  if(signal.aborted||approved.enrollment?.signal.aborted||typeof Window==='undefined'||!(globalThis instanceof Window)||globalThis.top!==globalThis.self||!globalThis.isSecureContext||!globalThis.crossOriginIsolated||location.origin!==approved.profile.origin||typeof Document==='undefined'||!(document instanceof Document)||document.visibilityState!=='visible'||typeof Worker!=='function'){await approved.enrollment?.retire?.();throw Error('browser entry unavailable');}
  if(!Number.isFinite(deadline)||deadline<=performance.timeOrigin+performance.now()||deadline>performance.timeOrigin+performance.now()+10000){await approved.enrollment?.retire?.();throw Error('browser entry deadline');}
  occupied=true;const controller=new AbortController(),source=document;
  let socket:WebSocket|undefined,relay:ReturnType<typeof createBrowserRelay>|undefined,transport:ReturnType<typeof serveBrowserTransport>|undefined,tls:ReturnType<typeof createBrowserTlsSignerBroker>|undefined,grant:ReturnType<typeof createBrowserGrantBroker>|undefined,worker:Worker|undefined,port:ReturnType<typeof browserClientPort>|undefined,closing:Promise<void>|undefined,setupTimer:ReturnType<typeof setTimeout>|undefined;
  let resolveSocket!:()=>void,rejectSocket!:(e:Error)=>void;const socketRetired=new Promise<void>((resolve,reject)=>{resolveSocket=resolve;rejectSocket=reject;});void socketRetired.catch(()=>{});
  let resolveRetired!:()=>void,rejectRetired!:(e:Error)=>void;const retired=new Promise<void>((a,b)=>{resolveRetired=a;rejectRetired=b;});void retired.catch(()=>{});
  let resolveOwned!:()=>void,rejectOwned!:(error:unknown)=>void;
  const owned=new Promise<void>((a,b)=>{resolveOwned=a;rejectOwned=b;});void owned.catch(()=>{});
  const lifecycle=()=>{if(document!==source||source.visibilityState!=='visible')void close();};
  const abort=()=>{void close();};
  function close(){
   if(closing)return closing;
   let resolveClose!:()=>void,rejectClose!:(error:unknown)=>void;
   // 先发布同一 closing，再触发可同步重入的 abort/retire。
   closing=new Promise<void>((resolve,reject)=>{resolveClose=resolve;rejectClose=reject;});void closing.catch(()=>{});
   if(setupTimer!==undefined)clearTimeout(setupTimer);controller.abort();tls?.close();grant?.close();relay?.close();transport?.close();try{worker?.postMessage({kind:'close'});}catch{rejectRetired(Error('browser Worker retirement uncertain'));}
   try{if(socket&&socket.readyState<2)socket.close();}catch{rejectSocket(Error('browser socket retirement uncertain'));}
   let storeRetirement:Promise<void>|undefined;
   try{storeRetirement=approved!.enrollment?.retire?.();}catch{storeRetirement=Promise.reject(Error('browser enrollment retirement uncertain'));}
   void storeRetirement?.catch(()=>{});
   const drain=Promise.resolve().then(async()=>{
    const facts=await Promise.allSettled([port?.close(),tls?.waitClosed(),grant?.waitClosed(),relay?.waitClosed(),transport?.waitClosed(),socket?socketRetired:undefined,worker?retired:undefined]);
    worker?.terminate();source.removeEventListener('visibilitychange',lifecycle);source.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);signal.removeEventListener('abort',abort);approved!.enrollment?.signal.removeEventListener('abort',abort);
    if(facts.some(f=>f.status==='rejected'))throw Error('browser entry retirement uncertain');
   });
   // 租约只保留原 transport 的完整退出；不能等待自身的 store 退休导致互锁。
   void drain.then(resolveOwned,rejectOwned);
   void Promise.allSettled([drain,storeRetirement]).then(facts=>{
    if(facts.some(f=>f.status==='rejected')){rejectClose(Error('browser entry retirement uncertain'));return;}
    occupied=false;resolveClose();
   });
   return closing;
  }
  signal.addEventListener('abort',abort,{once:true});source.addEventListener('visibilitychange',lifecycle);source.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);
  const expire=()=>{if(performance.timeOrigin+performance.now()<deadline){setupTimer=setTimeout(expire,Math.max(0,deadline-performance.timeOrigin-performance.now()));return;}void close();};setupTimer=setTimeout(expire,Math.max(0,deadline-performance.timeOrigin-performance.now()));
  try{
   if(approved.enrollment){
    // Register the original complete retirement before any socket, key use or Worker.
    approved.enrollment.retain(owned);
    approved.enrollment.signal.addEventListener('abort',abort,{once:true});
    if(approved.enrollment.signal.aborted)throw Error('browser enrollment revoked');
   }
   // Relay 构造的再次校验可能失败；先独立持有原 socket 的物理退出。
   socket=approved.relay.mode==='rendezvous-v1'?new WebSocket(approved.relay.endpoint,RENDEZVOUS_SUBPROTOCOL):new WebSocket(approved.relay.endpoint);
   socket.addEventListener('close',resolveSocket,{once:true});if(socket.readyState===3)resolveSocket();
   const live=approved.grant.peerInstall?.controlLease==='live-v1';
   const setupEnd=Math.floor(deadline),maxEnd=Math.floor(performance.timeOrigin+performance.now()+900000);
   const relayOptions={origin:approved.relay.endpoint,tunnel:approved.relay.tunnel,identity:approved.relay.identity,setupDeadline:live?setupEnd:deadline,ioBudgetMs:30000,mode:approved.relay.mode,socket,signal:controller.signal};
   const dormant=live?createBrowserDataKeepaliveRelay({...relayOptions,maxDeadline:maxEnd}):undefined;
   relay=dormant?.owner??createBrowserRelay(relayOptions);
   const wire=await relay.ready;if(controller.signal.aborted)throw Error('browser entry cancelled');
   tls=createBrowserTlsSignerBroker(approved.profile,{timeoutMs:10000,setupDeadline:deadline,signal:controller.signal});
   grant=createBrowserGrantBroker(approved.profile,{configuration:approved.grant,present:approved.present,signal:controller.signal});
   const io=new MessageChannel(),application=new MessageChannel();transport=serveBrowserTransport(io.port1,wire,dormant?{activation:dormant.activation,setupEnd,maxEnd,clock:()=>Math.floor(performance.timeOrigin+performance.now())}:undefined);
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
