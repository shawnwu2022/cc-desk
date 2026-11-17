/** A cross-bundle lifecycle contract, never evidence of enrollment or approval. */
export type BrowserEnrollmentLease=Readonly<{
 signal:AbortSignal;
 retain:(retirement:Promise<void>)=>void;
 retire?:()=>Promise<void>;
}>;

export function checkedBrowserEnrollmentLease(value:unknown):BrowserEnrollmentLease{
 if(!value||typeof value!=='object')throw Error('browser enrollment unavailable');
 const retire=Object.getOwnPropertyDescriptor(value,'retire');
 if(('retire'in value&&!retire)||Reflect.ownKeys(value).length!==(retire?3:2)||(retire&&(!retire.enumerable||!('value'in retire)||typeof retire.value!=='function')))throw Error('browser enrollment unavailable');
 const signal=Object.getOwnPropertyDescriptor(value,'signal'),retain=Object.getOwnPropertyDescriptor(value,'retain');
 if(!signal?.enumerable||!('value'in signal)||!retain?.enumerable||!('value'in retain)||typeof AbortSignal==='undefined'||!(signal.value instanceof AbortSignal)||typeof retain.value!=='function')throw Error('browser enrollment unavailable');
 return Object.isFrozen(value)?value as BrowserEnrollmentLease:Object.freeze({signal:signal.value,retain:retain.value,...(retire?{retire:retire.value}:{})});
}

/** Internal owner primitive. Closing admission precedes abort callbacks and drain. */
export function createBrowserEnrollmentLease(retire?:()=>Promise<void>){
 if(retire!==undefined&&typeof retire!=='function')throw Error('browser enrollment unavailable');
 const controller=new AbortController(),pending=new Set<Promise<void>>();let failed=false,finished=false;
 let resolve!:()=>void,reject!:(error:Error)=>void;
 const closed=new Promise<void>((a,b)=>{resolve=a;reject=b;});void closed.catch(()=>{});
 function drain(){if(!controller.signal.aborted||pending.size||finished)return;finished=true;if(failed)reject(Error('browser enrollment retirement uncertain'));else resolve();}
 const lease:BrowserEnrollmentLease=Object.freeze({...(retire?{retire}:{}),signal:controller.signal,retain(work:Promise<void>){
  if(controller.signal.aborted||!(work instanceof Promise))throw Error('browser enrollment unavailable');
  if(pending.has(work))return;pending.add(work);
  void work.then(()=>{pending.delete(work);drain();},()=>{failed=true;pending.delete(work);drain();});
 }});
 return Object.freeze({lease,revoke(){if(controller.signal.aborted)return;controller.abort();drain();},waitClosed:()=>closed});
}
