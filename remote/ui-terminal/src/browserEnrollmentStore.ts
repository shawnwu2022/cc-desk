import {browserProfileBoundary} from './browserApproval.ts';
import {createBrowserEnrollmentAuthority} from './browserEnrollment.ts';
import {createApprovedBrowserProfileReader,parseBrowserPairingConfiguration,type BrowserProfileHandles} from './browserPairingProfile.ts';
import type {ApprovedBrowserInputs} from './browserEntry.ts';
import {counter,hex} from './wire.ts';

export type StoredBrowserEnrollmentMaterial=Readonly<{configuration:Uint8Array;keys:Readonly<{tlsKey:CryptoKey;grantKey:CryptoKey;relayKey:CryptoKey}>}>;
type Metadata=Readonly<{revision:string;state:'active'|'revoking'|'revoked'}>;
type RecordValue=(Metadata&{owner?:string})&Partial<StoredBrowserEnrollmentMaterial>;
type Session=Readonly<{revision:string;provider:ReturnType<typeof createBrowserEnrollmentAuthority>['provider'];close:()=>void;waitClosed:()=>Promise<void>}>;
const unavailable=()=>Error('browser enrollment store unavailable');
const uncertain=()=>Error('browser enrollment store retirement uncertain');
const forever=()=>new Promise<void>(()=>{});
const idleSignal=()=>new AbortController().signal;
function exact(value:unknown,names:string[]):asserts value is Record<string,any>{
 if(!value||typeof value!=='object'||Reflect.ownKeys(value).length!==names.length)throw unavailable();
 for(const name of names){const field=Object.getOwnPropertyDescriptor(value,name);if(!field?.enumerable||!('value'in field))throw unavailable();}
}
function material(value:unknown,origin:string):StoredBrowserEnrollmentMaterial{
 exact(value,['configuration','keys']);exact(value.keys,['tlsKey','grantKey','relayKey']);
 const configuration=parseBrowserPairingConfiguration(value.configuration),p=configuration.profile,k=value.keys;
 if(p.origin!==origin||new Set([k.tlsKey,k.grantKey,k.relayKey]).size!==3)throw unavailable();
 const profile={...p,credentialId:Uint8Array.from(atob(p.credentialId.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0)),grantKey:k.grantKey};
 browserProfileBoundary({...profile,tlsKey:k.tlsKey});browserProfileBoundary({...profile,tlsKey:k.relayKey});
 return Object.freeze({configuration:new TextEncoder().encode(JSON.stringify(configuration)),keys:Object.freeze({tlsKey:k.tlsKey,grantKey:k.grantKey,relayKey:k.relayKey})});
}
function record(value:unknown,origin:string):RecordValue|undefined{
 if(value===undefined)return undefined;
 if(!value||typeof value!=='object')throw unavailable();
 const state=Object.getOwnPropertyDescriptor(value,'state')?.value,owns=Object.prototype.hasOwnProperty.call(value,'owner');
 exact(value,['version','revision','state',...(owns?['owner']:[]),...(state==='active'?['configuration','keys']:[])]);
 if(value.version!==1||!['active','revoking','revoked'].includes(state))throw unavailable();counter(value.revision);
 if(owns){hex(value.owner,16,true);if(state==='revoked')throw unavailable();}
 return Object.freeze({revision:value.revision,state,...(owns?{owner:value.owner}:{}),...(state==='active'?material({configuration:value.configuration,keys:value.keys},origin):{})});
}
const metadata=(value:RecordValue):Metadata=>Object.freeze({revision:value.revision,state:value.state});

/** Explicit controlled-origin persistence of already-approved handles, not an enrollment ceremony. */
export function createBrowserEnrollmentStore(options:Readonly<{origin:string;databaseName:string}>){
 exact(options,['origin','databaseName']);const {origin,databaseName}=options;
 if(typeof origin!=='string'||new URL(origin).origin!==origin||new URL(origin).protocol!=='https:'||typeof databaseName!=='string'||!/^[A-Za-z0-9._-]{1,64}$/.test(databaseName)||typeof Window==='undefined'||!(globalThis instanceof Window)||typeof Document==='undefined'||!(document instanceof Document)||!globalThis.indexedDB||!navigator.locks||typeof BroadcastChannel!=='function')throw unavailable();
 const page=document,name=`cc-desk-enrollment:${databaseName}`,storeName='enrollment';
 const eligible=()=>document===page&&location.origin===origin&&globalThis.top===globalThis.self&&globalThis.isSecureContext&&globalThis.crossOriginIsolated&&page.visibilityState==='visible';
 if(!eligible())throw unavailable();
 function openDatabase(signal:AbortSignal):Promise<IDBDatabase>{
  return new Promise((resolve,reject)=>{
   const request=indexedDB.open(databaseName,1);
   request.onupgradeneeded=()=>{try{if(signal.aborted||request.result.objectStoreNames.length)throw unavailable();request.result.createObjectStore(storeName);}catch{request.transaction?.abort();}};
   request.onerror=()=>reject(unavailable());
   request.onsuccess=()=>{const db=request.result;if(signal.aborted||db.objectStoreNames.length!==1||!db.objectStoreNames.contains(storeName)){db.close();reject(unavailable());return;}db.onversionchange=()=>db.close();resolve(db);};
   // A blocked original open remains pending; no timeout claims it retired.
  });
 }
 async function atomic<T>(mode:IDBTransactionMode,signal:AbortSignal,change:(current:RecordValue|undefined)=>{result:T;write?:RecordValue}):Promise<T>{
  if(!(signal instanceof AbortSignal)||signal.aborted)throw unavailable();
  const db=await openDatabase(signal);
  return new Promise<T>((resolve,reject)=>{
   let transaction:IDBTransaction,result:T,transformed=false;
   try{transaction=db.transaction(storeName,mode,{durability:'strict'});}catch{db.close();reject(unavailable());return;}
   const abort=()=>{try{transaction.abort();}catch{/* Commit may already be final; only its original event decides. */}};
   const cleanup=()=>{signal.removeEventListener('abort',abort);db.close();};
   transaction.onabort=()=>{cleanup();reject(unavailable());};
   transaction.oncomplete=()=>{cleanup();if(transformed)resolve(result);else reject(unavailable());};
   const store=transaction.objectStore(storeName),request=store.get('current');
   request.onsuccess=()=>{try{if(signal.aborted)throw unavailable();const next=change(record(request.result,origin));result=next.result;transformed=true;if(next.write)store.put({version:1,...next.write},'current');}catch{abort();}};
   signal.addEventListener('abort',abort,{once:true});if(signal.aborted)abort();
  });
 }
 const read=(signal=idleSignal())=>atomic('readonly',signal,current=>({result:current}));
 async function status(){const value=await read();return value?metadata(value):undefined;}
 async function installApproved(expectedRevision:string|undefined,load:(signal:AbortSignal)=>Promise<StoredBrowserEnrollmentMaterial>,signal:AbortSignal):Promise<Metadata>{
  if(!eligible()||!(signal instanceof AbortSignal)||signal.aborted||typeof load!=='function')throw unavailable();if(expectedRevision!==undefined)counter(expectedRevision);
  return navigator.locks.request(name,{mode:'exclusive',ifAvailable:true},async lock=>{
   if(!lock||!eligible()||signal.aborted)throw unavailable();
   const matches=(current:RecordValue|undefined)=>expectedRevision===undefined?current===undefined:current?.state==='revoked'&&current.revision===expectedRevision&&!current.owner;
   const before=await read(signal);if(!matches(before))throw unavailable();
   // The original loader owns its work until it settles, even when it ignores abort.
   const loaded=await load(signal);if(signal.aborted||!eligible())throw unavailable();const approved=material(loaded,origin);
   const revision=(BigInt(expectedRevision??'0')+1n).toString();counter(revision);
   return atomic('readwrite',signal,current=>{if(!matches(current)||!eligible())throw unavailable();const write:RecordValue={revision,state:'active',...approved};return{write,result:metadata(write)};});
  });
 }
 async function openExisting(present:BrowserProfileHandles['present'],signal:AbortSignal):Promise<Session|undefined>{
  if(!eligible()||!(signal instanceof AbortSignal)||signal.aborted||typeof present!=='function')throw unavailable();
  let resolveReady!:(value:Session|undefined)=>void,rejectReady!:(error:Error)=>void,resolveClosed!:()=>void,rejectClosed!:(error:Error)=>void;
  const ready=new Promise<Session|undefined>((r,j)=>{resolveReady=r;rejectReady=j;}),closed=new Promise<void>((r,j)=>{resolveClosed=r;rejectClosed=j;});void closed.catch(()=>{});
  const released=navigator.locks.request(name,{mode:'exclusive',ifAvailable:true},async lock=>{
   if(!lock){resolveReady(undefined);return;}
   const authority=createBrowserEnrollmentAuthority(),channel=new BroadcastChannel(name);
   let claimed:RecordValue|undefined;
   const close=()=>authority.revoke(),visibility=()=>{if(!eligible())close();};
   channel.onmessage=event=>{const value=event.data;if(value&&typeof value==='object'&&typeof value.revision==='string'&&(!claimed||value.revision===claimed.revision))close();};
   signal.addEventListener('abort',close,{once:true});page.addEventListener('visibilitychange',visibility);page.addEventListener('freeze',close);globalThis.addEventListener('pagehide',close);
   const cleanup=()=>{channel.close();signal.removeEventListener('abort',close);page.removeEventListener('visibilitychange',visibility);page.removeEventListener('freeze',close);globalThis.removeEventListener('pagehide',close);};
   try{
    if(signal.aborted||!eligible())throw unavailable();
    const owner=Array.from(crypto.getRandomValues(new Uint8Array(16)),n=>n.toString(16).padStart(2,'0')).join('');
    claimed=await atomic('readwrite',signal,current=>{if(!current||current.state!=='active')return{result:undefined};if(current.owner)throw uncertain();const write={...current,owner};return{write,result:write};});
    if(!claimed){close();await authority.waitClosed();resolveReady(undefined);return;}
    await authority.approve(async ownerSignal=>{
     if(ownerSignal.aborted||signal.aborted||!eligible())throw unavailable();const keys=claimed!.keys!;
     const handles:BrowserProfileHandles={tlsKey:keys.tlsKey,grantKey:keys.grantKey,present,relaySign:async(request,requestSignal)=>{if(ownerSignal.aborted||requestSignal.aborted)throw unavailable();const signed=await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},keys.relayKey,new Uint8Array(request.transcript));if(ownerSignal.aborted||requestSignal.aborted)throw unavailable();return new Uint8Array(signed);}};
     return{configuration:claimed!.configuration!,handles};
    });
    const current=await read(signal);
    if(!current||current.state!=='active'||current.revision!==claimed.revision||current.owner!==claimed.owner||authority.provider.enrollment.signal.aborted||!eligible())throw unavailable();
    resolveReady(Object.freeze({revision:claimed.revision,provider:authority.provider,close,waitClosed:()=>closed}));
    await authority.waitClosed();
   }catch{close();rejectReady(unavailable());try{await authority.waitClosed();}catch{cleanup();rejectClosed(uncertain());await forever();}}
   finally{
    if(claimed){try{await atomic('readwrite',idleSignal(),current=>{if(!current||current.revision!==claimed!.revision||current.owner!==claimed!.owner||current.state==='revoked')throw uncertain();const {owner:_owner,...write}=current;return{write,result:undefined};});}catch{cleanup();rejectClosed(uncertain());await forever();}}
    cleanup();
   }
  });
  void released.then(()=>resolveClosed(),()=>{rejectReady(unavailable());rejectClosed(uncertain());});
  return ready;
 }
 async function revoke():Promise<Metadata>{
  const selected=await atomic('readwrite',idleSignal(),current=>{
   if(current?.state==='revoked')return{result:current};const write:RecordValue={revision:current?.revision??'1',state:'revoking',...(current?.owner?{owner:current.owner}:{})};return{write,result:write};
  });
  if(selected.state==='revoked')return metadata(selected);
  const channel=new BroadcastChannel(name);try{channel.postMessage({revision:selected.revision});}finally{channel.close();}
  return navigator.locks.request(name,{mode:'exclusive'},async()=>atomic('readwrite',idleSignal(),current=>{
   if(!current||current.revision!==selected.revision||current.state==='active'||current.owner)throw uncertain();const write:RecordValue={revision:current.revision,state:'revoked'};return{write,result:metadata(write)};
  }));
 }
 return Object.freeze({installApproved,openExisting,revoke,status});
}

/** One original persisted supplier read per controlled module, never a reload/recovery loop. */
export function createStoredBrowserProfileReader(candidateBytes:Uint8Array,store:ReturnType<typeof createBrowserEnrollmentStore>,present:BrowserProfileHandles['present']){
 const candidate=new TextEncoder().encode(JSON.stringify(parseBrowserPairingConfiguration(candidateBytes)));
 let original:Promise<ApprovedBrowserInputs|undefined>|undefined,firstSignal:AbortSignal|undefined,session:Session|undefined;
 return async(signal:AbortSignal)=>{
  if(!original){firstSignal=signal;original=Promise.resolve().then(async()=>{
   session=await store.openExisting(present,signal);if(!session)return undefined;
   try{return await createApprovedBrowserProfileReader(candidate,session.provider)(signal);}catch{session.close();await session.waitClosed();throw unavailable();}
  });void original.catch(()=>{});}
  const value=await original;if(signal.aborted||firstSignal!.aborted||session?.provider.enrollment.signal.aborted)throw unavailable();return value;
 };
}
