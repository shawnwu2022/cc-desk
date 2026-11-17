import {createBrowserRegistrationBridge} from './browserRegistrationBridge.ts';
import {createBrowserRegistrationRelay,RENDEZVOUS_SUBPROTOCOL} from './browserRelay.ts';
import {serveBrowserTransport} from './browserTransport.ts';
import {createBrowserRegistrationInstallation,type BrowserRegistrationInstallationPlan} from './browserRegistrationInstallation.ts';
import type {BrowserTlsEvidence} from './browserTlsWorker.ts';

export type ApprovedBrowserRegistrationInputs=Readonly<{
 keys:Readonly<{tlsKey:CryptoKey;grantKey:CryptoKey;relayKey:CryptoKey}>;
 approvedPlan:BrowserRegistrationInstallationPlan;
 artifacts:Readonly<{wasm:string;glue:string}>;
}>;
export type BrowserRegistrationLifetime=Readonly<{signal:AbortSignal;setupDeadline:number;registrationDeadline:number}>;
const usedInputs=new WeakSet<object>(),usedKeys=new WeakSet<CryptoKey>();
const now=()=>performance.timeOrigin+performance.now();
const unavailable=()=>Error('browser registration entry unavailable');
function exact(value:unknown,keys:string[]):asserts value is Record<string,any>{
 if(!value||typeof value!=='object'||Reflect.ownKeys(value).length!==keys.length)throw unavailable();
 for(const key of keys){const d=Object.getOwnPropertyDescriptor(value,key);if(!d?.enumerable||!('value'in d))throw unavailable();}
}
function snapshot<T>(value:T,depth=0):T{
 if(value===null||typeof value==='string'||typeof value==='boolean'||typeof value==='number'&&Number.isFinite(value))return value;
 if(!value||typeof value!=='object'||depth>8||![Object.prototype,null].includes(Object.getPrototypeOf(value)))throw unavailable();
 const output:Record<string,unknown>={};
 for(const key of Reflect.ownKeys(value)){if(typeof key!=='string')throw unavailable();const d=Object.getOwnPropertyDescriptor(value,key);if(!d?.enumerable||!('value'in d))throw unavailable();output[key]=snapshot(d.value,depth+1);}return Object.freeze(output) as T;
}
// 批准时捕获原始句柄与公开配置，后续调用不得重读可变 provider 对象。
export function snapshotBrowserRegistrationInputs(inputs:ApprovedBrowserRegistrationInputs):ApprovedBrowserRegistrationInputs{
 exact(inputs,['keys','approvedPlan','artifacts']);exact(inputs.keys,['tlsKey','grantKey','relayKey']);
 return Object.freeze({keys:Object.freeze({...inputs.keys}),approvedPlan:snapshot(inputs.approvedPlan),artifacts:snapshot(inputs.artifacts)});
}
const decode=(v:string)=>Uint8Array.from(atob(v.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
function deferred<T>(){let resolve!:(v:T)=>void,reject!:(e:Error)=>void;const promise=new Promise<T>((r,j)=>{resolve=r;reject=j;});void promise.catch(()=>{});return{promise,resolve,reject};}

/** 仅显式调用启动登记；独立来源必须事先批准 Host pin、原 handles 与完整安装计划。 */
export function createBrowserRegistrationEntry(inputs:ApprovedBrowserRegistrationInputs,lifetime:BrowserRegistrationLifetime){
 exact(inputs,['keys','approvedPlan','artifacts']);exact(inputs.keys,['tlsKey','grantKey','relayKey']);exact(lifetime,['signal','setupDeadline','registrationDeadline']);
 if(usedInputs.has(inputs)||Object.values(inputs.keys).some(key=>usedKeys.has(key)))throw unavailable();
 const approvedPlan=snapshot(inputs.approvedPlan),artifacts=snapshot(inputs.artifacts);
 let keys:ApprovedBrowserRegistrationInputs['keys']|undefined=Object.freeze({...inputs.keys});
 const {signal,setupDeadline,registrationDeadline}=lifetime,origin=approvedPlan.configuration.profile.origin,page=document;
 exact(artifacts,['wasm','glue']);
 for(const address of [artifacts.wasm,artifacts.glue]){const url=new URL(address);if(url.href!==address||url.origin!==origin||url.protocol!=='https:'||url.username||url.password||url.search||url.hash||address.length>2048)throw unavailable();}
 const trusted=()=>typeof Window!=='undefined'&&globalThis instanceof Window&&typeof Document!=='undefined'&&document instanceof Document&&document===page&&page.visibilityState==='visible'&&globalThis.top===globalThis.self&&isSecureContext&&crossOriginIsolated&&location.origin===origin&&typeof Worker==='function';
 if(!trusted()||!(signal instanceof AbortSignal)||signal.aborted||!Number.isFinite(setupDeadline)||setupDeadline<=now()||setupDeadline-now()>10000||!Number.isFinite(registrationDeadline)||registrationDeadline<setupDeadline||registrationDeadline-now()>120000)throw unavailable();
 const controller=new AbortController(),outcome=deferred<unknown>(),completed=deferred<void>(),evidence=deferred<BrowserTlsEvidence>(),retired=deferred<void>(),joint=deferred<void>(),socketRetired=deferred<void>();
 const firstPerf=performance.now(),firstWall=Date.now(),firstEpoch=performance.timeOrigin+firstPerf;
 let lastPerf=firstPerf,lastWall=firstWall,clockFailed=false;
 const clock=()=>{const p=performance.now(),w=Date.now();if(!Number.isFinite(p)||!Number.isFinite(w)||p<lastPerf||w<lastWall||Math.max(p-lastPerf,w-lastWall)>1000)clockFailed=true;lastPerf=p;lastWall=w;return clockFailed?Infinity:firstEpoch+Math.max(p-firstPerf,w-firstWall);};
 let bridge:ReturnType<typeof createBrowserRegistrationBridge>|undefined,relay:ReturnType<typeof createBrowserRegistrationRelay>|undefined,transport:ReturnType<typeof serveBrowserTransport>|undefined,worker:Worker|undefined,socket:WebSocket|undefined;
 let closing:Promise<void>|undefined,boot:Promise<void>,timer:ReturnType<typeof setTimeout>|undefined,setupTimer:ReturnType<typeof setTimeout>|undefined,watchdog:ReturnType<typeof setInterval>|undefined;
 let started=false,gotOutcome=false,gotCompleted=false,gotEvidence=false,gotRetired=false;
 const abort=()=>close(),visibility=()=>{if(!trusted()||clock()>=(gotEvidence?registrationDeadline:setupDeadline))close();};
 const fail=()=>{outcome.reject(unavailable());completed.reject(unavailable());evidence.reject(unavailable());};
 function close(){
  if(closing)return;controller.abort();fail();bridge?.close();relay?.close();transport?.close();
  try{if(socket&&socket.readyState<2)socket.close();}catch{socketRetired.reject(Error('browser registration socket retirement uncertain'));}
  try{worker?.postMessage({kind:'close'});}catch{retired.reject(Error('browser registration Worker retirement uncertain'));}
  closing=Promise.resolve().then(async()=>{
   await boot;
   const facts=await Promise.allSettled([bridge?.waitClosed(),relay?.waitClosed(),transport?.waitClosed(),socket?socketRetired.promise:undefined,started?retired.promise:undefined]);
   // terminate 只是已观察退休后的清理，不补造原操作的退休事实。
   worker?.terminate();if(!started){bridge?.worker.port.close();bridge?.worker.signer.port.close();}
   if(timer!==undefined)clearTimeout(timer);if(setupTimer!==undefined)clearTimeout(setupTimer);if(watchdog!==undefined)clearInterval(watchdog);
   signal.removeEventListener('abort',abort);page.removeEventListener('visibilitychange',visibility);page.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);
   keys=undefined;if(facts.some(f=>f.status==='rejected'))throw Error('browser registration entry retirement uncertain');
  });void closing.then(()=>joint.resolve(),joint.reject);
 }
 const session=Object.freeze({outcome:outcome.promise,completed:completed.promise,close,waitClosed:()=>joint.promise});
 // 原安装器先验证计划和 handle 策略；下面的独立挑战在 socket 前证明三公私钥匹配。
 const installation=createBrowserRegistrationInstallation({session,keys,approvedPlan,signal,deadline:registrationDeadline});
 usedInputs.add(inputs);for(const key of Object.values(keys))usedKeys.add(key);
 signal.addEventListener('abort',abort,{once:true});page.addEventListener('visibilitychange',visibility);page.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);
 timer=setTimeout(close,Math.max(0,registrationDeadline-now()));setupTimer=setTimeout(close,Math.max(0,setupDeadline-now()));watchdog=setInterval(visibility,100);
 boot=Promise.resolve().then(async()=>{
  try{
   const live=()=>{if(controller.signal.aborted||!trusted()||clock()>=setupDeadline)throw unavailable();};live();
   const nonce=crypto.getRandomValues(new Uint8Array(32));
   const publicKeys=[approvedPlan.configuration.localSpki,approvedPlan.configuration.grant.issuerSpki,approvedPlan.relaySpki];
   const handles=[keys!.tlsKey,keys!.grantKey,keys!.relayKey];
   for(let index=0;index<3;index++){
    live();const key=await crypto.subtle.importKey('spki',decode(publicKeys[index]!),{name:'ECDSA',namedCurve:'P-256'},false,['verify']);live();
    const message=new Uint8Array([...new TextEncoder().encode(`CC-DESK-BROWSER-REGISTRATION-${['tls','grant','relay'][index]}-PREFLIGHT-v1\0`),...nonce]);
    const signature=await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},handles[index]!,message);live();
    const matched=await crypto.subtle.verify({name:'ECDSA',hash:'SHA-256'},key,signature,message);live();if(!matched)throw unavailable();
   }
   bridge=createBrowserRegistrationBridge({origin,keys:keys!,signal:controller.signal,setupDeadline,registrationDeadline});
   const r=approvedPlan.configuration.relay;
   // 工厂校验也可能在 socket 建立后失败；原 socket 必须先被独立持有并加入退休。
   socket=r.mode==='rendezvous-v1'?new WebSocket(r.endpoint,RENDEZVOUS_SUBPROTOCOL):new WebSocket(r.endpoint);
   socket.addEventListener('close',()=>socketRetired.resolve(),{once:true});if(socket.readyState===3)socketRetired.resolve();
   relay=createBrowserRegistrationRelay({origin:r.endpoint,tunnel:decode(r.tunnel),identity:{keyId:decode(r.keyId),sign:async(request,requestSignal)=>{
    if(controller.signal.aborted||requestSignal.aborted)throw unavailable();
    const signature=await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},keys!.relayKey,new Uint8Array(request.transcript));
    if(controller.signal.aborted||requestSignal.aborted)throw unavailable();return new Uint8Array(signature);
   }},mode:r.mode,setupDeadline,registrationDeadline,ioBudgetMs:30000,signal:controller.signal,socket});
   const wire=await relay.ready;
   if(controller.signal.aborted||!trusted()||now()>=setupDeadline)throw unavailable();
   const io=new MessageChannel();transport=serveBrowserTransport(io.port1,wire);
   worker=new Worker(new URL('./browserRegistrationEntry.worker.ts',import.meta.url),{type:'module',name:'cc-desk-registration'});
   worker.addEventListener('message',event=>{
    const m=event.data;
    if(m?.kind==='retired'&&!gotRetired){gotRetired=true;if(m.uncertain)retired.reject(Error('browser registration Worker retirement uncertain'));else retired.resolve();close();return;}
    if(m?.kind==='failed'){fail();return;}
    if(!controller.signal.aborted&&m?.kind==='evidence'&&!gotEvidence){gotEvidence=true;if(setupTimer!==undefined)clearTimeout(setupTimer);evidence.resolve(m.value);return;}
    if(!controller.signal.aborted&&m?.kind==='outcome'&&!gotOutcome){gotOutcome=true;outcome.resolve(m.value);return;}
    if(!controller.signal.aborted&&m?.kind==='completed'&&!gotCompleted){gotCompleted=true;completed.resolve();return;}
    retired.reject(Error('browser registration Worker control'));close();
   });
   const broken=()=>{retired.reject(Error('browser registration Worker retirement uncertain'));close();};worker.addEventListener('error',broken);worker.addEventListener('messageerror',broken);
   worker.postMessage({kind:'start',origin,artifacts,localSpki:decode(approvedPlan.configuration.localSpki),hostSpki:decode(approvedPlan.configuration.hostSpki),bridge:bridge.worker,transport:io.port2},[bridge.worker.port,bridge.worker.signer.port,io.port2]);started=true;
  }catch{close();}
 });
 return Object.freeze({outcome:outcome.promise,completed:completed.promise,evidence:evidence.promise,install:installation.install,close:()=>{installation.close();close();},waitClosed:async()=>{const facts=await Promise.allSettled([joint.promise,installation.waitClosed()]);if(facts.some(f=>f.status==='rejected'))throw Error('browser registration entry retirement uncertain');}});
}
