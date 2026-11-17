import type {NativeClientPort} from './native.ts';
import {snapshotPullRequest,type BrowserClientDescriptor,type BrowserNativeClientPort} from './browserNative.ts';
import type {Stamp} from './reducer.ts';
const claimed=new WeakSet<MessagePort>();
const MAX=4194304;
/** Window仅持私有Worker端口；descriptor来自原Rust proof，构造此adapter本身不是授权。 */
export function browserClientPort(port:MessagePort,stop:()=>void,waitClosed:()=>Promise<void>,cancel:SharedArrayBuffer):BrowserNativeClientPort&{descriptor:Promise<BrowserClientDescriptor>}{
 if(claimed.has(port))throw Error('browser client port reused');claimed.add(port);
 if(!(cancel instanceof SharedArrayBuffer)||cancel.byteLength!==4||Atomics.load(new Int32Array(cancel),0)!==0)throw Error('browser cancellation required');
 const cancelled=()=>Atomics.load(new Int32Array(cancel),0)!==0;
 const source=document;if(!(source instanceof Document)||source.visibilityState!=='visible')throw Error('visible document required');
 let closed=false,sequence=0,pending:{seq:number;resolve:(value:unknown)=>void;reject:(error:Error)=>void}|undefined,closing:Promise<void>|undefined;
 let retiredResolve!:()=>void,retiredReject!:(error:Error)=>void;
 const retiredAck=new Promise<void>((a,b)=>{retiredResolve=a;retiredReject=b;});void retiredAck.catch(()=>{});
 const listeners=new Set<()=>void>();let resolve!:(value:BrowserClientDescriptor)=>void,reject!:(error:Error)=>void;
 const descriptor=new Promise<BrowserClientDescriptor>((a,b)=>{resolve=a;reject=b;});void descriptor.catch(()=>{});
 function invalidate(){if(closed)return;closed=true;for(const listener of [...listeners]){try{listener();}catch{/* 通知异常不能截断原关闭。 */}}listeners.clear();reject(Error('browser client invalidated'));}
 function close(){
  if(closing)return closing;
  let resolve!:()=>void,reject!:(error:Error)=>void;
  closing=new Promise<void>((a,b)=>{resolve=a;reject=b;});void closing.catch(()=>{});
  invalidate();let failed=false;
  try{stop();}catch{failed=true;}
  try{port.postMessage({kind:'close'});}catch{failed=true;}
  let original:Promise<void>;try{original=waitClosed();}catch{original=Promise.reject(Error('browser retirement unavailable'));}
  void Promise.all([original,retiredAck]).then(()=>{if(failed)reject(Error('browser close uncertain'));else resolve();},()=>reject(Error('browser retirement uncertain')));
  return closing;
 }
 function lifecycle(){if(source.visibilityState!=='visible')void close();}
 source.addEventListener('visibilitychange',lifecycle);source.addEventListener('freeze',close);globalThis.addEventListener('pagehide',close);
 const retired=()=>{source.removeEventListener('visibilitychange',lifecycle);source.removeEventListener('freeze',close);globalThis.removeEventListener('pagehide',close);port.close();if(pending){pending.reject(Error('browser client retired'));pending=undefined;}};
 port.addEventListener('message',event=>{
  const m=event.data;if(cancelled())invalidate();
  if(m?.kind==='invalidated'){invalidate();return;}
  if(m?.kind==='retired'){invalidate();retired();if(m.uncertain===true)retiredReject(Error('browser retirement uncertain'));else retiredResolve();return;}
  if(m?.kind==='descriptor'){if(!closed)resolve(m.value);return;}
  if(m?.kind==='result'&&pending&&m.seq===pending.seq){const original=pending;pending=undefined;if(closed||!m.ok)original.reject(Error('browser request rejected'));else original.resolve(m.value);return;}
  void close();
 });port.addEventListener('messageerror',close);port.start();
 function run(kind:'snapshot'|'snapshotPull'|'exchange',payload:Uint8Array,budgetMs:number){if(closed||cancelled()||pending||!(payload instanceof Uint8Array)||payload.length>32748||!Number.isInteger(budgetMs)||budgetMs<1||budgetMs>1000||sequence===4294967295)return Promise.reject(Error('browser client unavailable'));const seq=++sequence;const result=new Promise<unknown>((a,b)=>{pending={seq,resolve:a,reject:b};});port.postMessage({kind,seq,payload:payload.slice(),budgetMs,deadline:Math.floor(performance.timeOrigin+performance.now())+budgetMs});return result;}
 return{descriptor,onInvalidate(listener){if(closed){listener();return()=>{};}listeners.add(listener);return()=>listeners.delete(listener);},snapshot:async(payload,budget)=>{const value=await run('snapshot',payload,budget) as {stamp:Stamp;parts:Uint8Array[]};if(!value||!Array.isArray(value.parts)||value.parts.length>24000||value.parts.reduce((n,b)=>n+(b instanceof Uint8Array?b.length:MAX+1),0)>MAX)throw Error('browser snapshot bounds');return value;},snapshotPull:async(request,budget)=>{const value=await run('snapshotPull',snapshotPullRequest(request),budget) as {stamp:Stamp;parts:Uint8Array[]};if(!value||!Array.isArray(value.parts)||value.parts.length>24000||value.parts.reduce((n,b)=>n+(b instanceof Uint8Array?b.length:MAX+1),0)>MAX)throw Error('browser snapshot bounds');return value;},exchange:async(payload,budget)=>{const value=await run('exchange',payload,budget);if(!(value instanceof Uint8Array)||value.length>32768)throw Error('browser reply bounds');return value;},close};
}
/** Worker持有原Promise到settle；关闭只封准入，不提前报告原任务退休。 */
export function serveBrowserClient(port:MessagePort,client:NativeClientPort&Partial<Pick<BrowserNativeClientPort,'snapshotPull'>>,descriptor:Promise<BrowserClientDescriptor>,close:()=>void,waitClosed:()=>Promise<void>,limitDeadline:(value:number)=>boolean){
 if(claimed.has(port))throw Error('browser client port reused');claimed.add(port);
 let closed=false,sequence=0,pending:Promise<void>|undefined;
 const invalidate=()=>{if(closed)return;closed=true;port.postMessage({kind:'invalidated'});};
 client.onInvalidate?.(invalidate);
 void descriptor.then(value=>{if(!closed)port.postMessage({kind:'descriptor',value});},()=>{invalidate();close();});
 port.addEventListener('message',event=>{
  const m=event.data;if(m?.kind==='close'){invalidate();close();return;}
  if(closed||pending||!m||Object.keys(m).length!==5||!['snapshot','snapshotPull','exchange'].includes(m.kind)||m.seq!==sequence+1||!Number.isInteger(m.seq)||m.seq>4294967295||!(m.payload instanceof Uint8Array)||m.payload.length>32748||!Number.isInteger(m.budgetMs)||m.budgetMs<1||m.budgetMs>1000){invalidate();close();return;}
  if(m.kind==='snapshotPull'&&(m.payload.length!==8||new DataView(m.payload.buffer,m.payload.byteOffset,8).getBigUint64(0,true)===0n||typeof client.snapshotPull!=='function')){invalidate();close();return;}
  if(!Number.isFinite(m.deadline)||!limitDeadline(m.deadline)){invalidate();close();return;}
  sequence=m.seq;const payload=m.payload.slice();
  const operation=Promise.resolve().then(async()=>{if(closed)throw Error('browser request closed');return m.kind==='snapshot'?client.snapshot(payload,m.budgetMs):m.kind==='snapshotPull'?client.snapshotPull!(new DataView(payload.buffer,payload.byteOffset,8).getBigUint64(0,true),m.budgetMs):client.exchange(payload,m.budgetMs);}).then(value=>{if(!closed)port.postMessage({kind:'result',seq:m.seq,ok:true,value});},()=>{port.postMessage({kind:'result',seq:m.seq,ok:false});invalidate();close();}).finally(()=>{if(pending===operation)pending=undefined;});
  pending=operation;
 });port.addEventListener('messageerror',()=>{invalidate();close();});port.start();
 void waitClosed().then(async()=>{await pending;invalidate();port.postMessage({kind:'retired'});port.close();},async()=>{await pending;invalidate();port.postMessage({kind:'retired',uncertain:true});port.close();});
}
