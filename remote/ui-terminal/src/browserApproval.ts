/** Controlled HTTPS origin is a code/key-use trust boundary. No enrollment or key generation. */
export type BrowserProfile = Readonly<{
 origin:string;rpId:string;credentialId:Uint8Array;tlsKey:CryptoKey;grantKey:CryptoKey;
}>;
function privateKey(value:unknown):value is CryptoKey{
 if(typeof CryptoKey==='undefined'||!(value instanceof CryptoKey))return false;
 const key=value as CryptoKey,algorithm=key.algorithm as EcKeyAlgorithm|undefined;
 return key.type==='private'&&key.extractable===false&&algorithm?.name==='ECDSA'&&algorithm.namedCurve==='P-256'&&key.usages.length===1&&key.usages[0]==='sign';
}
export function browserProfileBoundary(value:unknown):BrowserProfile{
 if(!value||typeof value!=='object')throw Error('browser enrollment unavailable');
 const p=value as BrowserProfile;
 let url:URL;try{url=new URL(p.origin);}catch{throw Error('browser origin');}
 if(url.protocol!=='https:'||url.origin!==p.origin||url.pathname!=='/'||url.search||url.hash||url.username||url.password||
  !p.rpId||!(url.hostname===p.rpId||url.hostname.endsWith(`.${p.rpId}`))||
  !(p.credentialId instanceof Uint8Array)||p.credentialId.length<1||p.credentialId.length>1024||
  !privateKey(p.tlsKey)||!privateKey(p.grantKey)||p.tlsKey===p.grantKey)throw Error('browser profile policy');
 return Object.freeze({...p,credentialId:p.credentialId.slice()});
}
function encode(bytes:Uint8Array):string{return btoa(Array.from(bytes,b=>String.fromCharCode(b)).join('')).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');}
export type BrowserGrantRequest=Readonly<{
 /** Supplied by the verified Rust management request, including nonce/binding/profile identity. */
 challenge:Uint8Array;tbs:Uint8Array;
 /** Rust completes the original COSE candidate; a callback never establishes channel authority. */
 completeCose(signature:Uint8Array):Uint8Array;
}>;
/** Called only after displaying the verified grant semantics on the enrolled origin. */
export async function produceBrowserApproval(profile:BrowserProfile,request:BrowserGrantRequest,signal:AbortSignal):Promise<Uint8Array>{
 const p=browserProfileBoundary(profile);
 if(!globalThis.isSecureContext||globalThis.location?.origin!==p.origin||globalThis.top!==globalThis.self||
  !globalThis.crypto?.subtle||!globalThis.navigator?.credentials||signal.aborted)throw Error('browser approval unavailable');
 if(!(request.challenge instanceof Uint8Array)||request.challenge.length!==32||!(request.tbs instanceof Uint8Array)||request.tbs.length<1||request.tbs.length>10000)throw Error('browser request bounds');
 const challenge=request.challenge.slice(),tbs=request.tbs.slice();
 const fixed=new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},p.grantKey,tbs));
 if(signal.aborted||location.origin!==p.origin||fixed.length!==64)throw Error('browser approval cancelled');
 const cose=request.completeCose(fixed);
 if(!(cose instanceof Uint8Array)||cose.length>8192)throw Error('browser COSE bounds');
 const result=await navigator.credentials.get({signal,publicKey:{challenge,rpId:p.rpId,allowCredentials:[{type:'public-key',id:p.credentialId.slice()}],userVerification:'required',timeout:30000}});
 if(signal.aborted||location.origin!==p.origin||!(result instanceof PublicKeyCredential)||result.type!=='public-key')throw Error('browser assertion unavailable');
 const response=result.response;
 if(!(response instanceof AuthenticatorAssertionResponse)||encode(new Uint8Array(result.rawId))!==encode(p.credentialId))throw Error('browser credential mismatch');
 const auth=new Uint8Array(response.authenticatorData),client=new Uint8Array(response.clientDataJSON),signature=new Uint8Array(response.signature);
 if(auth.length!==37||client.length>2048||signature.length>80)throw Error('browser assertion bounds');
 const bytes=new TextEncoder().encode(JSON.stringify({version:1,cose:encode(cose),credentialId:encode(p.credentialId),authenticatorData:encode(auth),clientDataJson:encode(client),signature:encode(signature)}));
 if(bytes.length>16384)throw Error('browser approval bounds');
 return bytes;
}
