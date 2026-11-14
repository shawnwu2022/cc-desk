import {createBrowserRegistrationEntry,type ApprovedBrowserRegistrationInputs,type BrowserRegistrationLifetime} from './browserRegistrationEntry.ts';
import {readRegistrationArtifact,importRegistrationArtifact} from './browserRegistrationArtifacts.ts';
let sourceClaimed=false;

/** 与普通 /browser-profile.js 完全分离；缺失来源不发起登记，所有结局均消耗本次读取。 */
export function controlledBrowserRegistrationSource(){
 return Object.freeze({async register(lifetime:BrowserRegistrationLifetime){
  if(sourceClaimed)throw Error('browser registration source consumed');sourceClaimed=true;
  const {signal,setupDeadline}=lifetime,page=document,origin=location.origin,controller=new AbortController();
  const now=()=>performance.timeOrigin+performance.now();
  const firstPerf=performance.now(),firstWall=Date.now(),epoch=performance.timeOrigin+firstPerf;
  let lastPerf=firstPerf,lastWall=firstWall,clockFailed=false;
  const clock=()=>{const p=performance.now(),w=Date.now();if(!Number.isFinite(p)||!Number.isFinite(w)||p<lastPerf||w<lastWall||Math.max(p-lastPerf,w-lastWall)>1000)clockFailed=true;lastPerf=p;lastWall=w;return clockFailed?Infinity:epoch+Math.max(p-firstPerf,w-firstWall);};
  const trusted=()=>typeof Window!=='undefined'&&globalThis instanceof Window&&document===page&&page.visibilityState==='visible'&&top===self&&isSecureContext&&crossOriginIsolated&&new URL(origin).protocol==='https:'&&location.origin===origin;
  if(!(signal instanceof AbortSignal)||signal.aborted||!trusted()||!Number.isFinite(setupDeadline)||setupDeadline<=now()||setupDeadline-now()>10000)throw Error('browser registration source unavailable');
  const abort=()=>controller.abort(),visible=()=>{if(!trusted()||clock()>=setupDeadline)abort();};
  signal.addEventListener('abort',abort,{once:true});page.addEventListener('visibilitychange',visible);page.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);
  const timer=setTimeout(abort,Math.max(0,setupDeadline-now()));
  const watchdog=setInterval(visible,100);
  const live=()=>{if(controller.signal.aborted||!trusted()||clock()>=setupDeadline)throw Error('browser registration source unavailable');};
  try{
   const bytes=await readRegistrationArtifact(new URL('/browser-registration.js',origin).href,origin,1048576,controller.signal);live();
   const module=await importRegistrationArtifact(bytes,controller.signal);live();
   if(Object.keys(module).length!==1||typeof module.readApprovedBrowserRegistrationInputs!=='function')throw Error('browser registration source unavailable');
   const inputs:ApprovedBrowserRegistrationInputs|undefined=await module.readApprovedBrowserRegistrationInputs(controller.signal);live();
   return inputs===undefined?undefined:createBrowserRegistrationEntry(inputs,lifetime);
  }catch{throw Error('browser registration source unavailable');}
  finally{clearTimeout(timer);clearInterval(watchdog);signal.removeEventListener('abort',abort);page.removeEventListener('visibilitychange',visible);page.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);}
 }});
}
