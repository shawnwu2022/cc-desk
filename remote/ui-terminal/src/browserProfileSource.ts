import type {TerminalConnectionSource} from './connectionSource.ts';
import {browserConnectionSource} from './browserSource.ts';
import type {ApprovedBrowserInputs} from './browserEntry.ts';
import {browserReconnectSession,type BrowserRetryPolicy} from './browserReconnect.ts';
/** One original operator module/read/source per page. No serialized key material. */
function controlledProfile<T>(build:(inputs:ApprovedBrowserInputs)=>T){
 let original:Promise<T|undefined>|undefined,opening=false;
 return Object.freeze({async open(signal:AbortSignal,setupDeadline=performance.timeOrigin+performance.now()+10000){
  if(opening||signal.aborted)throw Error('browser profile source unavailable');opening=true;
  try{
   const trusted=()=>typeof Window!=='undefined'&&globalThis instanceof Window&&globalThis.top===globalThis.self&&globalThis.isSecureContext&&globalThis.crossOriginIsolated&&typeof Document!=='undefined'&&document instanceof Document&&document.visibilityState==='visible'&&new URL(location.origin).protocol==='https:'&&typeof Worker==='function';
   if(!trusted())return undefined;
   const now=()=>performance.timeOrigin+performance.now();
   if(!Number.isFinite(setupDeadline)||setupDeadline<=now()||setupDeadline>now()+10000)throw Error('browser profile deadline');
   if(!original){const address=new URL('./browser-profile.js',location.href).href;
    const controller=new AbortController(),abort=()=>controller.abort();
    const page=document,origin=location.origin,visible=()=>{if(document!==page||page.visibilityState!=='visible')abort();};
    signal.addEventListener('abort',abort,{once:true});
    page.addEventListener('visibilitychange',visible);page.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);
    const timer=setTimeout(abort,Math.max(0,setupDeadline-now()));
    original=Promise.resolve().then(async()=>{
     const response=await fetch(address,{credentials:'omit',redirect:'error',cache:'no-store',signal:controller.signal});
     if(controller.signal.aborted||now()>=setupDeadline||!trusted()||document!==page||location.origin!==origin)throw Error('browser profile source cancelled');
     if(response.status===404)return undefined;if(!response.ok)throw Error('browser profile source unavailable');await response.body?.cancel();
     if(controller.signal.aborted||now()>=setupDeadline||!trusted()||document!==page||location.origin!==origin)throw Error('browser profile source cancelled');
     const module=await import(/* @vite-ignore */ address);
     if(Object.keys(module).length!==1||typeof module.readApprovedBrowserInputs!=='function')throw Error('browser profile source unavailable');
     if(controller.signal.aborted||now()>=setupDeadline||!trusted()||document!==page||location.origin!==origin)throw Error('browser profile source cancelled');
     const inputs:ApprovedBrowserInputs|undefined=await module.readApprovedBrowserInputs(controller.signal);
     try{
      if(controller.signal.aborted||now()>=setupDeadline||!trusted()||document!==page||location.origin!==origin)throw Error('browser profile source cancelled');
      return inputs===undefined?undefined:build(inputs);
     }catch{await inputs?.enrollment?.retire?.();throw Error('browser profile source cancelled');}
    }).finally(()=>{clearTimeout(timer);signal.removeEventListener('abort',abort);page.removeEventListener('visibilitychange',visible);page.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);});void original.catch(()=>{});
   }
   const source=await original;
   // 原 source 负责 admission 前取消/过期时退休已取得的 store session。
   if(source)return source;
   if(signal.aborted||now()>=setupDeadline)throw Error('browser profile source cancelled');return undefined;
  }finally{opening=false;}
 }});
}


/** 旧入口仍只持一个原单会话 source，不自动升级批准或重试。 */
export function controlledBrowserProfileSource():TerminalConnectionSource{
 const reader=controlledProfile(browserConnectionSource);let opening=false;
 return Object.freeze({async open(signal:AbortSignal,setupDeadline=performance.timeOrigin+performance.now()+10000){
  if(opening)throw Error('browser profile source unavailable');opening=true;
  try{const source=await reader.open(signal,setupDeadline);return source?await source.open(signal,setupDeadline):undefined;}finally{opening=false;}
 }});
}

/** 受控读取完成后才冻结本地重试预算；原读取另有不可延长的 setup 截止。 */
export function controlledBrowserReconnectSession(policy:BrowserRetryPolicy,signal:AbortSignal,setupDeadline=performance.timeOrigin+performance.now()+10000){
 return controlledProfile(inputs=>browserReconnectSession(inputs,policy,signal)).open(signal,setupDeadline);
}
