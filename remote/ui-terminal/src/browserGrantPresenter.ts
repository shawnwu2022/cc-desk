import type {ApprovedBrowserInputs} from './browserEntry.ts';
import {counter,hex} from './wire.ts';

const active=new WeakSet<Document>();let sequence=0;
const unavailable=()=>Error('browser grant presentation unavailable');
function fields(value:unknown,names:readonly string[]):Record<string,any>{
 if(!value||typeof value!=='object'||![Object.prototype,null].includes(Object.getPrototypeOf(value))||Reflect.ownKeys(value).length!==names.length)throw unavailable();
 const copy:Record<string,unknown>={};
 for(const name of names){const field=Object.getOwnPropertyDescriptor(value,name);if(!field?.enumerable||!('value'in field))throw unavailable();copy[name]=field.value;}
 return copy;
}
function snapshot(value:unknown){
 const g=fields(value,['version','context','principalId','scope','role','challenge','grantId','createdHostMs','expiresHostMs']);
 const c=fields(g.context,['hostEndpointId','clientEndpointId','hostKeyEpoch','clientKeyEpoch','activation','incarnation','policyEpoch','issuerEpoch','origin']);
 const s=fields(g.scope,['session','run','generation']);
 if(g.version!==1||!['Observer','Controller'].includes(g.role))throw unavailable();
 for(const key of ['principalId','challenge','grantId'])hex(g[key],32,true);
 for(const key of ['hostEndpointId','clientEndpointId','activation'])hex(c[key],32,true);
 for(const key of ['hostKeyEpoch','clientKeyEpoch','incarnation','policyEpoch','issuerEpoch'])counter(c[key],true);
 hex(s.session,16,true);counter(s.generation);
 if(typeof s.run!=='string'||!s.run.length||new TextEncoder().encode(s.run).length>128||/[\u0000-\u001f\u007f-\u009f]/.test(s.run))throw unavailable();
 counter(g.createdHostMs,true);counter(g.expiresHostMs,true);
 const duration=BigInt(g.expiresHostMs)-BigInt(g.createdHostMs);if(duration<=0n||duration>900000n)throw unavailable();
 if(typeof c.origin!=='string'||c.origin.length>512)throw unavailable();
 const relay=new URL(c.origin);if(relay.href!==c.origin||relay.protocol!=='wss:'||relay.username||relay.password||relay.search||relay.hash)throw unavailable();
 const record={...g,context:c,scope:s},json=JSON.stringify(record,null,2);
 if(new TextEncoder().encode(JSON.stringify(record)).length>8192)throw unavailable();
 return {record,json,duration,role:g.role as 'Observer'|'Controller'};
}
// Render directional controls literally, never as text that can reorder labels.
const visible=(text:string)=>text.replace(/[\u200e\u200f\u2028-\u202e\u2066-\u2069]/g,c=>'\\u'+c.charCodeAt(0).toString(16).padStart(4,'0'));

/** Controlled same-origin presentation only; never enrollment or grant authority. */
export function createBrowserGrantPresenter(origin:string):ApprovedBrowserInputs['present']{
 const address=new URL(origin);
 if(address.origin!==origin||address.protocol!=='https:'||typeof Window==='undefined'||!(globalThis instanceof Window)||typeof Document==='undefined'||!(document instanceof Document)||typeof HTMLDialogElement==='undefined')throw unavailable();
 const page=document,body=page.body;let used=false;
 const eligible=()=>document===page&&page.body===body&&page.visibilityState==='visible'&&globalThis.top===globalThis.self&&globalThis.isSecureContext&&globalThis.crossOriginIsolated&&location.origin===origin;
 if(!eligible()||!body)throw unavailable();
 return async(verified,signal)=>{
  if(used)throw unavailable();used=true;
  if(!(signal instanceof AbortSignal)||signal.aborted||!eligible()||active.has(page))throw unavailable();
  let displayed:ReturnType<typeof snapshot>;try{displayed=snapshot(verified);}catch{throw unavailable();}
  active.add(page);
  return new Promise<void>((resolve,reject)=>{
   const dialog=page.createElement('dialog'),title=page.createElement('h2'),summary=page.createElement('p'),details=page.createElement('pre'),buttons=page.createElement('div'),cancel=page.createElement('button'),approve=page.createElement('button');
   const focus=page.activeElement;let settled=false,originalMarkup='';
   const live=()=>eligible()&&!signal.aborted&&dialog.isConnected&&dialog.parentNode===body&&dialog.open&&dialog.outerHTML===originalMarkup;
   const changed=(records:MutationRecord[])=>{if(records.some(record=>record.target===dialog||dialog.contains(record.target))||!live())finish(false);},observer=new MutationObserver(changed);
   const abort=()=>finish(false),visibility=()=>{if(!eligible())finish(false);};
   const denied=(event:Event)=>{event.preventDefault();finish(false);};
   const accepted=(event:Event)=>{if(event.isTrusted)finish(live());};
   function finish(allowed:boolean){
    if(settled)return;settled=true;observer.disconnect();
    signal.removeEventListener('abort',abort);page.removeEventListener('visibilitychange',visibility);page.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);globalThis.removeEventListener('freeze',abort);
    dialog.removeEventListener('cancel',denied);dialog.removeEventListener('close',abort);cancel.removeEventListener('click',denied);approve.removeEventListener('click',accepted);
    try{if(dialog.open)dialog.close();}finally{dialog.remove();active.delete(page);}
    if(eligible()&&focus instanceof HTMLElement&&focus.isConnected)try{focus.focus({preventScroll:true});}catch{/* Focus restoration does not confer approval. */}
    if(allowed)resolve();else reject(unavailable());
   }
   try{
    dialog.className='browser-grant-dialog';title.id=`browser-grant-title-${++sequence}`;title.textContent='Approve remote session';dialog.setAttribute('aria-labelledby',title.id);
    const g=displayed.record;
    summary.textContent=visible(`${displayed.role==='Controller'?'View and control':'View only'} this running session. Session: ${g.scope.session}; run: ${g.scope.run}; generation: ${g.scope.generation}. Requested maximum grant duration: ${displayed.duration} ms. The Host deadline still applies. No automatic reconnect. Device verification follows approval.`);
    details.textContent=visible(displayed.json);details.setAttribute('aria-label','Complete verified grant');
    cancel.type='button';cancel.textContent='Cancel';cancel.autofocus=true;approve.type='button';approve.textContent='Approve this connection';buttons.className='browser-grant-actions';buttons.append(cancel,approve);dialog.append(title,summary,details,buttons);
    dialog.addEventListener('cancel',denied);dialog.addEventListener('close',abort);cancel.addEventListener('click',denied);approve.addEventListener('click',accepted);signal.addEventListener('abort',abort,{once:true});page.addEventListener('visibilitychange',visibility);page.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);globalThis.addEventListener('freeze',abort);
    body.append(dialog);dialog.showModal();cancel.focus({preventScroll:true});dialog.scrollTop=0;originalMarkup=dialog.outerHTML;observer.observe(page,{childList:true,subtree:true,characterData:true,attributes:true});if(!live())finish(false);
   }catch{finish(false);}
  });
 };
}
