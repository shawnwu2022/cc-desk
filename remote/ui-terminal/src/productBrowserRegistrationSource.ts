import {parseBrowserRegistrationConfiguration,type PublicBrowserRegistrationConfiguration} from './browserRegistrationProfile.ts';
import {BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME} from './browserProductStartup.ts';
import type {ApprovedBrowserRegistrationInputs} from './browserRegistrationEntry.ts';
let used=false;
const visible=(text:string)=>text.replace(/[\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f]/g,c=>'\\u'+c.charCodeAt(0).toString(16).padStart(4,'0'));
const encode=(bytes:ArrayBuffer)=>btoa(String.fromCharCode(...new Uint8Array(bytes))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
const unavailable=()=>Error('browser registration preparation unavailable');
/** 本地操作员选择不是账户认证，也不证明 Host 身份。 */
export async function readApprovedBrowserRegistrationInputs(signal:AbortSignal):Promise<ApprovedBrowserRegistrationInputs>{
 if(used)throw unavailable();used=true;
 const page=document,body=page.body,origin=location.origin;
 const eligible=()=>document===page&&page.body===body&&top===self&&isSecureContext&&crossOriginIsolated&&location.origin===origin&&new URL(origin).protocol==='https:';
 if(!(signal instanceof AbortSignal)||signal.aborted||!eligible()||page.visibilityState!=='visible')throw unavailable();
 return new Promise((resolve,reject)=>{
  const dialog=page.createElement('dialog'),title=page.createElement('h2'),summary=page.createElement('p'),details=page.createElement('pre'),nativeLabel=page.createElement('p'),nativeDetails=page.createElement('pre'),input=page.createElement('textarea'),actions=page.createElement('div'),cancel=page.createElement('button'),next=page.createElement('button');
  let keys:ApprovedBrowserRegistrationInputs['keys']|undefined,publicKeys:string[]=[],publicHex:string[]=[],configuration:PublicBrowserRegistrationConfiguration|undefined;
  let phase:'identity'|'generating'|'configuration'|'review'='identity',settled=false,markup='',pending:Promise<void>|undefined;
  const live=()=>!settled&&!signal.aborted&&eligible()&&dialog.isConnected&&dialog.parentNode===body&&dialog.open&&dialog.outerHTML===markup;
  const observer=new MutationObserver(records=>{if(records.some(r=>r.target===dialog||dialog.contains(r.target))||!live())void finish();});
  function render(fn:()=>void){observer.disconnect();fn();markup=dialog.outerHTML;observer.observe(page,{subtree:true,childList:true,attributes:true,characterData:true});}
  async function finish(result?:ApprovedBrowserRegistrationInputs){
   if(settled)return;settled=true;observer.disconnect();
   signal.removeEventListener('abort',abort);page.removeEventListener('visibilitychange',hidden);page.removeEventListener('freeze',abort);globalThis.removeEventListener('pagehide',abort);globalThis.removeEventListener('freeze',abort);
   dialog.removeEventListener('cancel',denied);dialog.removeEventListener('close',abort);cancel.removeEventListener('click',denied);next.removeEventListener('click',advance);
   if(dialog.open)dialog.close();dialog.remove();
   await pending?.catch(()=>{});keys=undefined;configuration=undefined;publicKeys=[];publicHex=[];
   if(result)resolve(result);else reject(unavailable());
  }
  const abort=()=>{void finish();},denied=(event:Event)=>{event.preventDefault();abort();};
  const hidden=()=>{if(!eligible()||phase==='review'&&page.visibilityState!=='visible')abort();};
  async function generate(){
   const pairs:CryptoKeyPair[]=[];
   try{
    for(let i=0;i<3;i++){
     const pair=await crypto.subtle.generateKey({name:'ECDSA',namedCurve:'P-256'},false,['sign','verify']);
     if(settled||signal.aborted)throw unavailable();pairs.push(pair);
     const spki=await crypto.subtle.exportKey('spki',pair.publicKey);
     publicKeys.push(encode(spki));publicHex.push(Array.from(new Uint8Array(spki),byte=>byte.toString(16).padStart(2,'0')).join(''));
     if(settled||signal.aborted)throw unavailable();
    }
    keys=Object.freeze({tlsKey:pairs[0]!.privateKey,grantKey:pairs[1]!.privateKey,relayKey:pairs[2]!.privateKey});
    render(()=>{phase='configuration';details.textContent=JSON.stringify({tlsSpki:publicKeys[0],grantSpki:publicKeys[1],relaySpki:publicKeys[2]},null,2);nativeDetails.textContent=JSON.stringify({tlsSpki:publicHex[0],grantSpki:publicHex[1],relaySpki:publicHex[2]},null,2);nativeLabel.hidden=false;nativeDetails.hidden=false;input.hidden=false;next.disabled=false;next.textContent='Review public configuration';summary.textContent='The first block is Base64URL for Browser configuration. The second block represents the same public identities in complete SPKI hex for separately authorized Native registration configuration. Paste full public Browser registration JSON below. Private keys never leave page memory. Five-minute preparation expiry includes time away.';});
   }finally{pairs.length=0;}
  }
  function advance(event:Event){
   if(!event.isTrusted||!live()||page.visibilityState!=='visible')return;
   if(phase==='identity'){
    render(()=>{phase='generating';next.disabled=true;summary.textContent='Generating three ephemeral non-exportable signing keys in this page only.';});
    pending=generate();void pending.catch(abort);return;
   }
   if(phase==='configuration'){
    try{
     const parsed=parseBrowserRegistrationConfiguration(new TextEncoder().encode(input.value)),p=parsed.plan,c=p.configuration;
     if(p.databaseName!==BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME||c.profile.origin!==origin||c.localSpki!==publicKeys[0]||c.grant.issuerSpki!==publicKeys[1]||p.relaySpki!==publicKeys[2])throw unavailable();
     configuration=parsed;
     render(()=>{phase='review';input.remove();nativeLabel.remove();nativeDetails.remove();title.textContent='Review complete registration';summary.textContent='Independently verify the exact Host pin below: plan.configuration.hostSpki is the full SPKI Base64URL public key, not a hash fingerprint. Local approval selects this user association, scope, role, budgets, relay, both runtime artifact pairs and browser storage target. It does not authenticate an account or prove Host identity. Host key proofs, device verification, native approval and storage approval remain separate. Approval lasts 60 seconds while visible.';details.textContent=visible(JSON.stringify(parsed,null,2));next.textContent='Approve this exact registration';});
    }catch{abort();}return;
   }
   if(phase==='review'&&configuration&&keys)void finish(Object.freeze({keys,approvedPlan:configuration.plan,artifacts:configuration.artifacts}));
  }
  try{
   dialog.className='browser-grant-dialog';title.textContent='Prepare browser identity';summary.textContent='Generate three distinct ephemeral P-256 signing keys for TLS, grants and relay. Nothing is stored or sent. Reload or cancellation discards this identity; no automatic retry.';
   nativeLabel.hidden=true;nativeDetails.hidden=true;nativeLabel.textContent='Native registration SPKI hex: use these as browser.tlsSpki, browser.grantSpki and browser.relaySpki. These are complete 91-byte SPKIs, not the 65-byte SEC1 points required by Relay policy public_key_sec1. This is a public encoding conversion, not identity verification or approval. Do not paste hex into Browser configuration.';nativeDetails.setAttribute('aria-label','Native registration public identities (SPKI hex)');
   input.hidden=true;input.maxLength=16384;input.setAttribute('aria-label','Public registration configuration JSON');details.setAttribute('aria-label','Complete registration identity and configuration');cancel.type='button';cancel.textContent='Cancel';next.type='button';next.textContent='Generate ephemeral identity';actions.className='browser-grant-actions';actions.append(cancel,next);dialog.append(title,summary,details,nativeLabel,nativeDetails,input,actions);
   cancel.addEventListener('click',denied);next.addEventListener('click',advance);dialog.addEventListener('cancel',denied);dialog.addEventListener('close',abort);signal.addEventListener('abort',abort,{once:true});page.addEventListener('visibilitychange',hidden);page.addEventListener('freeze',abort);globalThis.addEventListener('pagehide',abort);globalThis.addEventListener('freeze',abort);
   body.append(dialog);dialog.showModal();cancel.focus();render(()=>{});
  }catch{abort();}
 });
}
