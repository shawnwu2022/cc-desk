import {approvedBrowserInputs,type ApprovedBrowserInputs} from './browserEntry.ts';
import {createBrowserEnrollmentLease} from './browserEnrollmentLease.ts';
import {browserConnectionSource} from './browserSource.ts';
import {TerminalClient} from './client.ts';
import type {TerminalConnection} from './connectionSource.ts';
import type {Target} from './reducer.ts';

export type BrowserRetryPolicy=Readonly<{mode:'safe-idle-retry-v1';budgetMs:number}>;
export type BrowserRetryState='New'|'Connecting'|'Connected'|'Retiring'|'Waiting'|'Closed'|'Unknown';
type Generation={id:number;child:ReturnType<typeof createBrowserEnrollmentLease>;found?:TerminalConnection;unlisten?:()=>void;recovering:boolean};
const backoff=[250,500,1000] as const;
const sameTarget=(a:Target,b:Target)=>a.host===b.host&&a.session===b.session&&a.run===b.run&&a.generation===b.generation&&a.incarnation===b.incarnation;

/** 显式本地重试预算，不代表 Host 批准。旧单会话 source 不使用此入口。 */
export function browserReconnectSession(inputs:ApprovedBrowserInputs,policy:BrowserRetryPolicy,signal:AbortSignal){
 const fields=policy&&Object.getOwnPropertyDescriptors(policy),mode=fields?.mode,budget=fields?.budgetMs;
 const budgetMs=budget&&'value'in budget?budget.value:undefined;
 if(!fields||Reflect.ownKeys(fields).length!==2||!mode?.enumerable||!('value'in mode)||mode.value!=='safe-idle-retry-v1'||!budget?.enumerable||!('value'in budget)||typeof budgetMs!=='number'||!Number.isInteger(budgetMs)||budgetMs<1||budgetMs>60000||!(signal instanceof AbortSignal))throw Error('browser retry policy');
 const approved=approvedBrowserInputs(inputs),enrollment=approved.enrollment;
 if(approved.relay.mode!=='rendezvous-v1'||!enrollment?.retire||enrollment.signal.aborted||signal.aborted||approved.grant.peerInstall?.controlLease!=='live-v1'||approved.grant.peerInstall.snapshotPull!=='snapshot-pull-v1')throw Error('browser retry unavailable');
 if(typeof Document==='undefined'||!(document instanceof Document))throw Error('browser retry document');
 const page=document,origin=location.origin,controller=new AbortController(),tasks=new Set<Promise<unknown>>();
 let state:BrowserRetryState='New',closed=false,uncertain=false,replacements=0,nextId=0,current:Generation|undefined,client:TerminalClient|undefined,target:Target|undefined,role:TerminalConnection['role']|undefined;
 let last=performance.now();if(!Number.isFinite(last)||last<0)throw Error('browser retry clock');
 const end=last+budgetMs;
 let endTimer:ReturnType<typeof setTimeout>|undefined,retryTimer:ReturnType<typeof setTimeout>|undefined,releaseDelay:(()=>void)|undefined;
 let resolveOwned!:()=>void,rejectOwned!:(error:Error)=>void;
 const owned=new Promise<void>((a,b)=>{resolveOwned=a;rejectOwned=b});void owned.catch(()=>{});
 let resolveFinal!:()=>void,rejectFinal!:(error:Error)=>void;
 const final=new Promise<void>((a,b)=>{resolveFinal=a;rejectFinal=b});void final.catch(()=>{});
 let closing:Promise<void>|undefined;
 const active=()=>{const now=performance.now();if(!Number.isFinite(now)||now<last)return false;last=now;return !closed&&!signal.aborted&&!enrollment.signal.aborted&&now<end&&document===page&&location.origin===origin&&page.visibilityState==='visible';};
 function track<T>(work:Promise<T>):Promise<T>{tasks.add(work);void work.then(()=>tasks.delete(work),()=>tasks.delete(work));return work;}
 async function retire(g:Generation){
  g.unlisten?.();g.unlisten=undefined;
  let failed=false;
  try{g.found?.connection.close();await g.found?.connection.waitClosed();}catch{failed=true;}
  g.child.revoke();try{await g.child.waitClosed();}catch{failed=true;}
  if(failed){uncertain=true;throw Error('browser retry retirement uncertain');}
 }
 function close():Promise<void>{
  if(closing)return closing;
  const resolve=resolveFinal,reject=rejectFinal;closing=final;
  closed=true;state='Retiring';
  if(endTimer!==undefined)clearTimeout(endTimer);if(retryTimer!==undefined)clearTimeout(retryTimer);releaseDelay?.();releaseDelay=undefined;
  controller.abort();client?.disconnect();
  let parentRetirement:Promise<void>;
  try{parentRetirement=enrollment!.retire!();}catch{parentRetirement=Promise.reject(Error('browser parent retirement uncertain'));}
  void parentRetirement.catch(()=>{});
  const pending=[...tasks],generation=current;
  void Promise.resolve().then(async()=>{
   await Promise.allSettled(pending);
   if(generation)try{await retire(generation);}catch{uncertain=true;}
   if(current&&current!==generation)try{await retire(current);}catch{uncertain=true;}
   signal.removeEventListener('abort',abort);enrollment!.signal.removeEventListener('abort',abort);
   page.removeEventListener('visibilitychange',visibility);page.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);
   if(uncertain)rejectOwned(Error('browser retry retirement uncertain'));else resolveOwned();
   const result=await Promise.allSettled([owned,parentRetirement]);
   if(result.some(v=>v.status==='rejected')){state='Unknown';reject(Error('browser retry retirement uncertain'));}else{state='Closed';resolve();}
  }).catch(()=>{uncertain=true;state='Unknown';rejectOwned(Error('browser retry retirement uncertain'));reject(Error('browser retry retirement uncertain'));});
  return closing;
 }
 const abort=()=>{void close();},visibility=()=>{if(document!==page||page.visibilityState!=='visible')abort();};
 // 父持有总体资源收据；子代只等待其自身资源，不反向等待父退休。
 enrollment.retain(owned);
 signal.addEventListener('abort',abort,{once:true});enrollment.signal.addEventListener('abort',abort,{once:true});
 page.addEventListener('visibilitychange',visibility);page.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);
 const expire=()=>{if(!active()){abort();return;}endTimer=setTimeout(expire,Math.max(0,end-performance.now()));};
 endTimer=setTimeout(expire,Math.max(0,end-performance.now()));
 async function open(previous?:Generation):Promise<TerminalClient>{
  if(!active())throw Error('browser retry stopped');
  let child!:ReturnType<typeof createBrowserEnrollmentLease>;
  child=createBrowserEnrollmentLease(()=>{child.revoke();return child.waitClosed();});
  const g:Generation={id:++nextId,child,recovering:false};current=g;state='Connecting';
  try{
   const deadline=performance.timeOrigin+Math.min(end,performance.now()+10000);
   const found=await browserConnectionSource({...approved,enrollment:child.lease}).open(controller.signal,deadline);
   if(!found)throw Error('browser retry unavailable');g.found=found;
   if(!active()||current!==g)throw Error('browser retry stale');
   if(previous){
    if(!client||!target||!sameTarget(target,found.target)||role!==found.role||!previous.found||!client.canReconnectFrom(previous.found.connection))throw Error('browser retry identity or unsettled input');
   }else{target=Object.freeze({...found.target});role=found.role;client=new TerminalClient(target,found.binding);}
   client!.connect(found.connection,found.binding);
   g.unlisten=found.connection.onInvalidate?.(()=>{
    if(closed||current!==g||g.recovering)return;g.recovering=true;
    void track(Promise.resolve().then(()=>recover(g))).catch(()=>{void close();});
   });
   await client!.snapshot();
   if(!active()||current!==g||client!.phase!=='Synced')throw Error('browser retry setup stopped');
   state='Connected';return client!;
  }catch(error){await retire(g);throw error;}
 }
 async function recover(g:Generation){
  state='Retiring';await retire(g);
  if(!active()||current!==g||!g.found||!client?.canReconnectFrom(g.found.connection)||replacements>=backoff.length){void close();return;}
  const delay=backoff[replacements++];state='Waiting';
  await new Promise<void>(resolve=>{releaseDelay=resolve;retryTimer=setTimeout(()=>{retryTimer=undefined;releaseDelay=undefined;resolve();},delay);});
  if(!active()||current!==g||!client.canReconnectFrom(g.found.connection)){void close();return;}
  await open(g);
 }
 let started=false;
 return Object.freeze({
  start():Promise<TerminalClient>{if(started||closed)return Promise.reject(Error('browser retry already started'));started=true;const work=track(Promise.resolve().then(()=>open()));void work.catch(()=>{void close();});return work;},
  observe:()=>Object.freeze({state,generation:current?.id??0,replacements,localDeadline:performance.timeOrigin+end,client,role}),
  close,
  waitClosed:()=>final,
 });
}
