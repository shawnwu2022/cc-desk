/** 独立 TEST_ONLY authenticator 与只读计数；正式 provider、store 和 presenter 不被替换。 */
export function productAuthenticatorScript(credential:JsonWebKey){return `
 // 只在 fixture 的安全文档注入，避免新页面 about:blank 没有 SubtleCrypto 时抛错。
 if(location.origin==='https://desk.invalid'){
 const observed={assertions:0,imports:0,signatures:0,databaseOpens:0};Object.defineProperty(globalThis,'__ccDeskTestOnlyProduct',{value:observed});
 const sign=crypto.subtle.sign.bind(crypto.subtle),load=crypto.subtle.importKey.bind(crypto.subtle),open=indexedDB.open.bind(indexedDB);
 Object.defineProperty(crypto.subtle,'sign',{configurable:true,value:(...args)=>{observed.signatures++;return sign(...args)}});
 Object.defineProperty(crypto.subtle,'importKey',{configurable:true,value:(...args)=>{observed.imports++;return load(...args)}});
 Object.defineProperty(indexedDB,'open',{configurable:true,value:(...args)=>{observed.databaseOpens++;return open(...args)}});
 const encode=b=>btoa(String.fromCharCode(...b)).replace(/\\+/g,'-').replace(/\\//g,'_').replace(/=+$/,'');
 const der=raw=>{const part=b=>{let i=0;while(i<31&&b[i]===0)i++;let v=b.slice(i);if(v[0]&128)v=new Uint8Array([0,...v]);return new Uint8Array([2,v.length,...v])};const a=part(raw.slice(0,32)),b=part(raw.slice(32));return new Uint8Array([48,a.length+b.length,...a,...b])};
 Object.defineProperty(navigator.credentials,'get',{configurable:true,value:async request=>{
  observed.assertions++;const key=await crypto.subtle.importKey('jwk',${JSON.stringify(credential)},{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
  const client=new TextEncoder().encode(JSON.stringify({type:'webauthn.get',challenge:encode(new Uint8Array(request.publicKey.challenge)),origin:location.origin,crossOrigin:false}));
  const hash=async bytes=>new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),auth=new Uint8Array([...(await hash(new TextEncoder().encode('desk.invalid'))),5,0,0,0,0]);
  const signature=der(new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},key,new Uint8Array([...auth,...(await hash(client))]))));
  const response=Object.create(AuthenticatorAssertionResponse.prototype);Object.defineProperties(response,{authenticatorData:{value:auth.buffer},clientDataJSON:{value:client.buffer},signature:{value:signature.buffer}});
  const value=Object.create(PublicKeyCredential.prototype);Object.defineProperties(value,{type:{value:'public-key'},rawId:{value:new Uint8Array(32).fill(7).buffer},response:{value:response}});return value;
 }});
 const original=MessagePort.prototype.start,seen=new WeakSet();Object.defineProperty(MessagePort.prototype,'start',{value:function(){if(!seen.has(this)){seen.add(this);this.addEventListener('message',event=>{if(event.data?.kind==='descriptor')Reflect.set(globalThis,'__ccDeskTestOnlyRunDescriptor',event.data.value)})}return original.call(this)}});
 }
`}
