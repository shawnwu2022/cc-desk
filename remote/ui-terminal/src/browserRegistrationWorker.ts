import {createBrowserTlsWasmImports} from './browserTlsWasm.ts';
import type {BrowserRegistrationBridge} from './browserRegistrationBridge.ts';
import type {BrowserTlsEvidence,BrowserTlsTransport} from './browserTlsWorker.ts';

type Outcome=Readonly<{kind:'Rejected'}|{kind:'RunLocalCandidateInstalled';receipt:string;scope:Readonly<{session:string;run:string;generation:string}>}>;
type Api={memory:WebAssembly.Memory;browser_registration_start(setup:number,total:number):number;browser_registration_poll():void;browser_registration_cancel():void;browser_registration_event(kind:number):void;__wbindgen_start():void};
const CAP=16384;
/** 单次、专用登记 owner；不导出 AppBind/grant/data 或可安装的浏览器 profile。 */
export async function startBrowserRegistrationWorker(options:Readonly<{wasm:BufferSource|WebAssembly.Module;glue:WebAssembly.ModuleImports;bridge:BrowserRegistrationBridge;localSpki:Uint8Array;peerSpki:Uint8Array;transport:BrowserTlsTransport;signal:AbortSignal}>){
 const bridge=options.bridge,transport=options.transport,signal=options.signal,scope=globalThis as typeof globalThis&{DedicatedWorkerGlobalScope?:new()=>object};
 if(typeof scope.DedicatedWorkerGlobalScope!=='function'||!(globalThis instanceof scope.DedicatedWorkerGlobalScope)||!globalThis.crossOriginIsolated||!globalThis.isSecureContext||location.origin!==bridge.origin||!(signal instanceof AbortSignal)||!(bridge.port instanceof MessagePort)||!(bridge.cancel instanceof SharedArrayBuffer)||bridge.cancel.byteLength!==8)throw Error('browser registration trusted Worker required');
 const control=new Int32Array(bridge.cancel),setup=bridge.signer.setupDeadline,total=bridge.deadline;
 const epoch=performance.timeOrigin+performance.now();
 if(typeof setup!=='number'||!Number.isFinite(setup)||setup<=epoch||setup-epoch>10000||!Number.isFinite(total)||total<setup||total-epoch>120000||bridge.signer.origin!==bridge.origin||Atomics.compareExchange(control,1,0,1)!==0)throw Error('browser registration original deadline or owner');
 const local=Uint8Array.from(options.localSpki),peer=Uint8Array.from(options.peerSpki);
 let api:Api|undefined,memory:WebAssembly.Memory|undefined,signer:ReturnType<typeof createBrowserTlsWasmImports>|undefined;
 let stopping=false,rustDone=false,rustRetirementUnknown=false,transportDone=false,mainDone=false,retirementSent=false,retired=false,loading=true,queued=false,ioClosed=false,ioError=false,eof=false,failure:Error|undefined;
 let reading:Promise<Uint8Array|null>|undefined,writing:Promise<void>|undefined,rx:Uint8Array|undefined,offset=0,response:Uint8Array|undefined,requested=0,answered=0;
 let lastPerf=performance.now(),lastWall=Date.now(),clockFailed=false;
 const startPerf=lastPerf,startWall=lastWall,startEpoch=performance.timeOrigin+lastPerf;
 function now(){const p=performance.now(),w=Date.now();if(!Number.isFinite(p)||!Number.isFinite(w)||p<lastPerf||w<lastWall||Math.max(p-lastPerf,w-lastWall)>1000)clockFailed=true;lastPerf=p;lastWall=w;return clockFailed?Infinity:startEpoch+Math.max(p-startPerf,w-startWall);}
 const external=()=>signal.aborted||clockFailed||Atomics.load(control,0)!==0||Atomics.load(new Int32Array(bridge.signer.shared,0,1),0)!==0;
 let resolveEvidence!:(v:BrowserTlsEvidence)=>void,rejectEvidence!:(e:Error)=>void,resolveOutcome!:(v:Outcome)=>void,rejectOutcome!:(e:Error)=>void,resolveCompleted!:()=>void,rejectCompleted!:(e:Error)=>void,resolveClosed!:()=>void,rejectClosed!:(e:Error)=>void;
 const evidence=new Promise<BrowserTlsEvidence>((a,b)=>{resolveEvidence=a;rejectEvidence=b}),outcome=new Promise<Outcome>((a,b)=>{resolveOutcome=a;rejectOutcome=b}),completed=new Promise<void>((a,b)=>{resolveCompleted=a;rejectCompleted=b}),closed=new Promise<void>((a,b)=>{resolveClosed=a;rejectClosed=b});
 for(const promise of [evidence,outcome,completed,closed])void promise.catch(()=>{});
 function fail(e:Error){rejectEvidence(e);rejectOutcome(e);rejectCompleted(e);}
 function retire(){
  if(loading||!rustDone||!transportDone||reading||writing)return;
  if(!retirementSent){retirementSent=true;try{bridge.port.postMessage(failure||rustRetirementUnknown?'retirement-failed':'retired');}catch{failure=Error('browser registration bridge retirement uncertain');}}
  if(!mainDone||retired)return;retired=true;clearInterval(watchdog);disarm();signal.removeEventListener('abort',close);bridge.port.close();bridge.signer.port.close();rx=undefined;response=undefined;
  if(failure)rejectClosed(failure);else resolveClosed();
 }
 // trap 不证明 Rust Future 已 drop；只能报告失败，调用方仍需销毁隔离 Worker/module。
 function schedule(){if(queued||rustDone||!api)return;queued=true;queueMicrotask(()=>{queued=false;if(rustDone||!api)return;try{if(external())close();api.browser_registration_poll();}catch{failure=Error('browser registration WASM retirement uncertain');rustRetirementUnknown=true;rustDone=true;fail(failure);close();retire();}});}
 function uncertain(){failure=Error('browser registration WASM retirement uncertain');rustRetirementUnknown=true;rustDone=true;fail(failure);}
 function event(kind:number){if(api&&!rustDone){try{api.browser_registration_event(kind);schedule();}catch{uncertain();close();retire();}}}
 function closeIo(){if(ioClosed)return;ioClosed=true;rx=undefined;try{transport.close();}catch{failure=Error('browser registration transport close uncertain');}
  try{void transport.waitClosed().then(()=>{transportDone=true;event(1);event(2);retire();},()=>{failure=Error('browser registration transport retirement uncertain');transportDone=true;event(1);event(2);retire();});}catch{failure=Error('browser registration transport retirement uncertain');transportDone=true;retire();}
 }
 function close(){if(stopping)return;stopping=true;Atomics.store(control,0,1);signer?.close();try{bridge.port.postMessage('close');}catch{failure=Error('browser registration bridge unavailable');}try{if(api&&!rustDone)api.browser_registration_cancel();}catch{uncertain();}finally{closeIo();schedule();retire();}}
 let timer:ReturnType<typeof setTimeout>|undefined;
 function disarm(){if(timer!==undefined)clearTimeout(timer);timer=undefined;}
 function arm(value:number){disarm();if(rustDone)return;if(!Number.isFinite(value)||value>total){close();return;}timer=setTimeout(()=>{timer=undefined;event(3);},Math.max(0,value-now()));}
 function pumpRead(){if(ioClosed||reading||rx||eof||ioError)return;try{reading=transport.read(CAP);}catch{ioError=true;event(1);return;}const operation=reading;
  void operation.then(bytes=>{if(ioClosed)return;if(bytes===null)eof=true;else if(!(bytes instanceof Uint8Array)||bytes.length<1||bytes.length>CAP)ioError=true;else{rx=bytes.slice();offset=0;}},()=>{if(!ioClosed)ioError=true;}).finally(()=>{reading=undefined;event(1);event(2);retire();});
 }
 function range(pointer:number,length:number){if(!memory||memory.buffer instanceof SharedArrayBuffer||!Number.isInteger(pointer)||pointer<0||!Number.isInteger(length)||length<0||pointer>memory.buffer.byteLength-length)throw Error('browser registration memory bounds');return new Uint8Array(memory.buffer,pointer,length);}
 bridge.port.onmessage=(e:MessageEvent<unknown>)=>{
  if(e.data==='retired'||e.data==='retirement-failed'){mainDone=true;if(e.data==='retirement-failed')failure=Error('browser registration producer retirement uncertain');retire();return;}
  if(e.data==='close'){close();return;}
  try{const d=e.data as {kind?:unknown;seq?:unknown;bytes?:unknown};if(stopping||external()||now()>=total||response||!d||typeof d!=='object'||Reflect.ownKeys(d).length!==3||d.kind!=='response'||d.seq!==requested||requested!==answered+1||!(d.bytes instanceof Uint8Array)||d.bytes.length<1||d.bytes.length>CAP)throw Error('browser registration response order');response=d.bytes.slice();event(6);}catch{close();}
 };
 bridge.port.onmessageerror=()=>{failure=Error('browser registration bridge unavailable');close();};bridge.port.start();signal.addEventListener('abort',close,{once:true});
 const watchdog=setInterval(()=>{if(!Number.isFinite(now())||now()>=total||external())close();event(3);},100);
 const imports={cc_desk_browser_owner_v1:{now,deadline:()=>stopping?NaN:setup,peer_expiry:()=>total,schedule,arm_timer:arm,disarm_timer:disarm,
  public_key:(which:number,p:number,n:number)=>{const bytes=which===0?local:which===1?peer:undefined;if(n!==91||bytes?.length!==91)return -1;range(p,n).set(bytes);return n;},
  read:(p:number,n:number)=>{try{if(ioClosed||ioError||external()||n<1||n>CAP)return -2;if(rx){const count=Math.min(n,rx.length-offset);range(p,count).set(rx.subarray(offset,offset+count));offset+=count;if(offset===rx.length){rx=undefined;queueMicrotask(pumpRead);}return count;}if(eof)return 0;pumpRead();return -1;}catch{return -2;}},
  write:(p:number,n:number)=>{try{if(ioClosed||ioError||external()||n<1||n>CAP)return -2;if(writing)return -1;writing=transport.write(range(p,n).slice());void writing.catch(()=>{ioError=true;}).finally(()=>{writing=undefined;event(2);retire();});return n;}catch{ioError=true;return -2;}},
  flush:()=>ioClosed||ioError?-2:writing?-1:1,
  // TLS close_notify 只关闭写侧，保留原读侧等待对端 close_notify；原 Future drop 后才关闭 transport。
  shutdown:()=>ioClosed||ioError?-2:writing?-1:1,close_io:closeIo,
  report:(code:number,protocol:number,cipher:number,group:number,flags:number,p:number,n:number)=>{
   if(code===1){if(stopping||external()||ioClosed||ioError||now()>=setup||n!==32){close();return;}resolveEvidence(Object.freeze({protocol,cipher,group,fullHandshake:(flags&1)!==0,rawPublicKey:(flags&2)!==0,alpnMatches:(flags&4)!==0,peerEndpointId:range(p,n).slice()}));return;}
   rustDone=true;if(code===-5)resolveCompleted();else fail(Error(code===-2?'browser registration deadline':code===-3?'browser registration cancelled':'browser registration rejected'));close();retire();
  },
 },cc_desk_browser_registration_v1:{deadline:()=>total,
  request:(kind:number,p:number,n:number)=>{try{if(stopping||external()||now()>=total||kind!==requested+1||kind>2||requested!==answered||n<1||n>4096)return -2;const bytes=range(p,n).slice();requested=kind;bridge.port.postMessage({kind:'request',seq:kind,bytes},[bytes.buffer]);return 1;}catch{close();return -2;}},
  response:(kind:number,p:number,n:number)=>{try{if(stopping||external()||now()>=total||kind!==requested||kind!==answered+1||n!==CAP)return -2;if(!response)return -1;range(p,response.length).set(response);const length=response.length;response=undefined;answered=kind;return length;}catch{close();return -2;}},
  outcome:(p:number,n:number)=>{try{if(stopping||external()||now()>=total||n<1||n>4096||answered!==2)throw Error('registration outcome fence');const value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(range(p,n)));if(value.kind!=='Rejected'&&value.kind!=='RunLocalCandidateInstalled')throw Error('registration outcome');resolveOutcome(Object.freeze(value));}catch{close();}},
 }};
 try{
  if(external()||now()>=setup)throw Error('registration startup expired');
  signer=createBrowserTlsWasmImports(bridge.signer,local,()=>memory);
  const module=options.wasm instanceof WebAssembly.Module?options.wasm:await WebAssembly.compile(options.wasm);
  if(stopping||external()||now()>=setup)throw Error('registration startup expired');
  const instance=await WebAssembly.instantiate(module,{...signer.imports,...imports,'./fixture_bg.js':options.glue});
  if(stopping||external()||now()>=setup)throw Error('registration startup expired');
  api=instance.exports as unknown as Api;memory=api.memory;(options.glue.__wbg_set_wasm as (e:WebAssembly.Exports)=>void)(instance.exports);api.__wbindgen_start();
  if(api.browser_registration_start(Math.max(1,Math.ceil(setup-now())),Math.max(1,Math.ceil(total-now())))!==1)throw Error('registration startup rejected');
  pumpRead();schedule();
 }catch{if(api&&!rustDone){rustRetirementUnknown=true;failure=Error('browser registration WASM retirement uncertain');}rustDone=true;fail(Error('browser registration startup failed'));close();}finally{loading=false;retire();}
 return Object.freeze({evidence,outcome,completed,close,waitClosed:()=>closed});
}
