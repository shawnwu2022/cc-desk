import {browserProfileBoundary,type BrowserProfile} from './browserApproval.ts';

const MAX_WAIT_MS=10000,MAX_SEQ=0x7fffffff;
const CLOSED=0,STATE=1,SEQ=2,LENGTH=3,ATTACHED=4,WORDS=5,MAX_DER=72;
const IDLE=0,WAITING=1,READY=2,STOPPED=3;
const SIZE=WORDS*4+MAX_DER;
const context=new TextEncoder().encode('TLS 1.3, client CertificateVerify');
const MESSAGE_SIZE=64+context.length+1+32;
const order=Uint8Array.from('ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551'.match(/../g)!,v=>parseInt(v,16));

export type BrowserTlsWorkerBridge=Readonly<{
 port:MessagePort;shared:SharedArrayBuffer;generation:string;origin:string;timeoutMs:number;setupDeadline:number|undefined;
}>;
export type BrowserTlsSignerBroker=Readonly<{
 /** 仅转交给一个可信 DedicatedWorker，不广播端口或密钥。 */
 worker:BrowserTlsWorkerBridge;
 close():void;
 /** 只证明本桥任务完成且持有的 key 引用释放，不证明 CryptoKey 被销毁。 */
 waitClosed():Promise<void>;
}>;
export type BrowserTlsWorkerSigner=Readonly<{sign(message:Uint8Array):Uint8Array;close():void}>;

function now():number{return performance.timeOrigin+performance.now();}
function isolated():void{
 if(!globalThis.crossOriginIsolated||typeof SharedArrayBuffer==='undefined'||typeof Atomics?.wait!=='function')throw Error('TLS signer isolation unavailable');
}
function validOrigin(origin:string):boolean{
 try{const url=new URL(origin);return url.protocol==='https:'&&url.origin===origin&&!url.username&&!url.password;}catch{return false;}
}
function atOrigin(origin:string):boolean{return globalThis.isSecureContext===true&&globalThis.location?.origin===origin;}
function budget(value:number):boolean{return Number.isInteger(value)&&value>=1&&value<=MAX_WAIT_MS;}
function exact(value:unknown,keys:string[]):value is Record<string,unknown>{
 return !!value&&typeof value==='object'&&Object.keys(value).length===keys.length&&keys.every(k=>Object.prototype.hasOwnProperty.call(value,k));
}
function tlsMessage(value:unknown):Uint8Array{
 if(!(value instanceof Uint8Array)||value.length!==MESSAGE_SIZE)throw Error('TLS client CertificateVerify message bounds');
 // 在任何异步调用前独立复制，原始输入即使来自共享内存也不能改变已检查的请求。
 const bytes=Uint8Array.from(value);
 if(!bytes.subarray(0,64).every(v=>v===32)||!context.every((v,i)=>bytes[64+i]===v)||bytes[64+context.length]!==0)throw Error('TLS client CertificateVerify message domain');
 return bytes;
}
function derSignature(raw:ArrayBuffer):Uint8Array{
 if(!(raw instanceof ArrayBuffer)||raw.byteLength!==64)throw Error('TLS signature bounds');
 const bytes=new Uint8Array(raw);
 const integer=(part:Uint8Array)=>{
  let comparison=0;
  for(let i=0;i<32&&!comparison;i++)comparison=part[i]!-order[i]!;
  if(!part.some(v=>v!==0)||comparison>=0)throw Error('TLS signature scalar');
  let first=0;while(first<31&&part[first]===0)first++;
  const magnitude=part.subarray(first),pad=(magnitude[0]!&128)?1:0;
  const result=new Uint8Array(2+pad+magnitude.length);result.set([2,pad+magnitude.length]);result.set(magnitude,2+pad);return result;
 };
 const r=integer(bytes.subarray(0,32)),s=integer(bytes.subarray(32));
 const der=new Uint8Array(2+r.length+s.length);der.set([48,r.length+s.length]);der.set(r,2);der.set(s,2+r.length);return der;
}

/** 主线程 broker；不生成/导出 key，不使用 grantKey，不提供通用签名入口。 */
export function createBrowserTlsSignerBroker(profile:BrowserProfile,options:Readonly<{
 /** 必须取原 TLS setup 剩余预算，最多 10s；本桥不延长外层握手 deadline。 */
 timeoutMs:number;signal?:AbortSignal;setupDeadline?:number;
}>):BrowserTlsSignerBroker{
 const p=browserProfileBoundary(profile),timeoutMs=options.timeoutMs;
 if(typeof Window==='undefined'||!(globalThis instanceof Window)||globalThis.top!==globalThis.self||!atOrigin(p.origin)||!globalThis.crypto?.subtle)throw Error('TLS signer window unavailable');
 isolated();
 if(!budget(timeoutMs)||options.signal?.aborted)throw Error('TLS signer closed or invalid timeout');
 if(options.setupDeadline!==undefined&&(!Number.isFinite(options.setupDeadline)||options.setupDeadline<=now()||options.setupDeadline>now()+timeoutMs))throw Error('TLS setup deadline');
 return broker(p.tlsKey,p.origin,timeoutMs,options.signal,options.setupDeadline);
}

function broker(key:CryptoKey|undefined,origin:string,timeoutMs:number,signal?:AbortSignal,setupDeadline?:number):BrowserTlsSignerBroker{
 const channel=new MessageChannel(),port=channel.port1,shared=new SharedArrayBuffer(SIZE);
 const control=new Int32Array(shared,0,WORDS),output=new Uint8Array(shared,WORDS*4,MAX_DER);
 const generation=Array.from(crypto.getRandomValues(new Uint8Array(16)),v=>v.toString(16).padStart(2,'0')).join('');
 let expected=1,closed=false,retired=false,pending:Promise<ArrayBuffer>|undefined,timer:ReturnType<typeof setTimeout>|undefined;
 let resolveClosed!:()=>void;
 const completion=new Promise<void>(resolve=>{resolveClosed=resolve;});
 function retire(){
  if(!closed||pending||retired)return;
  retired=true;key=undefined;output.fill(0);Atomics.store(control,LENGTH,0);
  port.onmessage=null;port.onmessageerror=null;port.close();signal?.removeEventListener('abort',close);resolveClosed();
 }
 function close(){
  closed=true;Atomics.store(control,CLOSED,1);Atomics.store(control,STATE,STOPPED);Atomics.notify(control,STATE);
  try{port.postMessage('close');}catch{/* Worker仍会检查不可逆共享关闭标记。 */}
  if(timer!==undefined){clearTimeout(timer);timer=undefined;}
  retire();
 }
 function live(deadline:number,seq:number):boolean{
  return !closed&&Atomics.load(control,CLOSED)===0&&Atomics.load(control,ATTACHED)===1&&Atomics.load(control,SEQ)===seq&&Atomics.load(control,STATE)===WAITING&&atOrigin(origin)&&globalThis.crossOriginIsolated&&now()<deadline&&(setupDeadline===undefined||deadline<=setupDeadline);
 }
 port.onmessage=(event:MessageEvent<unknown>)=>{
  if(event.data==='close'){close();return;}
  try{
   if(closed||pending||!exact(event.data,['generation','seq','deadline','message']))throw Error('TLS signer request');
   const data=event.data,seq=data.seq,deadline=data.deadline;
   if(data.generation!==generation||!Number.isInteger(seq)||seq!==expected||expected>MAX_SEQ||typeof deadline!=='number'||!Number.isFinite(deadline)||deadline>now()+timeoutMs||!live(deadline,seq as number))throw Error('TLS signer request fence');
   const message=tlsMessage(data.message);
   if(!live(deadline,seq as number))throw Error('TLS signer expired');
   expected++;
   const expire=()=>{const remaining=deadline-now();if(remaining>0)timer=setTimeout(expire,remaining);else close();};
   timer=setTimeout(expire,Math.max(0,deadline-now()));
   // 原始 SubtleCrypto Promise 一直持有到 settle；close/timeout 不能假装取消运算。
   const operation=crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},key!,Uint8Array.from(message).buffer);
   pending=operation;
   void operation.then(raw=>{
    if(!live(deadline,seq as number)){close();return;}
    const der=derSignature(raw);
    if(!live(deadline,seq as number)){close();return;}
    output.set(der);Atomics.store(control,LENGTH,der.length);
    // Worker 会在复制后再次检查关闭标记和最初的 deadline。
    if(Atomics.compareExchange(control,STATE,WAITING,READY)!==WAITING){close();return;}
    Atomics.notify(control,STATE);
   }).catch(()=>close()).finally(()=>{
    pending=undefined;
    if(timer!==undefined){clearTimeout(timer);timer=undefined;}
    if(Atomics.load(control,CLOSED)!==0)close();
    retire();
   });
  }catch{close();}
 };
 port.onmessageerror=close;port.start();signal?.addEventListener('abort',close,{once:true});
 return Object.freeze({worker:Object.freeze({port:channel.port2,shared,generation,origin,timeoutMs,setupDeadline}),close,waitClosed:()=>completion});
}

/** 未来 rustls Signer 回调的同步端；没有 WASM ABI、TLS 状态机或 Ready 适配器。 */
export function createBrowserTlsWorkerSigner(bridge:BrowserTlsWorkerBridge):BrowserTlsWorkerSigner{
 const scope=globalThis as typeof globalThis&{DedicatedWorkerGlobalScope?:new()=>object};
 if(typeof scope.DedicatedWorkerGlobalScope!=='function'||!(globalThis instanceof scope.DedicatedWorkerGlobalScope)||typeof Window!=='undefined'||typeof document!=='undefined')throw Error('TLS signer dedicated worker required');
 isolated();
 if(!exact(bridge,['port','shared','generation','origin','timeoutMs','setupDeadline'])||!(bridge.port instanceof MessagePort)||!(bridge.shared instanceof SharedArrayBuffer)||bridge.shared.byteLength!==SIZE||typeof bridge.generation!=='string'||!/^[0-9a-f]{32}$/.test(bridge.generation)||typeof bridge.origin!=='string'||!validOrigin(bridge.origin)||!atOrigin(bridge.origin)||!budget(bridge.timeoutMs)||(bridge.setupDeadline!==undefined&&(!Number.isFinite(bridge.setupDeadline)||bridge.setupDeadline<=now()||bridge.setupDeadline>now()+bridge.timeoutMs)))throw Error('TLS signer worker bridge bounds');
 const {port,shared,generation,origin,timeoutMs,setupDeadline}=bridge;
 const control=new Int32Array(shared,0,WORDS),output=new Uint8Array(shared,WORDS*4,MAX_DER);
 if(Atomics.compareExchange(control,ATTACHED,0,1)!==0||Atomics.load(control,CLOSED)!==0||Atomics.load(control,SEQ)!==0||Atomics.load(control,STATE)!==IDLE)throw Error('TLS signer worker bridge reused');
 let sequence=0,active=false;
 function close(){
  Atomics.store(control,CLOSED,1);Atomics.store(control,STATE,STOPPED);Atomics.notify(control,STATE);
  try{port.postMessage('close');}catch{/* 共享关闭标记仍会拒绝原请求。 */}
  port.close();
 }
 function sign(value:Uint8Array):Uint8Array{
  try{
   if(active||Atomics.load(control,CLOSED)!==0)throw Error('TLS signer closed or concurrent');
   if(!atOrigin(origin)||!globalThis.crossOriginIsolated||sequence===MAX_SEQ)throw Error('TLS signer unavailable');
   const deadline=Math.min(now()+timeoutMs,setupDeadline??Infinity),message=tlsMessage(value),seq=++sequence;
   if(now()>=deadline)throw Error('TLS signer setup timeout');
   active=true;Atomics.store(control,LENGTH,0);Atomics.store(control,SEQ,seq);
   // 关闭必须改变实际等待槽；正常转换不能把终止态写回可等待状态。
   if(Atomics.compareExchange(control,STATE,IDLE,WAITING)!==IDLE)throw Error('TLS signer closed or response state');
   port.postMessage({generation,seq,deadline,message});
   for(;;){
    if(Atomics.load(control,CLOSED)!==0)throw Error('TLS signer closed');
    const remaining=deadline-now();if(remaining<=0)throw Error('TLS signer timeout');
    if(Atomics.load(control,STATE)===READY){
     const length=Atomics.load(control,LENGTH);
     if(Atomics.load(control,SEQ)!==seq||length<8||length>MAX_DER)throw Error('TLS signer response bounds');
     const result=output.slice(0,length);
     if(Atomics.load(control,CLOSED)!==0||now()>=deadline)throw Error('TLS signer closed or timeout');
     output.fill(0);
     if(Atomics.compareExchange(control,STATE,READY,IDLE)!==READY)throw Error('TLS signer closed or response state');
     return result;
    }
    if(Atomics.load(control,STATE)!==WAITING)throw Error('TLS signer response state');
    Atomics.wait(control,STATE,WAITING,remaining);
   }
  }catch(error){close();throw error;}finally{active=false;}
 }
 return Object.freeze({sign,close});
}
