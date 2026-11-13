/** A cross-bundle lifecycle contract, never evidence of enrollment or approval. */
export type BrowserEnrollmentLease=Readonly<{
 signal:AbortSignal;
 retain:(retirement:Promise<void>)=>void;
}>;

export function checkedBrowserEnrollmentLease(value:unknown):BrowserEnrollmentLease{
 if(!value||typeof value!=='object'||Reflect.ownKeys(value).length!==2)throw Error('browser enrollment unavailable');
 const signal=Object.getOwnPropertyDescriptor(value,'signal'),retain=Object.getOwnPropertyDescriptor(value,'retain');
 if(!signal?.enumerable||!('value'in signal)||!retain?.enumerable||!('value'in retain)||typeof AbortSignal==='undefined'||!(signal.value instanceof AbortSignal)||typeof retain.value!=='function')throw Error('browser enrollment unavailable');
 return Object.isFrozen(value)?value as BrowserEnrollmentLease:Object.freeze({signal:signal.value,retain:retain.value});
}

/** Internal owner primitive. Closing admission precedes abort callbacks and drain. */
export function createBrowserEnrollmentLease(){
 const controller=new AbortController(),pending=new Set<Promise<void>>();let failed=false,finished=false;
 let resolve!:()=>void,reject!:(error:Error)=>void;
 const closed=new Promise<void>((a,b)=>{resolve=a;reject=b;});void closed.catch(()=>{});
 function drain(){if(!controller.signal.aborted||pending.size||finished)return;finished=true;if(failed)reject(Error('browser enrollment retirement uncertain'));else resolve();}
 const lease:BrowserEnrollmentLease=Object.freeze({signal:controller.signal,retain(work:Promise<void>){
  if(controller.signal.aborted||!(work instanceof Promise))throw Error('browser enrollment unavailable');
  if(pending.has(work))return;pending.add(work);
  void work.then(()=>{pending.delete(work);drain();},()=>{failed=true;pending.delete(work);drain();});
 }});
 return Object.freeze({lease,revoke(){if(controller.signal.aborted)return;controller.abort();drain();},waitClosed:()=>closed});
}
