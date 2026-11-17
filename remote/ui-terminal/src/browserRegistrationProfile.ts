import {browserProfileBoundary} from './browserApproval.ts';
import type {ApprovedBrowserRegistrationInputs} from './browserRegistrationEntry.ts';
import {snapshotBrowserRegistrationPlan,type BrowserRegistrationInstallationPlan} from './browserRegistrationPlan.ts';
import {BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME} from './browserProductStartup.ts';
import {parseJson} from './wire.ts';

export type PublicBrowserRegistrationConfiguration=Readonly<{
 version:1;plan:BrowserRegistrationInstallationPlan;artifacts:Readonly<{wasm:string;glue:string}>;
}>;
/** 受控 provider 独立选定批准来源；公开候选从不作为选择器传入。 */
export type BrowserRegistrationProvider=Readonly<{
 readApproved(signal:AbortSignal):Promise<Readonly<{configuration:Uint8Array;keys:ApprovedBrowserRegistrationInputs['keys']}>|undefined>;
}>;
const unavailable=()=>Error('browser registration profile unavailable');
function exact(value:unknown,names:readonly string[]):asserts value is Record<string,any>{
 if(!value||typeof value!=='object'||Array.isArray(value)||Reflect.ownKeys(value).length!==names.length)throw unavailable();
 for(const name of names){const item=Object.getOwnPropertyDescriptor(value,name);if(!item?.enumerable||!('value'in item))throw unavailable();}
}
function same(a:unknown,b:unknown):boolean{
 if(a===b)return true;if(!a||!b||typeof a!=='object'||typeof b!=='object')return false;
 const left=a as Record<string,unknown>,right=b as Record<string,unknown>,names=Object.keys(left);
 return names.length===Object.keys(right).length&&names.every(name=>Object.prototype.hasOwnProperty.call(right,name)&&same(left[name],right[name]));
}

/** 公开语法校验不是批准；登记和后续终端 artifacts 各自保持完整绑定。 */
export function parseBrowserRegistrationConfiguration(bytes:Uint8Array):PublicBrowserRegistrationConfiguration{
 try{
  if(!(bytes instanceof Uint8Array)||bytes.byteLength>16384)throw unavailable();
  const value=parseJson(Uint8Array.from(bytes),16384);exact(value,['version','plan','artifacts']);if(value.version!==1)throw unavailable();
  const plan=snapshotBrowserRegistrationPlan(value.plan);exact(value.artifacts,['wasm','glue']);
  for(const address of [value.artifacts.wasm,value.artifacts.glue]){
   if(typeof address!=='string'||address.length>2048||!/^[\x00-\x7f]+$/.test(address)||address.includes('%')||address.includes('\\'))throw unavailable();
   const url=new URL(address);
   if(url.href!==address||url.protocol!=='https:'||url.origin!==plan.configuration.profile.origin||url.username||url.password||url.search||url.hash||url.port==='0')throw unavailable();
  }
  return Object.freeze({version:1,plan,artifacts:Object.freeze({wasm:value.artifacts.wasm,glue:value.artifacts.glue})});
 }catch{throw Error('browser registration configuration');}
}

/** 供 /browser-registration.js 唯一导出使用；不签名、不读网络/库、不创建 deadline。 */
export function createApprovedBrowserRegistrationReader(candidateBytes:Uint8Array,provider?:BrowserRegistrationProvider){
 return reader(parseBrowserRegistrationConfiguration(candidateBytes),provider);
}

/** 产品库必须已在原批准中选定，不能改写通用计划或自动迁移其他库。 */
export function createProductBrowserRegistrationReader(candidateBytes:Uint8Array,provider?:BrowserRegistrationProvider){
 const candidate=parseBrowserRegistrationConfiguration(candidateBytes);
 if(candidate.plan.databaseName!==BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME)throw unavailable();
 return reader(candidate,provider);
}

function reader(candidate:PublicBrowserRegistrationConfiguration,provider:BrowserRegistrationProvider|undefined){
 let read:BrowserRegistrationProvider['readApproved']|undefined;
 if(provider!==undefined){try{exact(provider,['readApproved']);if(typeof provider.readApproved!=='function')throw unavailable();read=provider.readApproved.bind(provider);}catch{throw unavailable();}}
 let original:Promise<ApprovedBrowserRegistrationInputs|undefined>|undefined,firstSignal:AbortSignal|undefined;
 return async(signal:AbortSignal):Promise<ApprovedBrowserRegistrationInputs|undefined>=>{
  if(!original){
   firstSignal=signal;
   original=Promise.resolve().then(async()=>{
    if(!(signal instanceof AbortSignal)||signal.aborted)throw unavailable();
    const supplied=await read?.(signal);
    if(signal.aborted)throw unavailable();if(supplied===undefined)return undefined;
    exact(supplied,['configuration','keys']);exact(supplied.keys,['tlsKey','grantKey','relayKey']);
    const keys=Object.freeze({tlsKey:supplied.keys.tlsKey,grantKey:supplied.keys.grantKey,relayKey:supplied.keys.relayKey});
    const expected=parseBrowserRegistrationConfiguration(supplied.configuration);
    if(!same(candidate,expected))throw unavailable();
    if(new Set(Object.values(keys)).size!==3)throw unavailable();
    const p=expected.plan.configuration.profile,boundary={origin:p.origin,rpId:p.rpId,credentialId:new Uint8Array([1]),grantKey:keys.grantKey};
    browserProfileBoundary({...boundary,tlsKey:keys.tlsKey});browserProfileBoundary({...boundary,tlsKey:keys.relayKey});
    // 原 entry/installer 继续证明公私钥对应，reader 只保留原 handles 与既有 key policy。
    return Object.freeze({keys,approvedPlan:expected.plan,artifacts:expected.artifacts});
   }).catch(()=>{throw unavailable();});
   void original.catch(()=>{});
  }
  const approved=await original;
  if(!(signal instanceof AbortSignal)||signal.aborted||firstSignal!.aborted)throw unavailable();
  return approved;
 };
}
