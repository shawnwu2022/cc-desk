import {approvedBrowserInputs,type ApprovedBrowserInputs} from './browserEntry.ts';
import type {BrowserExactGrantConfiguration} from './browserGrant.ts';
import {counter,hex,parseJson} from './wire.ts';
import {checkedBrowserEnrollmentLease,type BrowserEnrollmentLease} from './browserEnrollmentLease.ts';

export type PublicBrowserPairingConfiguration=Readonly<{
 version:1;
 profile:Readonly<{origin:string;rpId:string;credentialId:string}>;
 localSpki:string;hostSpki:string;
 trust:Readonly<{hostKeyEpoch:string;clientKeyEpoch:string;origin:string}>;
 grant:Readonly<{
  issuerSpki:string;principal:string;scope:Readonly<{session:string;run:string;generation:string}>;
  role:'Controller'|'Observer';budgetMs:number;peerInstall:Readonly<{budgetMs:number}>;
  profile:Readonly<{origin:string;rpId:string;credentialId:string;credentialSpki:string;backupEligible:boolean;allowBackedUp:boolean}>;
 }>;
 relay:Readonly<{mode?:'legacy'|'rendezvous-v1';endpoint:string;tunnel:string;keyId:string}>;
 artifacts:Readonly<{wasm:string;glue:string}>;
}>;
export type BrowserProfileHandles=Readonly<{
 tlsKey:CryptoKey;grantKey:CryptoKey;
 relaySign:ApprovedBrowserInputs['relay']['identity']['sign'];present:ApprovedBrowserInputs['present'];
}>;
/** 只允许受控模块提供独立事先批准的配置；候选从不作为选择器传入。 */
export type BrowserProfileProvider=Readonly<{
 enrollment?:BrowserEnrollmentLease;
 readApproved(signal:AbortSignal):Promise<Readonly<{configuration:Uint8Array;handles:BrowserProfileHandles}>|undefined>;
}>;

function exact(value:unknown,keys:readonly string[]):asserts value is Record<string,any>{
 if(!value||typeof value!=='object'||Array.isArray(value)||Reflect.ownKeys(value).length!==keys.length)throw Error('shape');
 for(const key of keys){const field=Object.getOwnPropertyDescriptor(value,key);if(!field||!field.enumerable||!('value'in field))throw Error('field');}
}
function decode(value:unknown,min:number,max:number):Uint8Array{
 if(typeof value!=='string'||value.length>Math.ceil(max*4/3)||!/^[A-Za-z0-9_-]+$/.test(value))throw Error('bytes');
 const result=Uint8Array.from(atob(value.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
 const encoded=btoa(Array.from(result,b=>String.fromCharCode(b)).join('')).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
 if(result.length<min||result.length>max||encoded!==value)throw Error('bytes');return result;
}
function budget(value:unknown,max:number){if(typeof value!=='number'||!Number.isInteger(value)||value<1||value>max)throw Error('budget');}
function address(value:unknown,max=512):URL{
 if(typeof value!=='string'||value.length>max||!/^[\x00-\x7f]+$/.test(value)||value.includes('%')||value.includes('\\'))throw Error('url');
 const url=new URL(value);if(url.href!==value||url.username||url.password||url.search||url.hash||url.port==='0')throw Error('url');return url;
}
function freeze<T>(value:T):T{
 if(value&&typeof value==='object'){for(const child of Object.values(value))freeze(child);Object.freeze(value);}return value;
}

/** 仅验证公开语法与交叉字段，不建立身份、批准或密钥绑定。 */
export function parseBrowserPairingConfiguration(bytes:Uint8Array):PublicBrowserPairingConfiguration{
 try{
  if(!(bytes instanceof Uint8Array)||bytes.byteLength>16384)throw Error('bytes');
  const v=parseJson(Uint8Array.from(bytes),16384);
  exact(v,['version','profile','localSpki','hostSpki','trust','grant','relay','artifacts']);
  if(v.version!==1)throw Error('version');
  exact(v.profile,['origin','rpId','credentialId']);const p=v.profile;
  if(typeof p.origin!=='string'||p.origin.length>512)throw Error('origin');
  const origin=new URL(p.origin);
  if(origin.protocol!=='https:'||origin.origin!==p.origin||origin.username||origin.password||origin.pathname!=='/'||origin.search||origin.hash||typeof p.rpId!=='string'||!p.rpId||p.rpId.length>253||!(origin.hostname===p.rpId||origin.hostname.endsWith(`.${p.rpId}`)))throw Error('origin');
  decode(p.credentialId,1,1024);decode(v.localSpki,91,91);decode(v.hostSpki,91,91);
  exact(v.trust,['hostKeyEpoch','clientKeyEpoch','origin']);counter(v.trust.hostKeyEpoch);counter(v.trust.clientKeyEpoch);
  exact(v.relay,Object.prototype.hasOwnProperty.call(v.relay??{},'mode')?['mode','endpoint','tunnel','keyId']:['endpoint','tunnel','keyId']);
  const r=v.relay;
  if(Object.prototype.hasOwnProperty.call(r,'mode')&&r.mode!=='legacy'&&r.mode!=='rendezvous-v1')throw Error('mode');
  if(address(r.endpoint).protocol!=='wss:'||v.trust.origin!==r.endpoint||!decode(r.tunnel,16,16).some(Boolean))throw Error('relay');decode(r.keyId,16,16);
  exact(v.artifacts,['wasm','glue']);
  for(const path of [v.artifacts.wasm,v.artifacts.glue])if(address(path,2048).origin!==p.origin)throw Error('artifact');
  exact(v.grant,['issuerSpki','principal','scope','role','budgetMs','peerInstall','profile']);const g=v.grant;
  decode(g.issuerSpki,91,91);hex(g.principal,32);exact(g.scope,['session','run','generation']);
  hex(g.scope.session,16);counter(g.scope.generation);
  if(typeof g.scope.run!=='string'||!g.scope.run||g.scope.run.length>128||!['Controller','Observer'].includes(g.role))throw Error('grant');
  budget(g.budgetMs,30000);exact(g.peerInstall,['budgetMs']);budget(g.peerInstall.budgetMs,10000);
  exact(g.profile,['origin','rpId','credentialId','credentialSpki','backupEligible','allowBackedUp']);
  if(g.profile.origin!==p.origin||g.profile.rpId!==p.rpId||g.profile.credentialId!==p.credentialId||typeof g.profile.backupEligible!=='boolean'||typeof g.profile.allowBackedUp!=='boolean')throw Error('credential');
  decode(g.profile.credentialSpki,91,91);
  return freeze(v) as PublicBrowserPairingConfiguration;
 }catch{throw Error('browser pairing configuration');}
}

// 两份都是已严格解析的纯值；递归比较全部字段，保留字段省略与显式值的差异。
function same(a:unknown,b:unknown):boolean{
 if(a===b)return true;
 if(!a||!b||typeof a!=='object'||typeof b!=='object')return false;
 const left=a as Record<string,unknown>,right=b as Record<string,unknown>,keys=Object.keys(left);
 return keys.length===Object.keys(right).length&&keys.every(key=>Object.prototype.hasOwnProperty.call(right,key)&&same(left[key],right[key]));
}
function assemble(expected:PublicBrowserPairingConfiguration,handles:BrowserProfileHandles,enrollment?:BrowserEnrollmentLease):ApprovedBrowserInputs<BrowserExactGrantConfiguration>{
 const p=expected.profile,g=expected.grant,r=expected.relay;
 return approvedBrowserInputs({
  profile:{origin:p.origin,rpId:p.rpId,credentialId:decode(p.credentialId,1,1024),tlsKey:handles.tlsKey,grantKey:handles.grantKey},
  localSpki:decode(expected.localSpki,91,91),hostSpki:decode(expected.hostSpki,91,91),
  trust:{hostKeyEpoch:expected.trust.hostKeyEpoch,clientKeyEpoch:expected.trust.clientKeyEpoch,origin:expected.trust.origin},
  grant:{issuerSpki:decode(g.issuerSpki,91,91),principal:g.principal,
   scope:{session:g.scope.session,run:g.scope.run,generation:g.scope.generation},role:g.role,budgetMs:g.budgetMs,peerInstall:{budgetMs:g.peerInstall.budgetMs},
   profile:{origin:g.profile.origin,rpId:g.profile.rpId,credentialId:decode(g.profile.credentialId,1,1024),credentialSpki:decode(g.profile.credentialSpki,91,91),backupEligible:g.profile.backupEligible,allowBackedUp:g.profile.allowBackedUp}},
  relay:{endpoint:r.endpoint,tunnel:decode(r.tunnel,16,16),identity:{keyId:decode(r.keyId,16,16),sign:handles.relaySign},...(Object.prototype.hasOwnProperty.call(r,'mode')?{mode:r.mode}:{})},
  artifacts:{wasm:expected.artifacts.wasm,glue:expected.artifacts.glue},present:handles.present,...(enrollment?{enrollment}:{}),
 });
}

/** 返回受控模块唯一导出所需的 reader；不读取网络、签名、呈现或创建 deadline。 */
export function createApprovedBrowserProfileReader(candidateBytes:Uint8Array,provider?:BrowserProfileProvider):(signal:AbortSignal)=>Promise<ApprovedBrowserInputs<BrowserExactGrantConfiguration>|undefined>{
 return createProfileReader(provider,parseBrowserPairingConfiguration(candidateBytes));
}

/** 仅供受控安装记录启动：批准来自原 provider，不构造候选自比较。 */
export function createInstalledBrowserEnrollmentProfileReader(provider:BrowserProfileProvider&Readonly<{enrollment:BrowserEnrollmentLease}>){
 return createProfileReader(provider,undefined,true);
}

function createProfileReader(provider:BrowserProfileProvider|undefined,candidate?:PublicBrowserPairingConfiguration,installed=false):(signal:AbortSignal)=>Promise<ApprovedBrowserInputs<BrowserExactGrantConfiguration>|undefined>{
 let read:BrowserProfileProvider['readApproved']|undefined;
 let enrollment:BrowserEnrollmentLease|undefined;
 if(provider!==undefined){try{const owns=Object.prototype.hasOwnProperty.call(provider,'enrollment');if('enrollment'in provider&&!owns)throw Error('provider');exact(provider,owns?['readApproved','enrollment']:['readApproved']);if(owns)enrollment=checkedBrowserEnrollmentLease(provider.enrollment);if(typeof provider.readApproved!=='function')throw Error('provider');read=provider.readApproved.bind(provider);}catch{throw Error('browser pairing profile unavailable');}}
 if(installed&&!enrollment)throw Error('browser pairing profile unavailable');
 let original:Promise<ApprovedBrowserInputs<BrowserExactGrantConfiguration>|undefined>|undefined,firstSignal:AbortSignal|undefined;
 return async(signal:AbortSignal)=>{
  if(!original){
   firstSignal=signal;
   original=Promise.resolve().then(async()=>{
    if(signal.aborted||enrollment?.signal.aborted)throw Error('cancelled');
    const supplied=await read?.(signal);
    if(signal.aborted||enrollment?.signal.aborted)throw Error('cancelled');
    if(supplied===undefined)return undefined;
    exact(supplied,['configuration','handles']);exact(supplied.handles,['tlsKey','grantKey','relaySign','present']);
    const h=supplied.handles,handles=Object.freeze({tlsKey:h.tlsKey,grantKey:h.grantKey,relaySign:h.relaySign,present:h.present});
    const expected=parseBrowserPairingConfiguration(supplied.configuration);
    if(candidate&&!same(candidate,expected))throw Error('mismatch');
    return assemble(expected,handles,enrollment);
   }).catch(()=>{throw Error('browser pairing profile unavailable');});
   void original.catch(()=>{});
  }
  const approved=await original;
  if(signal.aborted||firstSignal!.aborted||enrollment?.signal.aborted)throw Error('browser pairing profile unavailable');
  return approved;
 };
}
