import type {NativeClientPort} from './native.ts';
import {ProjectionReceiver,type Stamp} from './reducer.ts';
type Bridge={readonly instanceId:string;invoke(command:string,payload:unknown):Promise<unknown>};
function bridge():Bridge{
 const value=typeof window==='undefined'?undefined:(window as Window&{__CC_DESK_DOCUMENT__?:Bridge}).__CC_DESK_DOCUMENT__;
 if(!value||typeof value.invoke!=='function')throw Error('original document bridge unavailable');return value;
}
function encode(bytes:Uint8Array):string{return btoa(Array.from(bytes,b=>String.fromCharCode(b)).join('')).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');}
function decode(value:unknown,limit:number):Uint8Array{
 if(typeof value!=='string'||value.length>Math.ceil(limit/3)*4||!/^[A-Za-z0-9_-]*$/.test(value))throw Error('native byte bounds');
 let raw:string;try{raw=atob(value.replace(/-/g,'+').replace(/_/g,'/'));}catch{throw Error('native byte encoding');}
 const bytes=Uint8Array.from(raw,c=>c.charCodeAt(0));if(bytes.length>limit||encode(bytes)!==value)throw Error('native byte encoding');return bytes;
}
function object(value:unknown):Record<string,unknown>{if(!value||typeof value!=='object'||Array.isArray(value))throw Error('native response');return value as Record<string,unknown>;}
export function documentClientPort(resourceId:number):NativeClientPort{
 if(!Number.isInteger(resourceId)||resourceId<1||resourceId>4294967295)throw Error('native resource id');
 const original=bridge(),instance=original.instanceId;let closed=false,retired:Promise<void>|undefined;
 if(typeof instance!=='string'||instance.length<1||instance.length>128)throw Error('original document instance unavailable');
 const originalBridge=()=>{const current=bridge();if(current!==original||current.instanceId!==instance)throw Error('original document changed');return original;};
 const open=()=>{if(closed)throw Error('native port closed');return originalBridge();};
 const body=(payload:Uint8Array,budgetMs:number)=>{open();if(!(payload instanceof Uint8Array)||payload.length>16270||!Number.isInteger(budgetMs)||budgetMs<1||budgetMs>1000)throw Error('native request bounds');return {resourceId,budgetMs,payload:encode(payload)};};
 return {
  async exchange(payload,budgetMs){const result=object(await open().invoke('remote_client_exchange',body(payload,budgetMs)));open();if(Object.keys(result).length!==1)throw Error('native response');return decode(result.payload,16270);},
  async snapshot(payload,budgetMs){
   const result=object(await open().invoke('remote_client_snapshot',body(payload,budgetMs)));open();if(Object.keys(result).length!==2||!Array.isArray(result.parts)||result.parts.length<1||result.parts.length>24000)throw Error('native snapshot bounds');
   const stamp=result.stamp as Stamp;new ProjectionReceiver(stamp.target,stamp.binding).begin(stamp);
   let total=0;const parts=result.parts.map(value=>{const bytes=decode(value,32768);total+=bytes.length;if(total>4*1024*1024)throw Error('native snapshot budget');return bytes;});return {stamp,parts};
  },
  close(){if(!retired){closed=true;retired=(async()=>{const result=await originalBridge().invoke('remote_client_close',{resourceId});if(result!==null&&result!==undefined)throw Error('native close response');})();}return retired;},
 };
}
