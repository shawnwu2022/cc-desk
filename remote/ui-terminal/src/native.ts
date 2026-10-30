import type {NativeConnection} from './client.ts';
import {ProjectionReceiver,type Stamp} from './reducer.ts';
import {decodeProjection,decodeReply} from './wire.ts';
/** Backend-supplied port to an owned Rust NativeClient. Providing this object
 * is dependency injection, not authentication. A shell must admit its original
 * document on every call and revoke/retire its resource on document loss. */
export interface NativeClientPort {
  snapshot(payload:Uint8Array,budgetMs:number):Promise<{stamp:Stamp;parts:readonly Uint8Array[]}>;
  exchange(payload:Uint8Array,budgetMs:number):Promise<Uint8Array>;
  close():Promise<void>;
}
export interface RetirableNativeConnection extends NativeConnection {waitClosed():Promise<void>}
export function nativeConnection(port:NativeClientPort,budgetMs=1000):RetirableNativeConnection {
  if(!Number.isInteger(budgetMs)||budgetMs<1||budgetMs>1000)throw Error('invalid native budget');
  let closed=false,busy=false,epoch=0,retired:Promise<void>|undefined;
  const close=()=>{
    if(closed)return;closed=true;epoch++;
    try{retired=Promise.resolve(port.close());}catch{retired=Promise.reject(Error('native close failed'));}
    // Preserve the rejected result for waitClosed without an unhandled rejection
    // from the synchronous NativeConnection.close contract.
    void retired.catch(()=>{});
  };
  const run=async<T>(work:(check:()=>void)=>Promise<T>):Promise<T>=>{
    if(closed||busy)throw Error('native unavailable');busy=true;const current=epoch;
    const end=performance.now()+budgetMs;
    const check=()=>{if(closed||epoch!==current)throw Error('stale native completion');if(performance.now()>=end)throw Error('native deadline');};
    let timer:ReturnType<typeof setTimeout>|undefined;
    try{
      const value=await Promise.race([work(check),new Promise<never>((_,reject)=>{timer=setTimeout(()=>reject(Error('native deadline')),Math.max(0,end-performance.now()));})]);
      check();return value;
    }catch(error){close();throw error;}
    finally{busy=false;if(timer!==undefined)clearTimeout(timer);}
  };
  const exchange=(payload:Uint8Array)=>run(async()=>{
    const reply=await port.exchange(payload.slice(),budgetMs);
    if(!(reply instanceof Uint8Array)||reply.length>16270)throw Error('native reply bounds');
    decodeReply(reply);return reply.slice();
  });
  return {
    snapshot:(payload,sink)=>run(async check=>{
      const batch=await port.snapshot(payload.slice(),budgetMs);
      if(!batch||!Array.isArray(batch.parts)||batch.parts.length<1||batch.parts.length>24000)throw Error('native snapshot bounds');
      const receiver=new ProjectionReceiver(batch.stamp.target,batch.stamp.binding);receiver.begin(batch.stamp);
      let bytes=0,installed=false;const parts:Uint8Array[]=[];
      for(const part of batch.parts){
        if(!(part instanceof Uint8Array)||part.length>32768||installed)throw Error('native part bounds');
        bytes+=part.length;if(bytes>4*1024*1024)throw Error('native snapshot budget');
        const result=receiver.accept(decodeProjection(part));
        if(result==='Gap'||result==='Delta')throw Error('native snapshot coverage');
        installed=result==='Installed';parts.push(part.slice());
      }
      if(!installed||!receiver.ack())throw Error('incomplete native snapshot');
      check();sink.begin(batch.stamp);for(const part of parts){check();sink.part(part);}
    }),
    acknowledge:exchange,window:exchange,input:exchange,query:exchange,settle:exchange,close,
    waitClosed:()=>retired??Promise.reject(Error('native still open')),
  };
}
