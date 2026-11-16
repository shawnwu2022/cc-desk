import {createBrowserRegistrationEntry,snapshotBrowserRegistrationInputs,type ApprovedBrowserRegistrationInputs,type BrowserRegistrationLifetime} from './browserRegistrationEntry.ts';
import {readRegistrationArtifact,importRegistrationArtifact} from './browserRegistrationArtifacts.ts';
let sourceClaimed=false;
const unavailable=()=>Error('browser registration source unavailable');

/** 准备不延长 entry 原始 setup 与 registration 截止时间。 */
export function controlledBrowserRegistrationSource(){
 let inputs:ApprovedBrowserRegistrationInputs|undefined,consume=false,expiry=0;
 let cleanup=()=>{};let check:()=>void=()=>{throw unavailable();};
 const controller=new AbortController();
 const close=()=>{inputs=undefined;controller.abort();cleanup();};
 return Object.freeze({close,signal:controller.signal,async prepare(signal:AbortSignal):Promise<void>{
  if(sourceClaimed)throw unavailable();sourceClaimed=true;
  const page=document,origin=location.origin,firstPerf=performance.now(),firstWall=Date.now();
  let lastPerf=firstPerf,lastWall=firstWall,failed=false;
  const clock=()=>{const p=performance.now(),w=Date.now();if(!Number.isFinite(p)||!Number.isFinite(w)||p<lastPerf||w<lastWall||(inputs&&Math.max(p-lastPerf,w-lastWall)>1000))failed=true;lastPerf=p;lastWall=w;return failed?Infinity:Math.max(p-firstPerf,w-firstWall);};
  const trusted=()=>typeof Window!=='undefined'&&globalThis instanceof Window&&document===page&&top===self&&isSecureContext&&crossOriginIsolated&&new URL(origin).protocol==='https:'&&location.origin===origin;
  if(!(signal instanceof AbortSignal)||signal.aborted||!trusted()||page.visibilityState!=='visible')throw unavailable();
  check=()=>{if(controller.signal.aborted||signal.aborted||!trusted()||clock()>=300000||(inputs&&(page.visibilityState!=='visible'||clock()>=expiry)))throw unavailable();};
  const watch=()=>{try{check();}catch{close();}},abort=()=>close();
  signal.addEventListener('abort',abort,{once:true});page.addEventListener('visibilitychange',watch);page.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);globalThis.addEventListener('freeze',abort);
  const timer=setInterval(watch,100);
  cleanup=()=>{clearInterval(timer);signal.removeEventListener('abort',abort);page.removeEventListener('visibilitychange',watch);page.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);globalThis.removeEventListener('freeze',abort);};
  try{
   const bytes=await readRegistrationArtifact(new URL('/browser-registration.js',origin).href,origin,1048576,controller.signal);check();
   const module=await importRegistrationArtifact(bytes,controller.signal);check();
   if(Object.keys(module).length!==1||typeof module.readApprovedBrowserRegistrationInputs!=='function')throw unavailable();
   const approved:ApprovedBrowserRegistrationInputs|undefined=await module.readApprovedBrowserRegistrationInputs(controller.signal);check();
   if(!approved||page.visibilityState!=='visible')throw unavailable();
   inputs=snapshotBrowserRegistrationInputs(approved);expiry=Math.min(300000,clock()+60000);
  }catch{close();throw unavailable();}
 },async register(lifetime:BrowserRegistrationLifetime){
  check();if(consume||!inputs)throw unavailable();consume=true;
  const approved=inputs;inputs=undefined;cleanup();
  try{return createBrowserRegistrationEntry(approved,lifetime);}catch{close();throw unavailable();}
 }});
}
