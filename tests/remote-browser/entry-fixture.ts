import {spawn} from 'node:child_process';import {once} from 'node:events';import {createECDH,createHash} from 'node:crypto';import {readFile,mkdir,mkdtemp,writeFile,rename} from 'node:fs/promises';import {createServer} from 'node:net';import assert from 'node:assert/strict';import {resolve,sep,extname} from 'node:path';
// @ts-expect-error The existing locked transitive ws fixture has no declaration package.
import WebSocket from 'ws';
import type {BrowserContext} from '@playwright/test';
import {build} from 'esbuild';
import type {PublicBrowserPairingConfiguration} from '../../remote/ui-terminal/src/browserPairingProfile';
import {storedEntryModule} from './stored-entry-module';
import {productAuthenticatorScript} from './product-entry-authenticator';
export type PairingMutation='session'|'run'|'generation'|'hostSpki';
export type RunHandoffFixture='resume'|'exact-old'|'wrong-pin'|'wrong-principal'|'wrong-epoch'|'wrong-role';
function key(scalar:number|string){const d=typeof scalar==='number'?Buffer.alloc(32):Buffer.from(scalar,'hex');if(typeof scalar==='number')d[31]=scalar;const e=createECDH('prime256v1');e.setPrivateKey(d);const p=e.getPublicKey();return{jwk:{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},spki:Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),p]).toString('hex')};}
function processOwner(executable:string,args:string[],env:Record<string,string>){const child=spawn(executable,args,{env:{...process.env,...env},stdio:['pipe','pipe','pipe'],windowsHide:true});const exit=once(child,'exit');let output='';const waiters:{match:(v:any)=>boolean;resolve:(v:any)=>void;reject:(e:Error)=>void}[]=[];const values:any[]=[];let buffer='';child.stderr.on('data',b=>{output+=b});child.stdout.on('data',b=>{output+=b;buffer+=b;for(;;){const end=buffer.indexOf('\n');if(end<0)break;const line=buffer.slice(0,end).trim();buffer=buffer.slice(end+1);if(!line.startsWith('{')&&!line.startsWith('WSS_LISTEN='))continue;try{const value=line.startsWith('WSS_LISTEN=')?{listening:line.slice(11)}:JSON.parse(line);values.push(value);for(const waiter of [...waiters])if(waiter.match(value)){waiters.splice(waiters.indexOf(waiter),1);waiter.resolve(value);}}catch{}}});void exit.then(()=>{for(const waiter of waiters)waiter.reject(Error(output));});return{child,exit,output:()=>output,wait:(match:(v:any)=>boolean)=>{const value=values.find(match);if(value)return Promise.resolve(value);return new Promise<any>((resolve,reject)=>{waiters.push({match,resolve,reject});});},async dispose(){if(child.exitCode===null){child.kill();await exit;}}};}
export async function entryFixture(context:BrowserContext,mutation?:PairingMutation,productionPresenter=false,durableStore=false,runHandoff?:RunHandoffFixture,productProvider=false){
 const relayMode=process.env.CC_DESK_TEST_ONLY_ENTRY_RELAY_MODE??'legacy';if(relayMode!=='legacy'&&relayMode!=='rendezvous-v1')throw Error('explicit TEST_ONLY relay mode');
 const actualRelayExe=relayMode==='rendezvous-v1'?process.env.CC_DESK_TEST_ONLY_RENDEZVOUS_RELAY:undefined;if(relayMode==='rendezvous-v1'&&!actualRelayExe)throw Error('explicit TEST_ONLY actual rendezvous Relay required');
 const relayExe=process.env.CC_DESK_TEST_ONLY_ORIGINAL_RELAY_ORACLE,hostExe=process.env.CC_DESK_TEST_ONLY_DATA_HOST,wasm=process.env.CC_DESK_TEST_ONLY_DATA_WASM,glue=process.env.CC_DESK_TEST_ONLY_DATA_GLUE;
 if(!relayExe||!hostExe||!wasm||!glue)throw Error('explicit TEST_ONLY original Relay/Host/WASM/glue required');
 const artifacts=await Promise.all([relayExe,hostExe,wasm,glue,...(actualRelayExe?[actualRelayExe]:[]),...(productProvider?[resolve('dist/browser-profile.js')]:[])].map(async file=>({file:resolve(file),sha256:createHash('sha256').update(await readFile(file)).digest('hex')})));
 const helper=(await build({stdin:{contents:"export {createApprovedBrowserProfileReader} from './remote/ui-terminal/src/browserPairingProfile.ts';export {createBrowserEnrollmentAuthority} from './remote/ui-terminal/src/browserEnrollment.ts';export {createBrowserGrantPresenter} from './remote/ui-terminal/src/browserGrantPresenter.ts';export {createStoredBrowserRunProfileReader} from './remote/ui-terminal/src/browserRunHandoff.ts';export {createBrowserEnrollmentStore,createStoredBrowserProfileReader} from './remote/ui-terminal/src/browserEnrollmentStore.ts';",resolveDir:process.cwd()},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text;
 let browserConnections=0,artifactRequests=0;
 const errors:string[]=[],connections:{requested:string[];protocol:string;authorized:boolean;tls:string|null;servername:string}[]=[];
 const recordError=(error:unknown)=>errors.push(error instanceof Error?error.message:String(error));
 context.on('page',page=>page.on('pageerror',recordError));for(const page of context.pages())page.on('pageerror',recordError);
 let host:ReturnType<typeof processOwner>|undefined,relay:ReturnType<typeof processOwner>|undefined,actualRelay:ReturnType<typeof processOwner>|undefined;
 const sockets=new Set<WebSocket>();let policyArtifact:{file:string;sha256:string}|undefined;
 try{
 let externalEndpoint:string|undefined;
 if(actualRelayExe){
  const listener=createServer();listener.listen(0,'127.0.0.1');await once(listener,'listening');const address=listener.address();if(!address||typeof address==='string')throw Error('TEST_ONLY loopback address');await new Promise<void>((resolve,reject)=>listener.close(error=>error?reject(error):resolve()));
  externalEndpoint=`wss://foobar.com:${address.port}/relay`;
  const now=Date.now(),policy={schema:1,policy_id:'PUBLIC_TEST_ONLY_PRODUCT_RENDEZVOUS',revision:1,issued_at_unix_ms:now,not_before_unix_ms:now,expires_unix_ms:now+60000,listen:`127.0.0.1:${address.port}`,signed_wss_endpoint:externalEndpoint,browser_origins:['https://desk.invalid'],devices:[{id:'06'.repeat(16),public_key_sec1:key(1).spki.slice(-130),role:'Host'},{id:'07'.repeat(16),public_key_sec1:key('c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721').spki.slice(-130),role:'Phone'}],routes:[{tunnel:'09'.repeat(16),host:'06'.repeat(16),phone:'07'.repeat(16)}]};
  const base=resolve('.audit/product-rendezvous/policies');await mkdir(base,{recursive:true});const directory=await mkdtemp(resolve(base,'public-test-only-')),file=resolve(directory,'policy.json');await writeFile(file,JSON.stringify(policy,null,2));policyArtifact={file,sha256:createHash('sha256').update(await readFile(file)).digest('hex')};artifacts.push(policyArtifact);
  actualRelay=processOwner(actualRelayExe,['--policy',file,'--tls-chain',resolve('remote/crates/transport/tests/fixtures/chain.pem'),'--tls-key',resolve('remote/crates/transport/tests/fixtures/end.key'),'--tls-ca',resolve('remote/crates/transport/tests/fixtures/root.pem')],{});await actualRelay.wait(v=>v.listening===policy.listen);
 }
 relay=processOwner(relayExe,[],{CC_DESK_TEST_ONLY_BROWSER_RELAY:'1',CC_DESK_TEST_ONLY_TLS_FIXTURES:resolve('remote/crates/transport/tests/fixtures'),...(externalEndpoint?{CC_DESK_TEST_ONLY_EXTERNAL_RELAY:externalEndpoint}:{})});
 const endpoint=await relay.wait(v=>typeof v.endpoint==='string');host=processOwner(hostExe,['--exact','browser_data_fixture_tests::BrowserData_OriginalHostFixture_001','--nocapture'],{CC_DESK_TEST_ONLY_DATA_MODE:'normal',CC_DESK_TEST_ONLY_DATA_RELAY_ORIGIN:endpoint.endpoint});const ready=await host.wait(v=>v.kind==='ready');relay.child.stdin.end(String(ready.port)+'\n');await relay.wait(v=>v.hostAdmitted===true);
 const issuer=key(1),tls=key(2),credential=key(3),relayKey=key('c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721');
 const b64=(hex:string)=>Buffer.from(hex,'hex').toString('base64url');
 // Independent fixture authority starts from the actual Host scope and fixed public vectors.
 // The candidate is only a separately cloned untrusted input, never a provider selector.
 const trusted:PublicBrowserPairingConfiguration={
  version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:Buffer.alloc(32,7).toString('base64url')},
  localSpki:b64(tls.spki),hostSpki:b64(ready.hostSpki),trust:{hostKeyEpoch:'9007199254740993',clientKeyEpoch:'18446744073709551615',origin:endpoint.endpoint},
  grant:{issuerSpki:b64(issuer.spki),principal:'04'.repeat(32),scope:ready.scope,role:'Controller',budgetMs:3000,peerInstall:{budgetMs:1000},
   profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:Buffer.alloc(32,7).toString('base64url'),credentialSpki:b64(credential.spki),backupEligible:false,allowBackedUp:false}},
  relay:{mode:relayMode,endpoint:endpoint.endpoint,tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,7).toString('base64url')},
  artifacts:{wasm:'https://desk.invalid/remote-runtime/fixture_bg.wasm',glue:'https://desk.invalid/remote-runtime/fixture_bg.js'},
 };
 if(runHandoff){
  if(!durableStore)throw Error('TEST_ONLY handoff requires original stored enrollment');
  // 只有旧的登记 scope 进入 IDB；本次 Host scope 绝不送到 profile provider。
  (trusted as {grant:PublicBrowserPairingConfiguration['grant']}).grant={...trusted.grant,scope:{session:'55'.repeat(16),run:'PUBLIC_TEST_ONLY_PREVIOUS_RUN',generation:'1'}};
  if(runHandoff==='wrong-pin')(trusted as {hostSpki:string}).hostSpki=b64(tls.spki);
  if(runHandoff==='wrong-principal')(trusted as {grant:PublicBrowserPairingConfiguration['grant']}).grant={...trusted.grant,principal:'09'.repeat(32)};
  if(runHandoff==='wrong-epoch')(trusted as {trust:PublicBrowserPairingConfiguration['trust']}).trust={...trusted.trust,hostKeyEpoch:'1'};
  if(runHandoff==='wrong-role')(trusted as {grant:PublicBrowserPairingConfiguration['grant']}).grant={...trusted.grant,role:'Observer'};
 }
 const candidate=structuredClone(trusted) as {grant:{scope:{session:string;run:string;generation:string}};hostSpki:string};
 if(mutation==='session')candidate.grant.scope.session='ff'.repeat(16);
 if(mutation==='run')candidate.grant.scope.run='OTHER_PUBLIC_TEST_ONLY';
 if(mutation==='generation')candidate.grant.scope.generation='8';
 if(mutation==='hostSpki')candidate.hostSpki=b64(tls.spki);
 let profileModule=`// Public TEST_ONLY fixtures; never enrollment.
 import {createApprovedBrowserProfileReader,createBrowserEnrollmentAuthority,createBrowserGrantPresenter} from '/test-only-pairing-helper.mjs';
 const observations={sourceReads:0,providerReads:0,providerArgumentCount:0,providerSignalMatches:false,originalHandles:false,nonExportable:false,signatures:0,presentations:0,assertions:0};
 Object.defineProperty(globalThis,'__ccDeskTestOnlyPairing',{value:observations});
 const sign=crypto.subtle.sign.bind(crypto.subtle);Object.defineProperty(crypto.subtle,'sign',{configurable:true,value:(...args)=>{observations.signatures++;return sign(...args);}});
 const key=jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 const encode=b=>btoa(String.fromCharCode(...b)).replace(/\\+/g,'-').replace(/\\//g,'_').replace(/=+$/,'');
 const der=raw=>{const part=b=>{let i=0;while(i<31&&b[i]===0)i++;let v=b.slice(i);if(v[0]&128)v=new Uint8Array([0,...v]);return new Uint8Array([2,v.length,...v]);};const a=part(raw.slice(0,32)),b=part(raw.slice(32));return new Uint8Array([48,a.length+b.length,...a,...b]);};
 let originalHandles,sourceSignal;
 const authority=createBrowserEnrollmentAuthority();
 const productionPresenter=${productionPresenter}?createBrowserGrantPresenter(location.origin):undefined;
 const approval=authority.approve(async signal=>{
 const tlsKey=await key(${JSON.stringify(tls.jwk)}),grantKey=await key(${JSON.stringify(issuer.jwk)}),credentialKey=await key(${JSON.stringify(credential.jwk)}),relayKey=await key(${JSON.stringify(relayKey.jwk)});if(signal.aborted)throw Error('TEST_ONLY cancelled');
 Object.defineProperty(navigator.credentials,'get',{configurable:true,value:async request=>{observations.assertions++;const challenge=new Uint8Array(request.publicKey.challenge);const client=new TextEncoder().encode(JSON.stringify({type:'webauthn.get',challenge:encode(challenge),origin:location.origin,crossOrigin:false}));const hash=async bytes=>new Uint8Array(await crypto.subtle.digest('SHA-256',bytes));const auth=new Uint8Array([...(await hash(new TextEncoder().encode('desk.invalid'))),5,0,0,0,0]);const signature=der(new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},credentialKey,new Uint8Array([...auth,...(await hash(client))]))));const response=Object.create(AuthenticatorAssertionResponse.prototype);Object.defineProperties(response,{authenticatorData:{value:auth.buffer},clientDataJSON:{value:client.buffer},signature:{value:signature.buffer}});const value=Object.create(PublicKeyCredential.prototype);Object.defineProperties(value,{type:{value:'public-key'},rawId:{value:new Uint8Array(32).fill(7).buffer},response:{value:response}});return value;}});
 originalHandles={tlsKey,grantKey,relaySign:async(request,signal)=>{if(signal.aborted)throw Error('cancelled');return new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},relayKey,request.transcript));},present:async(verified,signal)=>{observations.presentations++;if(productionPresenter)return productionPresenter(verified,signal);const text=document.createElement('pre');text.textContent=JSON.stringify(verified);document.body.append(text);const button=document.createElement('button');button.textContent='Approve TEST_ONLY';document.body.append(button);try{if(globalThis.__ccDeskTestOnlyHoldPresentation){await new Promise(resolve=>{globalThis.__ccDeskTestOnlyReleasePresentation=resolve;});}else{await new Promise((resolve,reject)=>{button.onclick=()=>resolve();signal.addEventListener('abort',()=>reject(Error('cancelled')),{once:true});});}}finally{button.remove();text.remove();}}};
 return {configuration:new TextEncoder().encode(${JSON.stringify(JSON.stringify(trusted))}),handles:originalHandles};
 });void approval.catch(()=>{});
 const enrollment=globalThis.__ccDeskTestOnlyRevokeOnRetain?Object.freeze({signal:authority.provider.enrollment.signal,retain(work){authority.provider.enrollment.retain(work);authority.revoke();}}):authority.provider.enrollment;
 const reader=createApprovedBrowserProfileReader(new TextEncoder().encode(${JSON.stringify(JSON.stringify(candidate))}),{enrollment,async readApproved(...args){observations.providerReads++;observations.providerArgumentCount=args.length;observations.providerSignalMatches=args[0]===sourceSignal;return authority.provider.readApproved(...args);}});
 Object.defineProperty(globalThis,'__ccDeskTestOnlyEnrollment',{value:{revoke:authority.revoke,waitClosed:authority.waitClosed}});
 export async function readApprovedBrowserInputs(signal){observations.sourceReads++;sourceSignal=signal;const inputs=await reader(signal);if(inputs){observations.originalHandles=inputs.profile.tlsKey===originalHandles.tlsKey&&inputs.profile.grantKey===originalHandles.grantKey&&inputs.relay.identity.sign===originalHandles.relaySign&&inputs.present===originalHandles.present;observations.nonExportable=!inputs.profile.tlsKey.extractable&&!inputs.profile.grantKey.extractable;}return inputs;}
 `;
 const durable=durableStore?storedEntryModule(trusted,candidate,{tls:tls.jwk,grant:issuer.jwk,relay:relayKey.jwk,credential:credential.jwk},runHandoff?(runHandoff==='exact-old'?'exact':'resume'):undefined,productProvider?'CC_DESK_BROWSER_ENROLLMENT_V1':undefined):undefined;if(durable)profileModule=durable.profile;
 if(productProvider){if(!durableStore||!runHandoff||mutation)throw Error('TEST_ONLY product requires an independent installed record');await context.addInitScript({content:productAuthenticatorScript(credential.jwk)});}
 const root=resolve('dist');await context.route('https://desk.invalid/**',async route=>{const path=new URL(route.request().url()).pathname;const headers={'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp','Cache-Control':'no-store'};if(path==='/test-only-pairing-helper.mjs'){await route.fulfill({status:200,contentType:'text/javascript',headers,body:helper});return;}if(path==='/test-only-enrollment-install.mjs'&&durable){await route.fulfill({status:200,contentType:'text/javascript',headers,body:durable.install});return;}if(path==='/browser-profile.js'&&!productProvider){await route.fulfill({status:200,contentType:'text/javascript',headers,body:profileModule});return;}if(path==='/remote-runtime/fixture_bg.wasm'||path==='/remote-runtime/fixture_bg.js'){artifactRequests++;await route.fulfill({status:200,contentType:path.endsWith('wasm')?'application/wasm':'text/javascript',headers,body:await readFile(path.endsWith('wasm')?wasm:glue)});return;}const file=resolve(root,'.'+path);if(!file.startsWith(root+sep))throw Error('fixture path');await route.fulfill({status:200,contentType:({'.html':'text/html','.js':'text/javascript','.css':'text/css'} as Record<string,string>)[extname(file)]??'application/octet-stream',headers,body:await readFile(file)});});
 const ca=await readFile('remote/crates/transport/tests/fixtures/root.pem');await context.routeWebSocket(endpoint.endpoint,route=>{
  browserConnections++;let ws:WebSocket|undefined;
  const failClosed=(error:unknown)=>{recordError(error);try{ws?.terminate();}catch{}try{route.close();}catch{}};
  const guarded=(action:()=>void)=>{try{action();}catch(error){failClosed(error);}};
  guarded(()=>{
   const requested=route.protocols(),expected=relayMode==='rendezvous-v1'?['cc-desk-relay-rendezvous-v1']:[];assert.deepEqual(requested,expected,'product must request the approved relay protocol');
   const socket=new WebSocket(endpoint.endpoint,requested,{ca,rejectUnauthorized:true,minVersion:'TLSv1.3',maxVersion:'TLSv1.3',servername:'foobar.com',origin:'https://desk.invalid',lookup:(_name:string,_options:unknown,callback:(error:Error|null,addresses:{address:string;family:number}[])=>void)=>callback(null,[{address:'127.0.0.1',family:4}])});ws=socket;sockets.add(socket);
   socket.on('open',()=>guarded(()=>{const observation={requested,protocol:socket.protocol,authorized:socket._socket.authorized===true,tls:socket._socket.getProtocol(),servername:socket._socket.servername};connections.push(observation);assert.deepEqual(observation,{requested:expected,protocol:expected[0]??'',authorized:true,tls:'TLSv1.3',servername:'foobar.com'});}));
   route.onMessage(message=>guarded(()=>socket.send(message)));route.onClose(()=>guarded(()=>socket.close()));
   socket.on('message',(data:Buffer,binary:boolean)=>guarded(()=>{assert.equal(binary,true,'original Relay must send binary messages');route.send(Buffer.from(data));}));
   socket.on('close',()=>guarded(()=>{sockets.delete(socket);route.close();}));socket.on('error',failClosed);
  });
 });
 let serviceRetirement:{serviceStopped:true;connectionTasksJoined:true;exitCode:number;signal:null}|undefined;
 let retirement:Promise<void>|undefined;
 const retireRelay=()=>retirement??=Promise.resolve().then(async()=>{
  if(!actualRelay||!policyArtifact)return;
  // 只撤销本次公开 policy，保留原文件证据；launcher 正常取消并 join 连接任务。
  const retired=policyArtifact.file+'.retired';await rename(policyArtifact.file,retired);policyArtifact.file=retired;
  let timer:ReturnType<typeof setTimeout>|undefined;
  try{const [code,signal]=await Promise.race([actualRelay.exit,new Promise<never>((_,reject)=>{timer=setTimeout(()=>reject(Error('TEST_ONLY actual Relay retirement deadline')),5000);})]);
   assert.equal(code,0,'actual Relay must exit normally');assert.equal(signal,null,'actual Relay must not be killed');assert.match(actualRelay.output(),/^WSS_RETIRED\r?$/m);
   serviceRetirement={serviceStopped:true,connectionTasksJoined:true,exitCode:code!,signal:null};
  }finally{if(timer)clearTimeout(timer);}
 });
 const transport=()=>({mode:relayMode,connections,errors,policy:policyArtifact,serviceRetirement,hostAccepted:relay!.output().includes('"hostAdmitted":true'),hostReady:relay!.output().includes('"hostReady":true')});
 return{host,relay,artifacts,scope:ready.scope,storedScope:trusted.grant.scope,observations:()=>({browserConnections,artifactRequests}),transport,retireRelay,waitBridgeRetired:()=>relay!.wait(v=>relayMode==='legacy'?v.originalRelayRetired===true:v.hostBridgeRetired===true),async dispose(){await Promise.all([...sockets].map(async ws=>{const closed=once(ws,'close').catch(recordError);ws.terminate();await closed;}));await host!.dispose();await relay!.dispose();try{await retireRelay();}finally{await actualRelay?.dispose();}assert.deepEqual(errors,[],'TEST_ONLY transport/page callback errors');assert.equal(connections.length,browserConnections,'every requested browser socket must be independently verified');}};
 }catch(error){for(const ws of sockets)ws.terminate();await host?.dispose();await relay?.dispose();await actualRelay?.dispose();throw error;}
}
