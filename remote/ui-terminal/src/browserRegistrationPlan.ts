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
const unavailable=()=>Error('browser registration installation unavailable');
function field(value:object,name:string):any{
 const item=Object.getOwnPropertyDescriptor(value,name);
 if(!item?.enumerable||!('value'in item))throw unavailable();return item.value;
}
function exact(value:unknown,names:readonly string[]):asserts value is Record<string,any>{
 if(!value||typeof value!=='object'||Array.isArray(value)||Reflect.ownKeys(value).length!==names.length)throw unavailable();
 for(const name of names)field(value,name);
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
 return names.length===Object.keys(right).length&&names.every(name=>Object.prototype.hasOwnProperty.call(right,name)&&same(left[name],right[name]));
}
function decode(value:unknown,min:number,max:number):void{
 if(typeof value!=='string'||value.length>Math.ceil(max*4/3)||!/^[A-Za-z0-9_-]+$/.test(value))throw unavailable();
 const result=Uint8Array.from(atob(value.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
 const encoded=btoa(Array.from(result,n=>String.fromCharCode(n)).join('')).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
 if(result.length<min||result.length>max||encoded!==value)throw unavailable();
}

/** 共享安装计划语法快照，不建立批准、真实身份或公私钥对应关系。 */
export function snapshotBrowserRegistrationPlan(value:unknown):BrowserRegistrationInstallationPlan{
 try{
  const approved=snapshot(value);exact(approved,['databaseName','expectedRevision','configuration','relaySpki','requestedUserHandle']);
  if(typeof approved.databaseName!=='string'||!/^[A-Za-z0-9._-]{1,64}$/.test(approved.databaseName))throw unavailable();
  if(approved.expectedRevision!==null)counter(approved.expectedRevision);
  decode(approved.relaySpki,91,91);decode(approved.requestedUserHandle,1,64);
  const c=approved.configuration;exact(c,['version','profile','localSpki','hostSpki','trust','grant','relay','artifacts']);
  exact(c.profile,['origin','rpId']);exact(c.grant,['issuerSpki','principal','scope','role','budgetMs','peerInstall','profile']);
  exact(c.grant.profile,['origin','rpId','allowBackedUp']);
  // 仅为复用公开语法校验补占位字段；它们不返回、不安装，也不产生凭证授权。
  const parsed=parseBrowserPairingConfiguration(new TextEncoder().encode(JSON.stringify({...c,profile:{...c.profile,credentialId:'AQ'},grant:{...c.grant,profile:{...c.grant.profile,credentialId:'AQ',credentialSpki:c.localSpki,backupEligible:false}}})));
  const {credentialId:_id,...profile}=parsed.profile;
  const {credentialId:_grantId,credentialSpki:_spki,backupEligible:_backup,...grantProfile}=parsed.grant.profile;
  if(!same({...parsed,profile,grant:{...parsed.grant,profile:grantProfile}},c)||new Set([c.localSpki,c.grant.issuerSpki,approved.relaySpki]).size!==3)throw unavailable();
  return approved as BrowserRegistrationInstallationPlan;
 }catch{throw unavailable();}
}
