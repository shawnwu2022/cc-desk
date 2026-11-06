import {createBrowserTlsWasmImports} from './browserTlsWasm.ts';
import type {BrowserTlsWorkerBridge} from './browserTlsSigner.ts';
import {createBrowserGrantWorker,type BrowserGrantBridge} from './browserGrant.ts';
const CAP=16384;
export type BrowserTlsTransport=Readonly<{
 read(maximum:number):Promise<Uint8Array|null>;write(bytes:Uint8Array):Promise<void>;
 close():void;waitClosed():Promise<void>;
}>;
export type BrowserTlsEvidence=Readonly<{protocol:number;cipher:number;group:number;fullHandshake:boolean;rawPublicKey:boolean;alpnMatches:boolean;peerEndpointId:Uint8Array}>;
export type BrowserClientTrust=Readonly<{hostKeyEpoch:string;clientKeyEpoch:string;origin:string}>;
export type BrowserManagementFacts=Readonly<{kind:'ManagementOnly';hostEndpointId:string;clientEndpointId:string;hostKeyEpoch:string;clientKeyEpoch:string;origin:string;profile:number;minor:number}>;
export type BrowserGrantReceipt=Readonly<{kind:'GrantAccepted';grantId:string;principalId:string;scope:Readonly<{session:string;run:string;generation:string}>;role:'Observer'|'Controller';expiresHostMs:string}>;
export type BrowserTlsWorker=Readonly<{evidence:Promise<BrowserTlsEvidence>;management:Promise<BrowserManagementFacts>|undefined;grantAccepted:Promise<BrowserGrantReceipt>|undefined;close():void;shutdown():void;waitClosed():Promise<void>}>;
type Api={memory:WebAssembly.Memory;browser_tls_start(l:number,p:number,b:number):number;browser_tls_poll():void;browser_tls_cancel():void;browser_tls_event(k:number):void;__wbindgen_start():void};
/** 只在可信DedicatedWorker启动一次；IO能力/官方glue/实例都不发布到Window。 */
export async function startBrowserTlsWorker(options:Readonly<{wasm:BufferSource|WebAssembly.Module;glue:WebAssembly.ModuleImports;bridge:BrowserTlsWorkerBridge;localSpki:Uint8Array;peerSpki:Uint8Array;transport:BrowserTlsTransport;localRole?:number;peerRole?:number;signal?:AbortSignal;clientTrust?:BrowserClientTrust;grantBridge?:BrowserGrantBridge}>):Promise<BrowserTlsWorker>{
 const bridge=Object.freeze({...options.bridge}),transport=options.transport,signal=options.signal,wasm=options.wasm,glue=options.glue,suppliedDeadline=bridge.setupDeadline;
 const scope=globalThis as typeof globalThis&{DedicatedWorkerGlobalScope?:new()=>object};
 if(typeof scope.DedicatedWorkerGlobalScope!=='function'||!(globalThis instanceof scope.DedicatedWorkerGlobalScope)||!globalThis.crossOriginIsolated||!globalThis.isSecureContext||globalThis.location?.origin!==bridge.origin)throw Error('browser TLS trusted Worker required');
 if(typeof suppliedDeadline!=='number'||!Number.isFinite(suppliedDeadline))throw Error('browser setup deadline');
 const deadline=suppliedDeadline;
 const now=()=>performance.timeOrigin+performance.now();
 const localSpki=Uint8Array.from(options.localSpki),peerSpki=Uint8Array.from(options.peerSpki),localRole=options.localRole??0,peerRole=options.peerRole??1;
 const trust=options.clientTrust===undefined?undefined:Object.freeze({...options.clientTrust});
 const grantBridge=options.grantBridge===undefined?undefined:Object.freeze({...options.grantBridge,configuration:options.grantBridge?.configuration?.slice()});
 let grantAdapter:ReturnType<typeof createBrowserGrantWorker>|undefined,managementDeadline:number|undefined;
 let trustBytes:Uint8Array|undefined,managementBound=false;
 let api:Api|undefined,memory:WebAssembly.Memory|undefined,queued=false,rustDone=false,stopping=false,ioClosed=false,transportDone=false,retired=false,runtimeFailure:Error|undefined;
 let timer:ReturnType<typeof setTimeout>|undefined,timerEpoch=0,timerDeadline:number|undefined;
 let reading:Promise<Uint8Array|null>|undefined,writing:Promise<void>|undefined,rx:Uint8Array|undefined,rxOffset=0,eof=false,ioError=false;
 let resolveEvidence!:(value:BrowserTlsEvidence)=>void,rejectEvidence!:(error:Error)=>void,resolveClosed!:()=>void,rejectClosed!:(error:Error)=>void;
 const evidence=new Promise<BrowserTlsEvidence>((resolve,reject)=>{resolveEvidence=resolve;rejectEvidence=reject;});
 let resolveManagement:((value:BrowserManagementFacts)=>void)|undefined,rejectManagement:((error:Error)=>void)|undefined;
 const management=trust===undefined||grantBridge!==undefined?undefined:new Promise<BrowserManagementFacts>((resolve,reject)=>{resolveManagement=resolve;rejectManagement=reject;});void management?.catch(()=>{});
 let resolveGrant:((value:BrowserGrantReceipt)=>void)|undefined,rejectGrant:((error:Error)=>void)|undefined;
 const grantAccepted=grantBridge===undefined?undefined:new Promise<BrowserGrantReceipt>((resolve,reject)=>{resolveGrant=resolve;rejectGrant=reject;});void grantAccepted?.catch(()=>{});
 function fail(error:Error){rejectEvidence(error);rejectManagement?.(error);rejectGrant?.(error);}
 const closed=new Promise<void>((resolve,reject)=>{resolveClosed=resolve;rejectClosed=reject;});void evidence.catch(()=>{});void closed.catch(()=>{});
 let signer:ReturnType<typeof createBrowserTlsWasmImports>|undefined;
 const externallyStopped=()=>signal?.aborted===true||Atomics.load(new Int32Array(bridge.shared,0,1),0)!==0;
 function sealSigning(){if(signer)signer.close();else{const control=new Int32Array(bridge.shared,0,5);Atomics.store(control,0,1);Atomics.store(control,1,3);Atomics.notify(control,1);try{bridge.port.postMessage('close');}catch{/* 原主线程仍可看到共享关闭标记。 */}}}
 function sealGrant(){if(grantAdapter)grantAdapter.close();else if(grantBridge){try{grantBridge.port.postMessage('close');grantBridge.port.close();}catch{/* 无有效grant能力。 */}}}
 function retire(){if(rustDone&&transportDone&&!reading&&!writing&&!retired){retired=true;rx=undefined;disarm();sealGrant();signal?.removeEventListener('abort',close);bridge.port.removeEventListener('message',brokerClosed);bridge.port.removeEventListener('messageerror',close);if(runtimeFailure)rejectClosed(runtimeFailure);else resolveClosed();}}
 function schedule(){if(queued||rustDone||!api)return;queued=true;queueMicrotask(()=>{queued=false;if(rustDone||!api)return;try{if(externallyStopped())close();api.browser_tls_poll();}catch{rustDone=true;runtimeFailure=Error('browser TLS runtime failed');fail(runtimeFailure);closeIo();sealSigning();sealGrant();retire();}});}
 function event(kind:number){if(!rustDone&&api){api.browser_tls_event(kind);schedule();}}
 function closeIo(){
  if(ioClosed)return;ioClosed=true;rx=undefined;
  try{transport.close();}catch{ioError=true;runtimeFailure=Error('browser transport close failed');}
  let operation:Promise<void>;try{operation=transport.waitClosed();}catch{runtimeFailure=Error('browser transport retirement failed');transportDone=true;event(2);retire();return;}
  void operation.then(()=>{transportDone=true;event(2);retire();},()=>{runtimeFailure=Error('browser transport retirement failed');transportDone=true;event(2);fail(runtimeFailure);retire();});
 }
 function close(){if(stopping)return;stopping=true;sealSigning();sealGrant();if(api&&!rustDone)api.browser_tls_cancel();closeIo();schedule();}
 function shutdown(){if(trust&&!managementBound){close();return;}if(api&&!rustDone){api.browser_tls_event(5);schedule();}}
 function brokerClosed(){close();}
 function stopLoading(){rustDone=true;fail(Error(externallyStopped()||stopping?'browser TLS cancelled':'browser TLS deadline'));sealSigning();sealGrant();closeIo();retire();}
 function disarm(){timerEpoch++;if(timer!==undefined)clearTimeout(timer);timer=undefined;timerDeadline=undefined;}
 function arm(value:number){
  if(rustDone||!Number.isFinite(value)||value>(managementDeadline??deadline)){close();return;}
  if(timer!==undefined&&timerDeadline===value)return;
  disarm();timerDeadline=value;const epoch=timerEpoch;
  const tick=()=>{if(epoch!==timerEpoch||rustDone)return;const remaining=value-now();if(remaining>0){timer=setTimeout(tick,remaining);return;}timer=undefined;event(3);};
  timer=setTimeout(tick,Math.max(0,value-now()));
 }
 function pumpRead(){
  if(ioClosed||reading||rx||eof||ioError)return;
  try{reading=transport.read(CAP);}catch{ioError=true;event(4);event(1);return;}
  const operation=reading;
  void operation.then(bytes=>{if(ioClosed)return;if(bytes===null)eof=true;else if(!(bytes instanceof Uint8Array)||bytes.length<1||bytes.length>CAP)ioError=true;else{rx=Uint8Array.from(bytes);rxOffset=0;}}).catch(()=>{if(!ioClosed)ioError=true;}).finally(()=>{reading=undefined;if(eof||ioError)event(4);event(1);if(ioClosed)event(2);retire();});
 }
 function range(pointer:number,length:number):Uint8Array{
  if(!memory||memory.buffer instanceof SharedArrayBuffer||!Number.isInteger(pointer)||pointer<0||!Number.isInteger(length)||length<0||pointer>memory.buffer.byteLength-length)throw Error('browser TLS memory bounds');
  return new Uint8Array(memory.buffer,pointer,length);
 }
 function ownerImports(){return {cc_desk_browser_owner_v1:{
  grant_deadline:(budgetMs:number)=>{const current=now();if(!grantAdapter||managementDeadline!==undefined||stopping||ioClosed||externallyStopped()||current>=deadline||!Number.isInteger(budgetMs)||budgetMs<1||budgetMs>30000)return NaN;managementDeadline=current+budgetMs;return managementDeadline;},
  grant_config:(pointer:number,capacity:number)=>{if(!grantAdapter)return 0;if(capacity!==8192)return -1;const bytes=grantAdapter.configuration;range(pointer,bytes.length).set(bytes);return bytes.length;},
  grant_accepted:(pointer:number,length:number)=>{try{if(!grantAdapter||managementBound||stopping||ioClosed||ioError||eof||externallyStopped()||managementDeadline===undefined||now()>=managementDeadline||length<1||length>4096)throw Error('grant publication');const value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(range(pointer,length))) as BrowserGrantReceipt;if(!value||Object.keys(value).length!==6||value.kind!=='GrantAccepted'||!/^[0-9a-f]{64}$/.test(value.grantId)||!/^[0-9a-f]{64}$/.test(value.principalId)||!['Observer','Controller'].includes(value.role)||!value.scope||typeof value.expiresHostMs!=='string')throw Error('grant receipt');managementBound=true;resolveGrant?.(Object.freeze({...value,scope:Object.freeze(value.scope)}));}catch{fail(Error('browser grant rejected'));close();}},
  client_trust:(pointer:number,capacity:number)=>{if(!trustBytes)return 0;if(capacity!==2304||trustBytes.length>capacity)return -1;range(pointer,trustBytes.length).set(trustBytes);return trustBytes.length;},
  management:(pointer:number,length:number)=>{
   try{
    if(!trust||managementBound||stopping||ioClosed||ioError||eof||externallyStopped()||now()>=deadline||length<1||length>4096)throw Error('browser management publication');
    const value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(range(pointer,length))) as BrowserManagementFacts;
    const keys=['kind','profile','minor','hostEndpointId','clientEndpointId','hostKeyEpoch','clientKeyEpoch','origin'];
    if(!value||Object.keys(value).length!==keys.length||!keys.every(key=>Object.hasOwn(value,key))||value.kind!=='ManagementOnly'||value.profile!==1||value.minor!==0||!/^[0-9a-f]{64}$/.test(value.hostEndpointId)||!/^[0-9a-f]{64}$/.test(value.clientEndpointId)||value.hostKeyEpoch!==trust.hostKeyEpoch||value.clientKeyEpoch!==trust.clientKeyEpoch||value.origin!==trust.origin)throw Error('browser management facts');
    managementBound=true;resolveManagement?.(Object.freeze(value));
   }catch{fail(Error('browser AppBind rejected'));close();}
  },
  now,deadline:()=>externallyStopped()||stopping?NaN:deadline,schedule,arm_timer:arm,disarm_timer:disarm,
  public_key:(which:number,pointer:number,capacity:number)=>{try{const key=which===0?localSpki:which===1?peerSpki:undefined;if(capacity!==91||key?.length!==91)return -1;range(pointer,91).set(key);return 91;}catch{return -1;}},
  read:(pointer:number,capacity:number)=>{try{if(ioClosed||ioError||externallyStopped()||capacity<1||capacity>CAP)return -2;if(rx){const n=Math.min(capacity,rx.length-rxOffset);range(pointer,n).set(rx.subarray(rxOffset,rxOffset+n));rxOffset+=n;if(rxOffset===rx.length){rx=undefined;queueMicrotask(pumpRead);}return n;}if(eof)return 0;pumpRead();return -1;}catch{return -2;}},
  write:(pointer:number,length:number)=>{try{if(ioClosed||ioError||externallyStopped()||length<1||length>CAP)return -2;if(writing)return -1;const bytes=range(pointer,length).slice();writing=transport.write(bytes);const operation=writing;void operation.catch(()=>{ioError=true;}).finally(()=>{writing=undefined;if(ioError)event(4);event(2);retire();});return length;}catch{ioError=true;return -2;}},
  flush:()=>ioClosed||ioError?-2:writing?-1:1,
  shutdown:()=>{if(ioError||runtimeFailure)return -2;if(writing)return -1;closeIo();return transportDone&&!reading&&!writing?1:-1;},
  close_io:closeIo,
  report:(code:number,protocol:number,cipher:number,group:number,flags:number,pointer:number,length:number)=>{
   if(code===1){if(stopping||ioClosed||ioError||eof||Atomics.load(new Int32Array(bridge.shared,0,1),0)!==0||now()>=deadline||length!==32){close();return;}resolveEvidence(Object.freeze({protocol,cipher,group,fullHandshake:(flags&1)!==0,rawPublicKey:(flags&2)!==0,alpnMatches:(flags&4)!==0,peerEndpointId:range(pointer,32).slice()}));return;}
   rustDone=true;const failure=Error(code===-8?'browser grant rejected':code===-6?'browser AppBind Unsupported':code===-7?'browser AppBind rejected':code===-2?'browser TLS deadline':code===-3?'browser TLS cancelled':code===-4?'browser TLS role':'browser TLS rejected');fail(failure);sealSigning();grantAdapter?.close();closeIo();retire();
  },
 }};}
 signal?.addEventListener('abort',close,{once:true});
 bridge.port.addEventListener('message',brokerClosed);bridge.port.addEventListener('messageerror',close);bridge.port.start();
 try{
  if(externallyStopped()||now()>=deadline){stopLoading();return Object.freeze({evidence,management,grantAccepted,close,shutdown,waitClosed:()=>closed});}
  if(trust){const epoch=(value:unknown)=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=18446744073709551615n;if(Object.keys(trust).length!==3||!epoch(trust.hostKeyEpoch)||!epoch(trust.clientKeyEpoch)||typeof trust.origin!=='string'||trust.origin.length>2048)throw Error('browser AppBind trust');trustBytes=new TextEncoder().encode(JSON.stringify(trust));}
  if(!Number.isInteger(localRole)||!Number.isInteger(peerRole)||localRole<0||localRole>1||peerRole<0||peerRole>1){rustDone=true;fail(Error('browser TLS role'));sealSigning();closeIo();retire();return Object.freeze({evidence,management,grantAccepted,close,shutdown,waitClosed:()=>closed});}
  signer=createBrowserTlsWasmImports(bridge,localSpki,()=>memory);
  if(grantBridge){if(!trust)throw Error('grant requires ClientTrust');grantAdapter=createBrowserGrantWorker(grantBridge,()=>memory,()=>managementDeadline,()=>stopping||ioClosed||externallyStopped(),()=>event(6),close);}
  // 原始加载Promise由本函数持续持有到settle；取消不使用race丢弃迟到实例。
  const compilation=wasm instanceof WebAssembly.Module?undefined:WebAssembly.compile(wasm);
  const module=wasm instanceof WebAssembly.Module?wasm:await compilation!;
  if(stopping||externallyStopped()||now()>=deadline){stopLoading();return Object.freeze({evidence,management,grantAccepted,close,shutdown,waitClosed:()=>closed});}
  const instantiation=WebAssembly.instantiate(module,{...signer.imports,'./fixture_bg.js':glue,...ownerImports(),...(grantAdapter?.imports??{cc_desk_browser_grant_v1:{begin:()=>-2,read:()=>-2,cose:()=>-2,cancel:()=>{}}})});
  const instance=await instantiation;
  if(stopping||externallyStopped()||now()>=deadline){stopLoading();return Object.freeze({evidence,management,grantAccepted,close,shutdown,waitClosed:()=>closed});}
  api=instance.exports as unknown as Api;memory=api.memory;
  (glue.__wbg_set_wasm as (value:WebAssembly.Exports)=>void)(instance.exports);api.__wbindgen_start();
  if(api.browser_tls_start(localRole,peerRole,Math.max(1,Math.ceil(deadline-now())))!==1){rustDone=true;fail(Error('browser TLS startup rejected'));closeIo();sealSigning();retire();}
  else{pumpRead();schedule();}
 }catch{rustDone=true;closeIo();sealSigning();sealGrant();fail(Error('browser TLS startup failed'));retire();}
 return Object.freeze({evidence,management,grantAccepted,close,shutdown,waitClosed:()=>closed});
}
