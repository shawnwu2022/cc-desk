import {browserProfileBoundary} from './browserApproval.ts';
import {createBrowserEnrollmentStore,type StoredBrowserEnrollmentMaterial} from './browserEnrollmentStore.ts';
import {parseBrowserPairingConfiguration,type PublicBrowserPairingConfiguration} from './browserPairingProfile.ts';
import {counter} from './wire.ts';

type Configuration=PublicBrowserPairingConfiguration;
export type BrowserRegistrationInstallationPlan=Readonly<{
 databaseName:string;expectedRevision:string|null;relaySpki:string;requestedUserHandle:string;
 configuration:Omit<Configuration,'profile'|'grant'>&Readonly<{
  profile:Omit<Configuration['profile'],'credentialId'>;
  grant:Omit<Configuration['grant'],'profile'>&Readonly<{profile:Omit<Configuration['grant']['profile'],'credentialId'|'credentialSpki'|'backupEligible'>}>;
 }>;
}>;
export type BrowserRegistrationInstallationSession=Readonly<{
 outcome:Promise<unknown>;completed:Promise<void>;close():void;waitClosed():Promise<void>;
}>;
type Metadata=Readonly<{revision:string;state:'active'|'revoking'|'revoked'}>;
const unavailable=()=>Error('browser registration installation unavailable');
const uncertain=()=>Error('browser registration installation retirement uncertain');
const encoder=new TextEncoder(),used=new WeakSet<object>();
function exact(value:unknown,names:readonly string[]):asserts value is Record<string,any>{
 if(!value||typeof value!=='object'||Array.isArray(value)||Reflect.ownKeys(value).length!==names.length)throw unavailable();
 for(const name of names)field(value,name);
}
function field(value:object,name:string):any{
 const item=Object.getOwnPropertyDescriptor(value,name);
 if(!item?.enumerable||!('value'in item))throw unavailable();return item.value;
}
function snapshot(value:unknown,depth=0):any{
 if(value===null||typeof value==='string'||typeof value==='boolean'||typeof value==='number'&&Number.isFinite(value))return value;
 if(!value||typeof value!=='object'||Array.isArray(value)||depth>8||![Object.prototype,null].includes(Object.getPrototypeOf(value)))throw unavailable();
 const result:Record<string,unknown>=Object.create(null);
 for(const key of Reflect.ownKeys(value)){if(typeof key!=='string')throw unavailable();result[key]=snapshot(field(value,key),depth+1);}return Object.freeze(result);
}
function same(a:unknown,b:unknown):boolean{
 if(a===b)return true;if(!a||!b||typeof a!=='object'||typeof b!=='object')return false;
 const left=a as Record<string,unknown>,right=b as Record<string,unknown>,names=Object.keys(left);
 return names.length===Object.keys(right).length&&names.every(name=>Object.hasOwn(right,name)&&same(left[name],right[name]));
}
function decode(value:unknown,min:number,max:number):Uint8Array<ArrayBuffer>{
 if(typeof value!=='string'||value.length>Math.ceil(max*4/3)||!/^[A-Za-z0-9_-]+$/.test(value))throw unavailable();
 const result=Uint8Array.from(atob(value.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
 const encoded=btoa(Array.from(result,n=>String.fromCharCode(n)).join('')).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
 if(result.length<min||result.length>max||encoded!==value)throw unavailable();return result;
}
function template(value:Configuration){
 const {credentialId:_id,...profile}=value.profile;
 const {credentialId:_grantId,credentialSpki:_spki,backupEligible:_backup,...grantProfile}=value.grant.profile;
 return {...value,profile,grant:{...value.grant,profile:grantProfile}};
}
function plan(value:unknown):BrowserRegistrationInstallationPlan{
 const approved=snapshot(value);exact(approved,['databaseName','expectedRevision','configuration','relaySpki','requestedUserHandle']);
 if(typeof approved.databaseName!=='string'||!/^[A-Za-z0-9._-]{1,64}$/.test(approved.databaseName))throw unavailable();
 if(approved.expectedRevision!==null)counter(approved.expectedRevision);
 decode(approved.relaySpki,91,91);decode(approved.requestedUserHandle,1,64);
 const c=approved.configuration;exact(c,['version','profile','localSpki','hostSpki','trust','grant','relay','artifacts']);
 exact(c.profile,['origin','rpId']);exact(c.grant,['issuerSpki','principal','scope','role','budgetMs','peerInstall','profile']);
 exact(c.grant.profile,['origin','rpId','allowBackedUp']);
 // 只为复用公开语法校验补占位字段；这不是安装值，也不产生凭证授权。
 const parsed=parseBrowserPairingConfiguration(encoder.encode(JSON.stringify({...c,profile:{...c.profile,credentialId:'AQ'},grant:{...c.grant,profile:{...c.grant.profile,credentialId:'AQ',credentialSpki:c.localSpki,backupEligible:false}}})));
 if(!same(template(parsed),c)||new Set([c.localSpki,c.grant.issuerSpki,approved.relaySpki]).size!==3)throw unavailable();
 return approved as unknown as BrowserRegistrationInstallationPlan;
}
function configuration(value:unknown,approved:BrowserRegistrationInstallationPlan):Uint8Array{
 exact(value,['kind','receipt','scope','installation']);if(value.kind!=='BrowserInstallationApproved')throw unavailable();counter(value.receipt);
 exact(value.installation,['databaseName','expectedRevision','configuration','relaySpki','requestedUserHandle']);const i=value.installation;
 if(i.databaseName!==approved.databaseName||i.expectedRevision!==approved.expectedRevision||i.relaySpki!==approved.relaySpki||i.requestedUserHandle!==approved.requestedUserHandle||typeof i.configuration!=='string'||i.configuration.length>16384)throw unavailable();
 const bytes=encoder.encode(i.configuration),parsed=parseBrowserPairingConfiguration(bytes);
 if(!same(template(parsed),approved.configuration)||!same(value.scope,parsed.grant.scope))throw unavailable();
 return encoder.encode(JSON.stringify(parsed));
}

/** 仅限同源受信 owner：session 必须是本次原 Worker wrapper，keys 必须是原 bridge 的同一 handles。
 * 公开 outcome/receipt 不可跨上下文导入为 bearer；本函数不验证任意调用者，也不补建信任。
 * approvedPlan 必须来自独立事先批准的来源，不可由候选或 outcome 选择。缺省来源保持 Locked。
 * deadline/signal 是原调用者的绝对截止与取消源；内部取消只向下传播，不影响登记的正常内部关闭。
 */
export function createBrowserRegistrationInstallation(options:Readonly<{
 session:BrowserRegistrationInstallationSession;keys:StoredBrowserEnrollmentMaterial['keys'];
 approvedPlan:BrowserRegistrationInstallationPlan;signal:AbortSignal;deadline:number;
}>){
 exact(options,['session','keys','approvedPlan','signal','deadline']);
 const {session,signal,deadline}=options,approved=plan(options.approvedPlan),origin=approved.configuration.profile.origin;
 if(typeof Window==='undefined'||!(globalThis instanceof Window)||typeof Document==='undefined'||!(document instanceof Document)||!(signal instanceof AbortSignal)||signal.aborted||!session||typeof session!=='object'||used.has(session))throw unavailable();
 const outcome=field(session,'outcome'),completed=field(session,'completed'),wait=field(session,'waitClosed'),stop=field(session,'close');
 if(!(outcome instanceof Promise)||!(completed instanceof Promise)||typeof wait!=='function'||typeof stop!=='function')throw unavailable();
 exact(options.keys,['tlsKey','grantKey','relayKey']);
 let keys:StoredBrowserEnrollmentMaterial['keys']|undefined=Object.freeze({...options.keys});
 if(new Set(Object.values(keys)).size!==3)throw unavailable();
 const boundary={origin,rpId:approved.configuration.profile.rpId,credentialId:new Uint8Array([1]),grantKey:keys.grantKey};
 browserProfileBoundary({...boundary,tlsKey:keys.tlsKey});browserProfileBoundary({...boundary,tlsKey:keys.relayKey});
 const page=document,controller=new AbortController();
 let lastPerf=performance.now(),lastWall=Date.now(),clockFailed=false;
 const startPerf=lastPerf,startWall=lastWall,startEpoch=performance.timeOrigin+startPerf;
 if(!Number.isFinite(deadline)||!Number.isFinite(startEpoch)||!Number.isFinite(startWall)||deadline<=startEpoch||deadline-startEpoch>120000)throw unavailable();
 let stopping=false,started=false,finished=false,retirementFailed=false,originalsDone=false,operationDone=true;
 let timer:ReturnType<typeof setInterval>|undefined,resolveClosed!:()=>void,rejectClosed!:(error:Error)=>void;
 const closed=new Promise<void>((r,j)=>{resolveClosed=r;rejectClosed=j});void closed.catch(()=>{});
 function now(){
  const p=performance.now(),w=Date.now();
  if(!Number.isFinite(p)||!Number.isFinite(w)||p<lastPerf||w<lastWall||Math.max(p-lastPerf,w-lastWall)>1000)clockFailed=true;
  lastPerf=p;lastWall=w;return clockFailed?Infinity:startEpoch+Math.max(p-startPerf,w-startWall);
 }
 function eligible(){return !stopping&&!signal.aborted&&!controller.signal.aborted&&document===page&&location.origin===origin&&globalThis.top===globalThis.self&&globalThis.isSecureContext&&globalThis.crossOriginIsolated&&page.visibilityState==='visible'&&now()<deadline;}
 function live(){if(!eligible()){close();throw unavailable();}}
 function cleanup(){
  if(timer!==undefined)clearInterval(timer);signal.removeEventListener('abort',close);page.removeEventListener('visibilitychange',visibility);page.removeEventListener('freeze',close);globalThis.removeEventListener('pagehide',close);
 }
 function retire(){
  if(finished||!stopping||!originalsDone||!operationDone)return;
  finished=true;cleanup();keys=undefined;if(retirementFailed)rejectClosed(uncertain());else resolveClosed();
 }
 function close(){
  if(!stopping){stopping=true;controller.abort();try{stop.call(session);}catch{retirementFailed=true;}}
  retire();
 }
 function visibility(){if(page.visibilityState!=='visible')close();}
 if(!eligible())throw unavailable();used.add(session);
 // 捕获原对象与函数，不读调用者后来替换的 Promise；全部真实 settle 后才可释放 keys。
 const decision=outcome.then(value=>snapshot(value));void decision.catch(()=>{});void completed.catch(()=>{});
 const retirement=Promise.resolve().then(()=>{const original=wait.call(session);if(!(original instanceof Promise))throw uncertain();return original;});void retirement.catch(()=>{retirementFailed=true;});
 const originals=Promise.allSettled([decision,completed,retirement]);
 void originals.then(()=>{originalsDone=true;retire();});
 signal.addEventListener('abort',close,{once:true});page.addEventListener('visibilitychange',visibility);page.addEventListener('freeze',close);globalThis.addEventListener('pagehide',close);
 timer=setInterval(()=>{if(!eligible())close();},100);if(!eligible())close();
 async function prove(){
  const handles=[keys!.tlsKey,keys!.grantKey,keys!.relayKey],publicKeys=[approved.configuration.localSpki,approved.configuration.grant.issuerSpki,approved.relaySpki],purposes=['tls','grant','relay'];
  const nonce=crypto.getRandomValues(new Uint8Array(32));
  for(let index=0;index<3;index++){
   live();const publicKey=await crypto.subtle.importKey('spki',decode(publicKeys[index],91,91),{name:'ECDSA',namedCurve:'P-256'},false,['verify']);
   live();const message=new Uint8Array([...encoder.encode(`CC-DESK-BROWSER-INSTALL-${purposes[index]}-PREFLIGHT-v1\0`),...nonce]);
   const proof=await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},handles[index],message);live();
   const valid=await crypto.subtle.verify({name:'ECDSA',hash:'SHA-256'},publicKey,proof,message);live();if(!valid)throw unavailable();
  }
 }
 function install(...args:never[]):Promise<Metadata>{
  if(started)return Promise.reject(unavailable());started=true;operationDone=false;
  const operation=Promise.resolve().then(async()=>{
   const results=await originals;live();if(args.length||results.some(result=>result.status!=='fulfilled'))throw unavailable();
   const bytes=configuration((results[0] as PromiseFulfilledResult<unknown>).value,approved);live();
   const store=createBrowserEnrollmentStore({origin,databaseName:approved.databaseName});
   const result=await store.installApproved(approved.expectedRevision??undefined,async originalSignal=>{
    live();if(originalSignal!==controller.signal)throw unavailable();await prove();live();return{configuration:bytes,keys:keys!};
   },controller.signal);
   // 原 IDB oncomplete 已证明提交；迟到取消不能抹去落盘事实。metadata 仅是历史，不授权连接。
   return result;
  }).catch(()=>{throw unavailable();}).finally(()=>{operationDone=true;close();});
  void operation.catch(()=>{});return operation;
 }
 return Object.freeze({install,close,waitClosed:()=>closed});
}
