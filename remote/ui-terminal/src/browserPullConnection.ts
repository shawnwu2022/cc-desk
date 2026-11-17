import {nativeConnection,type RetirableNativeConnection} from './native.ts';
import type {BrowserNativeClientPort} from './browserNative.ts';
import {command,counter} from './wire.ts';
/** 仅显式选择；选择不是授权，每次 pull 仍由原 Rust P1 认证事实准入。 */
export function browserPullConnection(port:BrowserNativeClientPort,budgetMs=1000):RetirableNativeConnection {
 return nativeConnection({
  onInvalidate:listener=>port.onInvalidate?.(listener)??(()=>{}),
  snapshot:async(payload,budget)=>{
   // 只转换 TerminalClient 发出的有界 canonical 命令，不接受另一套调用格式。
   if(!(payload instanceof Uint8Array)||payload.length>128)throw Error('pull snapshot command');
   const value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(payload)) as {request?:string}|null;
   const request=value?.request;counter(request);const canonical=command('Snapshot',request);
   if(payload.length!==canonical.length||!payload.every((byte,i)=>byte===canonical[i]))throw Error('pull snapshot command');
   return port.snapshotPull(BigInt(request),budget);
  },
  exchange:(payload,budget)=>port.exchange(payload,budget),
  close:()=>port.close(),
 },budgetMs);
}