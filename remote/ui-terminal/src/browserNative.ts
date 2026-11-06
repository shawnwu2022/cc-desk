import type {NativeClientPort} from './native.ts';
import type {Target,Stamp} from './reducer.ts';
export type BrowserClientDescriptor=Readonly<{target:Target;binding:string;role:'Controller'|'Observer';fence:string}>;
/** 仅受控Worker持有此adapter；外部只拿固定NativeClientPort，不拿memory/exports。 */
export function createBrowserNativeAdapter(memory:()=>WebAssembly.Memory|undefined,stopped:()=>boolean,wake:()=>void,stop:()=>void,retired:()=>Promise<void>,clock:()=>number,expiry:()=>number){
 let sequence=0,invalid=false,request:Uint8Array|undefined,nextDeadline:number|undefined,pending:{seq:number;kind:number;parts:Uint8Array[];bytes:number;end:number;resolve:(parts:Uint8Array[])=>void;reject:(error:Error)=>void}|undefined;
 const listeners=new Set<()=>void>();
 let resolveDescriptor!:(value:BrowserClientDescriptor)=>void,rejectDescriptor!:(error:Error)=>void;
 const descriptor=new Promise<BrowserClientDescriptor>((a,b)=>{resolveDescriptor=a;rejectDescriptor=b;});void descriptor.catch(()=>{});
 function range(pointer:number,length:number){const value=memory();if(!value||value.buffer instanceof SharedArrayBuffer||!Number.isInteger(pointer)||pointer<0||!Number.isInteger(length)||length<0||pointer>value.buffer.byteLength-length)throw Error('browser application memory');return new Uint8Array(value.buffer,pointer,length);}
 function invalidate(){if(invalid)return;invalid=true;for(const listener of [...listeners]){try{listener();}catch{/* 通知异常不能截断原任务关闭。 */}}listeners.clear();rejectDescriptor(Error('browser application invalidated'));}
 function terminal(){invalidate();request=undefined;nextDeadline=undefined;if(pending){const original=pending;pending=undefined;original.reject(Error('browser application closed'));}}
 function run(kind:number,payload:Uint8Array,budget:number){
  if(invalid||stopped()||pending||!(payload instanceof Uint8Array)||payload.length>32748||!Number.isInteger(budget)||budget<1||budget>1000||sequence===4294967295)return Promise.reject(Error('browser application unavailable'));
  const original=nextDeadline;nextDeadline=undefined;const end=Math.min(Math.floor(clock())+budget,expiry(),original??Infinity);if(!Number.isFinite(end)||clock()>=end)return Promise.reject(Error('browser original deadline'));
  const seq=++sequence,copy=payload.slice();request=new Uint8Array(20+copy.length);const header=new DataView(request.buffer);header.setUint32(0,seq,true);header.setUint32(4,kind,true);header.setUint32(8,budget,true);header.setFloat64(12,end,true);request.set(copy,20);
  const operation=new Promise<Uint8Array[]>((resolve,reject)=>{pending={seq,kind,parts:[],bytes:0,end,resolve,reject};});wake();return operation;
 }
 const port:NativeClientPort={
  snapshot:async(payload,budget)=>{const parts=await run(1,payload,budget);if(parts.length===0)throw Error('snapshot empty');const first=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(parts[0])) as {body:{stamp:Stamp}};return{stamp:first.body.stamp,parts};},
  exchange:async(payload,budget)=>{const parts=await run(2,payload,budget);if(parts.length!==1)throw Error('exchange parts');return parts[0];},
  onInvalidate(listener){if(invalid){listener();return()=>{};}listeners.add(listener);return()=>listeners.delete(listener);},
  close(){invalidate();stop();return retired();},
 };
 const imports={cc_desk_browser_client_v1:{
  request:(pointer:number,capacity:number)=>{if(invalid||stopped())return -2;if(capacity!==32768)return -2;if(!request)return -1;const bytes=request;request=undefined;range(pointer,bytes.length).set(bytes);return bytes.length;},
  reply:(seq:number,status:number,pointer:number,length:number)=>{try{if(!pending||seq!==pending.seq||stopped()||invalid||clock()>=pending.end||clock()>=expiry())throw Error('application sequence');if(status===0){if(length<1||length>32768||pending.parts.length>=24000||(pending.bytes+=length)>4194304)throw Error('application bounds');pending.parts.push(range(pointer,length).slice());return;}if(length!==0||status!==1)throw Error('application rejected');const original=pending;pending=undefined;original.resolve(original.parts);}catch{terminal();stop();}},
  descriptor:(pointer:number,length:number)=>{try{if(invalid||stopped()||length<1||length>4096)throw Error('application descriptor');const value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(range(pointer,length))) as BrowserClientDescriptor;if(!value||Object.keys(value).length!==4||!/^[0-9a-f]{64}$/.test(value.binding)||!['Controller','Observer'].includes(value.role)||!/^[1-9][0-9]{0,19}$/.test(value.fence)||!value.target||Object.keys(value.target).length!==5)throw Error('application identity');resolveDescriptor(Object.freeze({...value,target:Object.freeze(value.target)}));}catch{terminal();stop();}},
  invalidated:()=>{invalidate();stop();},
 }};
 return Object.freeze({port,descriptor,imports,invalidate,terminal,limitDeadline:(value:number)=>{if(!Number.isFinite(value)||value<=clock()||pending||nextDeadline!==undefined)return false;nextDeadline=value;return true;}});
}
