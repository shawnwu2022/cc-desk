import {test} from 'node:test';
import assert from 'node:assert/strict';
import {browserProfileBoundary,produceBrowserApproval} from './browserApproval.ts';

test('BrowserProfile_MissingEnrollmentAndUnsafeOrigin_001',()=>{
 assert.throws(()=>browserProfileBoundary(undefined));
 assert.throws(()=>browserProfileBoundary({origin:'http://desk.invalid',rpId:'desk.invalid'}));
});

test('BrowserProfile_PublicFixtureNonExportableKeys_002',async()=>{
 // PUBLIC TEST_ONLY P256 scalar1, never enroll. No key generation occurs.
 const b64=(hex:string)=>Buffer.from(hex,'hex').toString('base64url');
 const jwk={kty:'EC',crv:'P-256',x:b64('6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296'),y:b64('4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5'),d:b64('00'.repeat(31)+'01')};
 const importKey=(extractable=false)=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},extractable,['sign']);
 const tlsKey=await importKey(),grantKey=await importKey();
 const p={origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:new Uint8Array([7]),tlsKey,grantKey};
 assert.equal(browserProfileBoundary(p).grantKey,grantKey);
 assert.throws(()=>browserProfileBoundary({...p,grantKey:tlsKey}));
 assert.throws(()=>browserProfileBoundary({...p,grantKey:{...grantKey,type:'private'}}));
 const extractable=await importKey(true);
 assert.throws(()=>browserProfileBoundary({...p,grantKey:extractable}));
 await assert.rejects(produceBrowserApproval(p,{challenge:new Uint8Array(32),tbs:new Uint8Array([1]),completeCose:()=>new Uint8Array()},new AbortController().signal));
 const saved=new Map<string,PropertyDescriptor|undefined>();
 const set=(name:string,value:unknown)=>{saved.set(name,Object.getOwnPropertyDescriptor(globalThis,name));Object.defineProperty(globalThis,name,{configurable:true,value});};
 class FixtureAssertion {authenticatorData=new ArrayBuffer(37);clientDataJSON=new TextEncoder().encode('{}').buffer;signature=new Uint8Array([1,2]).buffer;}
 class FixtureCredential {type='public-key';rawId=new Uint8Array([7]).buffer;response=new FixtureAssertion();}
 let options:CredentialRequestOptions|undefined,signature:Uint8Array|undefined;
 try{
  set('isSecureContext',true);set('location',{origin:p.origin});
  set('PublicKeyCredential',FixtureCredential);set('AuthenticatorAssertionResponse',FixtureAssertion);
  set('navigator',{credentials:{get:async(value:CredentialRequestOptions)=>{options=value;return new FixtureCredential();}}});
  const challenge=new Uint8Array(32).fill(8),tbs=new Uint8Array([1,2,3]);
  const encoded=await produceBrowserApproval(p,{challenge,tbs,completeCose(value){signature=value;return new Uint8Array([9]);}},new AbortController().signal);
  assert.equal(options?.publicKey?.rpId,p.rpId);assert.equal(options?.publicKey?.userVerification,'required');
  assert.deepEqual(new Uint8Array(options!.publicKey!.challenge as ArrayBuffer),challenge);
  const publicKey=await crypto.subtle.importKey('jwk',{kty:jwk.kty,crv:jwk.crv,x:jwk.x,y:jwk.y},{name:'ECDSA',namedCurve:'P-256'},false,['verify']);
  assert.equal(await crypto.subtle.verify({name:'ECDSA',hash:'SHA-256'},publicKey,signature!,tbs),true);
  const envelope=JSON.parse(new TextDecoder().decode(encoded));
  assert.equal(envelope.version,1);assert.equal(envelope.cose,'CQ');assert.equal(envelope.credentialId,'Bw');
 }finally{for(const [name,descriptor]of saved){if(descriptor)Object.defineProperty(globalThis,name,descriptor);else Reflect.deleteProperty(globalThis,name);}}
});
