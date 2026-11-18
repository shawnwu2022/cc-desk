import {parseJson} from './wire.ts';
import {browserProfileBoundary} from './browserApproval.ts';
import {createBrowserEnrollmentLease} from './browserEnrollmentLease.ts';

type Keys=Readonly<{tlsKey:CryptoKey;grantKey:CryptoKey;relayKey:CryptoKey}>;
const unavailable=()=>Error('browser registration unavailable');
const encoder=new TextEncoder();
const encode=(b:Uint8Array)=>btoa(Array.from(b,n=>String.fromCharCode(n)).join('')).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
function exact(v:unknown,keys:string[]):asserts v is Record<string,any>{
 if(!v||typeof v!=='object'||Reflect.ownKeys(v).length!==keys.length)throw unavailable();
 for(const name of keys){const f=Object.getOwnPropertyDescriptor(v,name);if(!f?.enumerable||!('value'in f))throw unavailable();}
}
function decode(s:unknown,min:number,max:number):Uint8Array<ArrayBuffer>{
 if(typeof s!=='string'||s.length>Math.ceil(max*4/3)||!/^[A-Za-z0-9_-]+$/.test(s))throw unavailable();
 const bytes=Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
 if(bytes.length<min||bytes.length>max||encode(bytes)!==s)throw unavailable();return bytes;
}
function copy(value:ArrayBuffer,min:number,max:number):Uint8Array<ArrayBuffer>{
 if(!(value instanceof ArrayBuffer)||value.byteLength<min||value.byteLength>max)throw unavailable();return new Uint8Array(value).slice();
}
function request(bytes:Uint8Array,creating:boolean,origin:string){
 if(!(bytes instanceof Uint8Array))throw unavailable();const v=parseJson(bytes.slice(),4096);
 exact(v,creating?['version','origin','rpId','userHandle','challenge','tlsSpki','grantSpki','relaySpki']:['version','origin','rpId','credentialId','challenge']);
 const host=new URL(origin).hostname;
 if(v.version!==1||v.origin!==origin||typeof v.rpId!=='string'||!v.rpId||v.rpId.length>253||!(host===v.rpId||host.endsWith(`.${v.rpId}`)))throw unavailable();
 const challenge=decode(v.challenge,32,32);
 return{origin,rpId:v.rpId as string,challenge,credential:decode(creating?v.userHandle:v.credentialId,1,creating?64:1024),spkis:creating?[decode(v.tlsSpki,91,91),decode(v.grantSpki,91,91),decode(v.relaySpki,91,91)]:[]};
}
async function hash(domain:string,parts:Uint8Array[]){
 const size=encoder.encode(domain).length+parts.reduce((n,p)=>n+4+p.length,0),data=new Uint8Array(size);let at=0;
 const prefix=encoder.encode(domain);data.set(prefix);at+=prefix.length;
 for(const p of parts){new DataView(data.buffer).setUint32(at,p.length);at+=4;data.set(p,at);at+=p.length;}
 return new Uint8Array(await crypto.subtle.digest('SHA-256',data));
}

/** 仅供独立可信 owner 显式调用；不生成 TLS/grant/relay 密钥，也不批准或保存登记。
 * 实际 WebAuthn create 会创建认证器凭证，调用前必须另行取得具体授权。 */
export function createBrowserRegistrationProducer(origin:string,signal:AbortSignal,deadline:number){
 const page=document,url=new URL(origin),now=()=>performance.timeOrigin+performance.now();
 if(typeof Window==='undefined'||!(globalThis instanceof Window)||!(signal instanceof AbortSignal)||url.protocol!=='https:'||url.origin!==origin||!Number.isFinite(deadline)||deadline<=now()||deadline-now()>120000)throw unavailable();
 const lifetime=createBrowserEnrollmentLease();let state:'new'|'creating'|'created'|'confirming'|'closed'='new',credential:string|undefined,rpId:string|undefined,timer:ReturnType<typeof setTimeout>|undefined;
 const close=()=>{state='closed';if(timer!==undefined)clearTimeout(timer);lifetime.revoke();};
 const eligible=()=>document===page&&location.origin===origin&&globalThis.top===globalThis.self&&globalThis.isSecureContext&&globalThis.crossOriginIsolated&&page.visibilityState==='visible'&&!signal.aborted&&!lifetime.lease.signal.aborted&&now()<deadline;
 const live=()=>{if(!eligible())throw unavailable();};
 const visibility=()=>{if(page.visibilityState!=='visible')close();};
 signal.addEventListener('abort',close,{once:true});page.addEventListener('visibilitychange',visibility);page.addEventListener('freeze',close);globalThis.addEventListener('pagehide',close);
 const cleanup=()=>{signal.removeEventListener('abort',close);page.removeEventListener('visibilitychange',visibility);page.removeEventListener('freeze',close);globalThis.removeEventListener('pagehide',close);};
 void lifetime.waitClosed().then(cleanup,cleanup);
 if(!eligible()){close();throw unavailable();}
 const expire=()=>{if(state==='closed')return;if(now()>=deadline){close();return;}timer=setTimeout(expire,Math.max(1,Math.ceil(deadline-now())));};expire();
 function start(expected:typeof state,next:typeof state,operation:()=>Promise<Uint8Array>):Promise<Uint8Array>{
  if(state!==expected){close();return Promise.reject(unavailable());}state=next;
  const work=Promise.resolve().then(async()=>{live();return operation();}).catch(()=>{close();throw unavailable();});
  // 原平台 promise 真正结束之前保留其所有权，不以 abort/timeout 冒充完成。
  lifetime.lease.retain(work.then(()=>{},()=>{}));return work;
 }
 async function signature(key:CryptoKey,message:Uint8Array){live();const result=new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},key,new Uint8Array(message)));live();if(result.length!==64)throw unavailable();return result;}
 function create(bytes:Uint8Array,keys:Keys):Promise<Uint8Array>{
  let r:ReturnType<typeof request>,privateKeys:CryptoKey[];
  try{
   r=request(bytes,true,origin);exact(keys,['tlsKey','grantKey','relayKey']);privateKeys=[keys.tlsKey,keys.grantKey,keys.relayKey];
   if(new Set(privateKeys).size!==3)throw unavailable();
   browserProfileBoundary({origin,rpId:r.rpId,credentialId:new Uint8Array([1]),tlsKey:privateKeys[0],grantKey:privateKeys[1]});browserProfileBoundary({origin,rpId:r.rpId,credentialId:new Uint8Array([1]),tlsKey:privateKeys[2],grantKey:privateKeys[1]});
  }catch{close();return Promise.reject(unavailable());}
  return start('new','creating',async()=>{
   const purposes=['tls','grant','relay'];
   for(let i=0;i<3;i++){
    live();const publicKey=await crypto.subtle.importKey('spki',r.spkis[i],{name:'ECDSA',namedCurve:'P-256'},false,['verify']);
    const message=new Uint8Array([...encoder.encode(`CC-DESK-REGISTRATION-${purposes[i]}-PREFLIGHT-v1\0`),...r.challenge]);
    const proof=await signature(privateKeys[i],message);if(!await crypto.subtle.verify({name:'ECDSA',hash:'SHA-256'},publicKey,proof,message))throw unavailable();
   }
   live();const value=await navigator.credentials.create({signal:lifetime.lease.signal,publicKey:{challenge:r.challenge.slice(),rp:{id:r.rpId,name:'CC Desk'},user:{id:r.credential.slice(),name:'CC Desk enrollment',displayName:'CC Desk enrollment'},pubKeyCredParams:[{type:'public-key',alg:-7}],authenticatorSelection:{userVerification:'required',residentKey:'discouraged'},attestation:'none',timeout:Math.max(1,Math.floor(deadline-now()))}});
   live();if(!(value instanceof PublicKeyCredential)||value.type!=='public-key'||!(value.response instanceof AuthenticatorAttestationResponse)||Reflect.ownKeys(value.getClientExtensionResults()).length)throw unavailable();
   const id=copy(value.rawId,1,1024),client=copy(value.response.clientDataJSON,1,2048),attestation=copy(value.response.attestationObject,1,4096);
   const transcript=await hash('CC-DESK-REGISTRATION-CREATE-v1\0',[r.challenge,id,client,attestation]),proofs:Uint8Array[]=[];
   for(let i=0;i<3;i++)proofs.push(await signature(privateKeys[i],new Uint8Array([...encoder.encode(`CC-DESK-REGISTRATION-${purposes[i]}-POSSESSION-v1\0`),...transcript])));
   live();credential=encode(id);rpId=r.rpId;state='created';return encoder.encode(JSON.stringify({version:1,credentialId:credential,clientDataJson:encode(client),attestationObject:encode(attestation),tlsSignature:encode(proofs[0]),grantSignature:encode(proofs[1]),relaySignature:encode(proofs[2])}));
  });
 }
 function confirm(bytes:Uint8Array):Promise<Uint8Array>{
  let r:ReturnType<typeof request>;
  try{r=request(bytes,false,origin);if(encode(r.credential)!==credential||r.rpId!==rpId)throw unavailable();}catch{close();return Promise.reject(unavailable());}
  return start('created','confirming',async()=>{
   live();const value=await navigator.credentials.get({signal:lifetime.lease.signal,publicKey:{challenge:r.challenge.slice(),rpId:r.rpId,allowCredentials:[{type:'public-key',id:r.credential.slice()}],userVerification:'required',timeout:Math.max(1,Math.floor(deadline-now()))}});
   live();if(!(value instanceof PublicKeyCredential)||value.type!=='public-key'||!(value.response instanceof AuthenticatorAssertionResponse)||Reflect.ownKeys(value.getClientExtensionResults()).length)throw unavailable();
   const id=copy(value.rawId,1,1024),response=value.response;if(encode(id)!==credential)throw unavailable();
   const output=encoder.encode(JSON.stringify({version:1,credentialId:credential,authenticatorData:encode(copy(response.authenticatorData,37,37)),clientDataJson:encode(copy(response.clientDataJSON,1,2048)),signature:encode(copy(response.signature,1,80)),userHandle:response.userHandle===null?null:encode(copy(response.userHandle,1,64))}));
   live();close();return output;
  });
 }
 return Object.freeze({create,confirm,close,waitClosed:lifetime.waitClosed});
}
