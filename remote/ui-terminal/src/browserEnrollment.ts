import {browserProfileBoundary} from './browserApproval.ts';
import {parseBrowserPairingConfiguration,type BrowserProfileHandles,type BrowserProfileProvider} from './browserPairingProfile.ts';
import {createBrowserEnrollmentLease} from './browserEnrollmentLease.ts';

export type BrowserApprovedEnrollmentMaterial=Readonly<{configuration:Uint8Array;handles:BrowserProfileHandles}>;
type Load=(signal:AbortSignal)=>Promise<BrowserApprovedEnrollmentMaterial|undefined>;
function exact(value:unknown,keys:readonly string[]):asserts value is Record<string,any>{
 if(!value||typeof value!=='object'||Reflect.ownKeys(value).length!==keys.length)throw Error('browser enrollment unavailable');
 for(const key of keys){const field=Object.getOwnPropertyDescriptor(value,key);if(!field?.enumerable||!('value'in field))throw Error('browser enrollment unavailable');}
}
function snapshot(value:BrowserApprovedEnrollmentMaterial):BrowserApprovedEnrollmentMaterial{
 exact(value,['configuration','handles']);exact(value.handles,['tlsKey','grantKey','relaySign','present']);
 const configuration=parseBrowserPairingConfiguration(value.configuration),h=value.handles;
 const handles=Object.freeze({tlsKey:h.tlsKey,grantKey:h.grantKey,relaySign:h.relaySign,present:h.present});
 if(typeof handles.relaySign!=='function'||typeof handles.present!=='function')throw Error('browser enrollment unavailable');
 const p=configuration.profile;
 browserProfileBoundary({...p,credentialId:Uint8Array.from(atob(p.credentialId.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0)),tlsKey:handles.tlsKey,grantKey:handles.grantKey});
 return Object.freeze({configuration:new TextEncoder().encode(JSON.stringify(configuration)),handles});
}

/** Only controlled owner code receives approve/revoke. No candidate or serialized record import.
 * load must independently supply previously approved, correctly bound existing handles/metadata.
 * This mechanism does not authenticate an operator or implement an enrollment ceremony. */
export function createBrowserEnrollmentAuthority(){
 const lifetime=createBrowserEnrollmentLease();
 let state:'unapproved'|'preparing'|'approved'|'unavailable'|'revoked'='unapproved',readStarted=false;
 let record:BrowserApprovedEnrollmentMaterial|undefined,pending:Promise<void>|undefined,originalRead:Promise<void>|undefined;
 const unavailable=()=>Error('browser enrollment unavailable');
 function revoke(){state='revoked';record=undefined;lifetime.revoke();}
 function approve(load:Load):Promise<void>{
  if(state!=='unapproved'||readStarted||typeof load!=='function')return Promise.reject(unavailable());
  state='preparing';
  const loaded=Promise.resolve().then(()=>lifetime.lease.signal.aborted?undefined:load(lifetime.lease.signal));
  lifetime.lease.retain(loaded.then(()=>{}));
  const operation=loaded.then(value=>{
   if(lifetime.lease.signal.aborted)throw unavailable();
   if(value===undefined){state='unavailable';return;}
   record=snapshot(value);state='approved';
  }).catch(()=>{if(state!=='revoked')revoke();throw unavailable();}).finally(()=>{pending=undefined;});
  pending=operation;void operation.catch(()=>{});
  lifetime.lease.retain(operation.then(()=>{},()=>{}));
  return operation;
 }
 const readApproved:BrowserProfileProvider['readApproved']=(signal)=>{
  if(!(signal instanceof AbortSignal)||lifetime.lease.signal.aborted)return Promise.reject(unavailable());
  // Reserve the complete read before an already-aborted first signal can revoke us.
  let retired!:()=>void;
  lifetime.lease.retain(new Promise<void>(resolve=>{retired=resolve;}));
  if(!readStarted){
   readStarted=true;
   if(state==='unapproved')state='unavailable';
   const first=signal,abort=()=>revoke();
   originalRead=Promise.resolve().then(async()=>{if(pending)await pending;if(first.aborted||lifetime.lease.signal.aborted)throw unavailable();});
   void originalRead.catch(()=>{});
   first.addEventListener('abort',abort,{once:true});
   void originalRead.then(()=>first.removeEventListener('abort',abort),()=>first.removeEventListener('abort',abort));
   if(first.aborted)revoke();
  }
  const result=originalRead!.then(()=>{
   if(signal.aborted||lifetime.lease.signal.aborted)throw unavailable();
   if(state!=='approved'||!record)return undefined;
   return Object.freeze({configuration:record.configuration.slice(),handles:record.handles});
  },()=>{throw unavailable();});
  void result.then(retired,retired);
  return result;
 };
 const provider=Object.freeze({readApproved,enrollment:lifetime.lease});
 return Object.freeze({provider,approve,revoke,waitClosed:lifetime.waitClosed});
}
