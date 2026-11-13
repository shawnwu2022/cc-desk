import {browserProfileBoundary,type BrowserProfile} from './browserApproval.ts';
import {checkedBrowserEnrollmentLease,type BrowserEnrollmentLease} from './browserEnrollmentLease.ts';
import type {BrowserGrantConfiguration} from './browserGrant.ts';
import type {BrowserClientTrust} from './browserTlsWorker.ts';
import type {BrowserRelayIdentity,BrowserRelayMode} from './browserRelay.ts';
export type ApprovedBrowserInputs=Readonly<{
 enrollment?:BrowserEnrollmentLease;
 profile:BrowserProfile;localSpki:Uint8Array;hostSpki:Uint8Array;trust:BrowserClientTrust;
 grant:BrowserGrantConfiguration;
 relay:Readonly<{mode?:BrowserRelayMode;endpoint:string;tunnel:Uint8Array;identity:BrowserRelayIdentity}>;
 artifacts:Readonly<{wasm:string;glue:string}>;
 present:(verified:Readonly<Record<string,unknown>>,signal:AbortSignal)=>Promise<void>;
}>;
/** The caller supplies approved handles/metadata. This boundary never enrolls. */
export function approvedBrowserInputs(value:ApprovedBrowserInputs):ApprovedBrowserInputs{
 const exact=(v:unknown,keys:string[])=>!!v&&typeof v==='object'&&Object.keys(v).length===keys.length&&keys.every(k=>Object.prototype.hasOwnProperty.call(v,k));
 const bytes=(v:unknown,n:number)=>v instanceof Uint8Array&&v.length===n;
 const positive=(v:unknown)=>typeof v==='string'&&/^[1-9][0-9]{0,19}$/.test(v)&&BigInt(v)<=18446744073709551615n;
 const lifetime=value&&Object.getOwnPropertyDescriptor(value,'enrollment');
 if(value&&'enrollment'in value&&!lifetime)throw Error('approved browser inputs unavailable');
 if(lifetime&&(!('value'in lifetime)||!lifetime.enumerable))throw Error('approved browser inputs unavailable');
 const enrollment=lifetime?checkedBrowserEnrollmentLease(lifetime.value):undefined;
 if(enrollment?.signal.aborted||!exact(value,['profile','localSpki','hostSpki','trust','grant','relay','artifacts','present',...(lifetime?['enrollment']:[])]))throw Error('approved browser inputs unavailable');
 if(!exact(value.profile,['origin','rpId','credentialId','tlsKey','grantKey']))throw Error('approved browser profile');
 const p=browserProfileBoundary(value.profile),g=value.grant,r=value.relay,a=value.artifacts;
 // 模式必须是自有数据字段，不能在批准快照中被继承属性或 getter 改写。
 const mode=Object.getOwnPropertyDescriptor(r,'mode');
 if(('mode' in r&&!mode)||(mode&&(!('value' in mode)||!['legacy','rendezvous-v1'].includes(mode.value))))throw Error('approved relay mode');
 if(!bytes(value.localSpki,91)||!bytes(value.hostSpki,91)||!exact(value.trust,['hostKeyEpoch','clientKeyEpoch','origin'])||value.trust.origin!==r.endpoint||!positive(value.trust.hostKeyEpoch)||!positive(value.trust.clientKeyEpoch)||!exact(r,Object.prototype.hasOwnProperty.call(r,'mode')?['endpoint','tunnel','identity','mode']:['endpoint','tunnel','identity'])||!bytes(r.tunnel,16)||!r.tunnel.some(Boolean)||!exact(r.identity,['keyId','sign'])||!bytes(r.identity.keyId,16)||typeof r.identity.sign!=='function'||typeof value.present!=='function')throw Error('approved browser identity');
 const endpoint=new URL(r.endpoint);
 if(r.endpoint.length>512||!/^[\x00-\x7f]+$/.test(r.endpoint)||r.endpoint.includes('%')||r.endpoint.includes('\\')||endpoint.href!==r.endpoint||endpoint.protocol!=='wss:'||endpoint.username||endpoint.password||endpoint.search||endpoint.hash||endpoint.port==='0')throw Error('approved relay endpoint');
 if(!exact(a,['wasm','glue']))throw Error('approved browser artifacts');
 for(const address of [a.wasm,a.glue]){const url=new URL(address);if(url.href!==address||url.origin!==p.origin||url.username||url.password||url.search||url.hash)throw Error('approved browser artifact origin');}
 if(!exact(g,['issuerSpki','principal','scope','role','budgetMs','peerInstall','profile'])||!bytes(g.issuerSpki,91)||typeof g.principal!=='string'||!/^[0-9a-f]{64}$/.test(g.principal)||!exact(g.scope,['session','run','generation'])||!/^[0-9a-f]{32}$/.test(g.scope.session)||typeof g.scope.run!=='string'||!g.scope.run||g.scope.run.length>128||!positive(g.scope.generation)||!['Controller','Observer'].includes(g.role)||!Number.isInteger(g.budgetMs)||g.budgetMs<1||g.budgetMs>30000||!exact(g.peerInstall,['budgetMs'])||!Number.isInteger(g.peerInstall?.budgetMs)||g.peerInstall!.budgetMs<1||g.peerInstall!.budgetMs>10000||!exact(g.profile,['origin','rpId','credentialId','credentialSpki','backupEligible','allowBackedUp'])||g.profile.origin!==p.origin||g.profile.rpId!==p.rpId||!(g.profile.credentialId instanceof Uint8Array)||g.profile.credentialId.length!==p.credentialId.length||!g.profile.credentialId.every((b,i)=>b===p.credentialId[i])||!bytes(g.profile.credentialSpki,91)||typeof g.profile.backupEligible!=='boolean'||typeof g.profile.allowBackedUp!=='boolean')throw Error('approved browser grant');
 // Byte fields are copy-on-read. Object.freeze alone cannot freeze typed arrays.
 const immutable=<T>(source:T):T=>{if(source===enrollment)return source;if(source instanceof Uint8Array)return source.slice() as T;if(source&&typeof source==='object'&&!(source instanceof CryptoKey)){const result:Record<string,unknown>={};for(const [name,v]of Object.entries(source)){const child=immutable(v);if(child instanceof Uint8Array)Object.defineProperty(result,name,{enumerable:true,get:()=>child.slice()});else result[name]=child;}return Object.freeze(result) as T;}return source;};
 return immutable({...value,profile:p,relay:{endpoint:r.endpoint,tunnel:r.tunnel,identity:r.identity,...(mode?{mode:mode.value}:{})},...(enrollment?{enrollment}:{})});
}
