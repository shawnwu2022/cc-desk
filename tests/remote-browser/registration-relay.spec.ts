import {test,expect,type BrowserContext,type Page} from '@playwright/test';
import {build} from 'esbuild';
import {createECDH,createHash,createPublicKey,verify} from 'node:crypto';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createRequire} from 'node:module';
import {join} from 'node:path';
const Ws=createRequire(join(process.cwd(),'package.json'))('ws');
import {readFile,writeFile} from 'node:fs/promises';
const entryRelayMode=process.env.CC_DESK_TEST_ONLY_REGISTRATION_RELAY_MODE??'legacy';
if(!['legacy','rendezvous-v1'].includes(entryRelayMode))throw Error('explicit TEST_ONLY registration relay mode required');
let module:string,fixture:string,relayModule:string,installationModule:string,productionEntryModule:string,productionWorkerModule:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 productionEntryModule=(await build({stdin:{resolveDir:process.cwd(),contents:`export {controlledBrowserRegistrationSource} from './remote/ui-terminal/src/browserRegistrationSource.ts'; export {createBrowserEnrollmentStore} from './remote/ui-terminal/src/browserEnrollmentStore.ts';`},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text;
 productionWorkerModule=(await build({entryPoints:['remote/ui-terminal/src/browserRegistrationEntry.worker.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text;
 module=(await build({entryPoints:['remote/ui-terminal/src/browserRegistrationBridge.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
 relayModule=(await build({entryPoints:['remote/ui-terminal/src/browserRelay.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text;
 installationModule=(await build({stdin:{resolveDir:process.cwd(),contents:`export {createBrowserRegistrationInstallation} from './remote/ui-terminal/src/browserRegistrationInstallation.ts'; export {createBrowserEnrollmentStore} from './remote/ui-terminal/src/browserEnrollmentStore.ts';`},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text;
 const keys=[1,2,3,4].map(n=>{const d=Buffer.alloc(32);d[31]=n;const e=createECDH('prime256v1');e.setPrivateKey(d);const p=e.getPublicKey();return{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')}})
 fixture=`export * from '/registration.mjs';
 export async function setup(randomCredential=false){
 const keys=await Promise.all(${JSON.stringify(keys)}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));
 const encode=b=>btoa(String.fromCharCode(...b)).replace(/\\+/g,'-').replace(/\\//g,'_').replace(/=+$/,'');
 const decode=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
 const hash=async b=>new Uint8Array(await crypto.subtle.digest('SHA-256',b));
 const cbor=v=>{const head=(m,n)=>n<24?[m*32+n]:n<256?[m*32+24,n]:[m*32+25,n>>8,n&255];if(typeof v==='string'){const b=new TextEncoder().encode(v);return [...head(3,b.length),...b]};if(typeof v==='number')return v>=0?head(0,v):head(1,-1-v);if(v instanceof Uint8Array)return [...head(2,v.length),...v];if(v instanceof Map)return [...head(5,v.size),...Array.from(v).flatMap(([k,v])=>[...cbor(k),...cbor(v)])];throw Error('TEST_ONLY CBOR')};
 const der=raw=>{const part=b=>{let i=0;while(i<31&&b[i]===0)i++;let v=b.slice(i);if(v[0]&128)v=new Uint8Array([0,...v]);return [2,v.length,...v]};const a=part(raw.slice(0,32)),b=part(raw.slice(32));return new Uint8Array([48,a.length+b.length,...a,...b])};
 const f={keys:{tlsKey:keys[0],grantKey:keys[1],relayKey:keys[2]},creates:0,gets:0,options:[],hold:false,mode:'normal',credential:undefined,randomCredential};globalThis.f=f;
 Object.defineProperty(navigator.credentials,'create',{configurable:true,value:async options=>{f.creates++;if(f.credential)throw Error('TEST_ONLY duplicate credential creation');f.credential=randomCredential?crypto.getRandomValues(new Uint8Array(32)):new Uint8Array(32).fill(7);f.options.push({attestation:options.publicKey.attestation,uv:options.publicKey.authenticatorSelection.userVerification,algorithm:options.publicKey.pubKeyCredParams[0].alg});const p=${JSON.stringify(keys[3])};const cose=new Map([[1,2],[3,-7],[-1,1],[-2,decode(p.x)],[-3,decode(p.y)]]);const client=new TextEncoder().encode(JSON.stringify({type:'webauthn.create',challenge:encode(new Uint8Array(options.publicKey.challenge)),origin:location.origin,crossOrigin:false}));const auth=new Uint8Array([...(await hash(new TextEncoder().encode('desk.invalid'))),f.mode==='no-uv'?65:69,0,0,0,0,...new Uint8Array(16),0,32,...f.credential,...cbor(cose)]);const att=new Uint8Array(cbor(new Map([['fmt','none'],['authData',auth],['attStmt',new Map()]])));const response=Object.create(AuthenticatorAttestationResponse.prototype);Object.defineProperties(response,{clientDataJSON:{value:client.buffer},attestationObject:{value:att.buffer}});const value=Object.create(PublicKeyCredential.prototype);Object.defineProperties(value,{type:{value:'public-key'},rawId:{value:f.credential.slice().buffer},response:{value:response},getClientExtensionResults:{value:()=>({})}});if(f.hold)await new Promise(r=>{f.release=r});return value}});
 Object.defineProperty(navigator.credentials,'get',{configurable:true,value:async options=>{f.gets++;if(!f.credential||options.publicKey.allowCredentials.length!==1||!new Uint8Array(options.publicKey.allowCredentials[0].id).every((v,i)=>v===f.credential[i]))throw Error('TEST_ONLY credential binding mismatch');const client=new TextEncoder().encode(JSON.stringify({type:'webauthn.get',challenge:encode(new Uint8Array(options.publicKey.challenge)),origin:location.origin,crossOrigin:false}));const auth=new Uint8Array([...(await hash(new TextEncoder().encode('desk.invalid'))),5,0,0,0,0]);const raw=new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},keys[f.mode==='wrong-key'?2:3],new Uint8Array([...auth,...(await hash(client))])));const response=Object.create(AuthenticatorAssertionResponse.prototype);Object.defineProperties(response,{clientDataJSON:{value:client.buffer},authenticatorData:{value:auth.buffer},signature:{value:der(raw).buffer},userHandle:{value:null}});const value=Object.create(PublicKeyCredential.prototype);Object.defineProperties(value,{type:{value:'public-key'},rawId:{value:f.credential.slice().buffer},response:{value:response},getClientExtensionResults:{value:()=>({})}});return value}});
 return f;}`
})

// 生产入口与真实专用 Worker；合成凭证、原生 cfg(test) 审批与 WSS 外层转发仍限于 fixture。
for(const scenario of ['install','wrong-pin','start-yes','start-deny','start-cancel','start-remember','start-remember-deny','start-remember-cancel'])test(`RegistrationEntry_ActualRelayNative_${entryRelayMode}_${scenario}_003`,async({page,context},info)=>{
 test.setTimeout(45000);
 const startup=scenario.startsWith('start-'),remember=scenario.startsWith('start-remember'),success=scenario==='install'||scenario==='start-yes'||scenario==='start-remember';
 const o=oracle(startup?scenario:scenario==='install'?'install':'cancel',entryRelayMode);let r:Awaited<ReturnType<typeof route>>|undefined;
 try{
  const ready=await o.next();expect(ready.plan).toBeDefined();expect(ready.plan.configuration.relay.mode).toBe(entryRelayMode);r=await route(context,ready.origin);
  const extra:Record<string,string>={'/production-entry.mjs':productionEntryModule,'/browserRegistrationEntry.worker.ts':productionWorkerModule,'/browser-registration.js':'export async function readApprovedBrowserRegistrationInputs(signal){globalThis.f.sourceReads++;if(signal.aborted)throw Error("TEST_ONLY cancelled");return globalThis.f.approvedInputs}'};
  for(const [path,body] of Object.entries(extra))await context.route('https://desk.invalid'+path,route=>route.fulfill({headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:'text/javascript',body}));
  await page.goto('https://desk.invalid/');
  await page.evaluate(async({plan,scenario})=>{
   const fixture=await import(location.origin+'/fixture.mjs'),api=await import(location.origin+'/production-entry.mjs'),f=await fixture.setup(true);f.controller=new AbortController();f.events=[];f.sourceReads=0;f.retired=false;
   if(scenario==='wrong-pin')plan.configuration.hostSpki=plan.configuration.grant.issuerSpki;
   f.approvedInputs={keys:f.keys,approvedPlan:plan,artifacts:{wasm:location.origin+'/fixture_bg.wasm',glue:location.origin+'/fixture_bg.js'}};
   const now=performance.timeOrigin+performance.now();f.source=api.controlledBrowserRegistrationSource();f.owner=await f.source.register({signal:f.controller.signal,setupDeadline:now+10000,registrationDeadline:now+40000});
   void f.owner.evidence.then((value:unknown)=>f.events.push({kind:'evidence',value}),()=>f.events.push({kind:'evidenceFailed'}));
   void f.owner.outcome.then((value:unknown)=>f.events.push({kind:'outcome',value}),()=>f.events.push({kind:'outcomeFailed'}));
   void f.owner.completed.then(()=>f.events.push({kind:'completed'}),()=>f.events.push({kind:'completionFailed'}));
   f.installState='pending';void f.owner.install().then((value:unknown)=>{f.installState='installed';f.result=value},()=>{f.installState='rejected'}).then(()=>f.owner.waitClosed()).then(()=>{f.retired=true},()=>{f.retirementFailed=true});
   f.store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName:plan.databaseName});
  },{plan:structuredClone(ready.plan),scenario});
  await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').retired),{timeout:20000}).toBe(true);
  const browser=await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{creates:f.creates,gets:f.gets,sourceReads:f.sourceReads,state:f.installState,result:f.result,status:await f.store.status(),events:f.events}});
  if(success){
   expect(browser).toMatchObject({creates:1,gets:1,sourceReads:1,state:'installed',result:{revision:'1',state:'active'},status:{revision:'1',state:'active'}});
   expect(browser.events).toContainEqual(expect.objectContaining({kind:'evidence',value:expect.objectContaining({protocol:772,fullHandshake:true,rawPublicKey:true,alpnMatches:true})}));
   expect(browser.events).toContainEqual({kind:'completed'});
  }else if(remember){
   // 原登记验证已完成，但独立本机保存 No/取消不得发布任一端记录。
   expect(browser).toMatchObject({creates:1,gets:1,sourceReads:1,state:'rejected',status:undefined});
   expect(browser.events).toContainEqual(expect.objectContaining({kind:'evidence',value:expect.objectContaining({protocol:772,fullHandshake:true,rawPublicKey:true,alpnMatches:true})}));
   expect(browser.events).not.toContainEqual({kind:'completed'});
  }else expect(browser).toMatchObject({creates:0,gets:0,sourceReads:1,state:'rejected',status:undefined,events:expect.not.arrayContaining([expect.objectContaining({kind:'evidence'})])});
  const native=await o.next();expect(native).toMatchObject({relayMode:entryRelayMode==='legacy'?'explicit-legacy':entryRelayMode,nativeCandidateInstalled:success,acknowledged:success,locked:true,hostNetworkAttempts:0,hostKeyOpens:startup&&scenario!=='start-deny'?2:0,nativeTransportRetired:true});
  if(startup)expect(native).toMatchObject({nativeStartupOwner:true,nativeKeysRetired:true,registrationKeyOpens:scenario==='start-deny'?[0,0]:[1,1]});
  if(remember)expect(native).toMatchObject({nativeRemembered:success,nativePairingReloadLocked:success});
  o.releaseRelay();expect((await o.exit)[0],o.output()).toBe(0);
  if(success){
   await page.reload();expect(await page.evaluate(async databaseName=>{const api=await import(location.origin+'/production-entry.mjs');crypto.subtle.importKey=()=>{throw Error('TEST_ONLY unexpected key import')};const store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName}),signal=new AbortController().signal,session=await store.openExisting(async()=>{},signal),material=await session.provider.readApproved(signal);const nonExportable=[material.handles.tlsKey,material.handles.grantKey].every(k=>k instanceof CryptoKey&&!k.extractable);session.close();await session.waitClosed();return{status:await store.status(),nonExportable,relayMode:JSON.parse(new TextDecoder().decode(material.configuration)).relay.mode}},ready.plan.databaseName)).toEqual({status:{revision:'1',state:'active'},nonExportable:true,relayMode:entryRelayMode});
  }
  const files=[o.file,r.wasm,r.glue,process.env.CC_DESK_TEST_RELAY_EXE!,'remote/ui-terminal/src/browserRegistrationEntry.ts','remote/ui-terminal/src/browserRegistrationEntry.worker.ts','remote/ui-terminal/src/browserRegistrationArtifacts.ts','remote/ui-terminal/src/browserRegistrationSource.ts','remote/ui-terminal/src/browserRelay.ts','src-tauri/src/tests/native_cli_registration_relay.rs','src-tauri/src/tests/native_cli_registration_start.rs','tests/remote-browser/registration-relay.spec.ts'];
  const metrics=r.metrics();expect(metrics.forwarderErrors).toEqual([]);expect(metrics.connections).toBe(1);expect(metrics.requestedProtocols).toEqual([entryRelayMode==='rendezvous-v1'?['cc-desk-relay-rendezvous-v1']:[]]);
  if(success||scenario==='wrong-pin'||remember){expect(metrics.outerVerified).toBe(true);expect(metrics.outerProtocol).toBe('TLSv1.3');expect(metrics.outerSubprotocol).toBe(entryRelayMode==='rendezvous-v1'?'cc-desk-relay-rendezvous-v1':'');}
  const artifact={testOnly:true,scenario,relayMode:entryRelayMode,browserVersion:context.browser()?.version(),outerHandshakeBoundary:'Playwright echoes requested protocol; Node ws independently validates actual CA/hostname and negotiated protocol, not Edge WebPKI/subprotocol qualification',boundary:'production entry/Worker and real TLS/Relay/native fixture; synthetic credentials and approval; not production deployment',browser,native,metrics:r.metrics(),artifacts:await Promise.all(files.map(async file=>({file,sha256:createHash('sha256').update(await readFile(file)).digest('hex')})))};
  const path=info.outputPath('production-registration-entry.json');await writeFile(path,JSON.stringify(artifact,null,2));await info.attach('production-registration-entry',{path,contentType:'application/json'});
 }catch(error){const path=info.outputPath('production-entry-failure.json');await writeFile(path,JSON.stringify({native:o.output(),metrics:r?.metrics(),browser:await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return f?{events:f.events,state:f.installState,retired:f.retired,retirementFailed:f.retirementFailed}:undefined}).catch(()=>null)},null,2));throw error;}
 finally{await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f?.owner?.close();f?.controller?.abort()}).catch(()=>{});r?.dispose();await o.dispose();}
});

const entry=`import {startBrowserRegistrationWorker} from './remote/ui-terminal/src/browserRegistrationWorker.ts';
let releaseClose;const controller=new AbortController();
self.onmessage=async({data:d})=>{
 if(d.kind==='releaseClose'){releaseClose?.();return;}if(d.kind==='close'){controller.abort();return;}if(d.kind!=='start')return;
 try{
 const io=d.io;let queue=[],reader,ended=false,retired,nextWrite=1,writes=new Map();
 const actualClosed=new Promise(r=>{retired=r;});let join=actualClosed;if(d.holdClose)join=actualClosed.then(()=>new Promise(r=>{releaseClose=r;postMessage({kind:'closeHeld'});}));
 io.onmessage=({data:v})=>{if(v.kind==='bytes'){const bytes=v.bytes;if(reader){const r=reader;reader=undefined;r(bytes);}else queue.push(bytes);}if(v.kind==='written'||v.kind==='writeError'){const pending=writes.get(v.seq);writes.delete(v.seq);if(pending){if(v.kind==='written')pending.resolve();else pending.reject(Error('TEST_ONLY original transport write failed'));}}if(v.kind==='end'){ended=true;reader?.(null);reader=undefined;retired();}};io.start();
 const transport={read:async max=>{if(queue.length){const v=queue.shift();if(v.length>max)throw Error('TEST_ONLY chunk');return v;}if(ended)return null;return new Promise(r=>{reader=r;});},write:bytes=>new Promise((resolve,reject)=>{const seq=nextWrite++;writes.set(seq,{resolve,reject});io.postMessage({kind:'write',seq,bytes});}),close:()=>io.postMessage({kind:'close'}),waitClosed:()=>join};
 const glue=await import('/fixture_bg.js'),wasm=await(await fetch('/fixture_bg.wasm')).arrayBuffer();
 const owner=await startBrowserRegistrationWorker({wasm,glue,bridge:d.bridge,localSpki:d.localSpki,peerSpki:d.peerSpki,transport,signal:controller.signal});
 owner.evidence.then(value=>postMessage({kind:'evidence',value}),error=>postMessage({kind:'error',error:String(error)}));
 owner.outcome.then(value=>postMessage({kind:'outcome',value}),()=>{});
 owner.completed.then(()=>postMessage({kind:'completed'}),error=>postMessage({kind:'completionError',error:String(error)}));
 owner.waitClosed().then(()=>postMessage({kind:'retired'}),error=>postMessage({kind:'retirementError',error:String(error)}));
 }catch(error){postMessage({kind:'startupError',error:String(error)});}
};`;
let workerModule:string;
test.beforeAll(async()=>{workerModule=(await build({stdin:{contents:entry,resolveDir:process.cwd()},bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text;});
function oracle(mode:string,relayMode='legacy'){
 const file=process.env.CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_ORACLE;if(!file)throw Error('explicit native registration oracle executable required');
 const child=spawn(file,[mode.startsWith('start-')?'NativeRegistration_StartOracle_904':'NativeRegistration_RelayOracle_903','--ignored','--nocapture'],{stdio:['pipe','pipe','pipe'],windowsHide:true,env:{...process.env,CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE:mode,CC_DESK_TEST_ONLY_REGISTRATION_RELAY_MODE:relayMode}}),exit=once(child,'exit');
 const values:any[]=[],waiters:{resolve:(v:any)=>void;reject:(e:Error)=>void}[]=[];let buffer='',output='',ended=false;
 child.stderr.on('data',b=>{output+=b});child.stdout.on('data',b=>{buffer+=b;output+=b;for(;;){const n=buffer.indexOf('\n');if(n<0)break;const line=buffer.slice(0,n);buffer=buffer.slice(n+1);const at=line.indexOf('CC_DESK_REGISTRATION_ORACLE:');if(at<0)continue;const value=JSON.parse(line.slice(at+'CC_DESK_REGISTRATION_ORACLE:'.length));const waiter=waiters.shift();if(waiter)waiter.resolve(value);else values.push(value);}});
 void exit.then(()=>{ended=true;for(const waiter of waiters)waiter.reject(Error(output));});
 return{file,exit,releaseRelay:()=>child.stdin.end('TEST_ONLY_BROWSER_RETIRED\n'),output:()=>output,next:()=>values.length?Promise.resolve(values.shift()):ended?Promise.reject(Error(output)):new Promise<any>((resolve,reject)=>waiters.push({resolve,reject})),async dispose(){if(!ended){if(!child.stdin.writableEnded)child.stdin.end('TEST_ONLY_TEST_ABORTED\n');const timer=setTimeout(()=>child.kill(),2000);await exit;clearTimeout(timer);}}};
}
async function route(context:BrowserContext,origin:string){
 const wasm=process.env.CC_DESK_TEST_ONLY_REGISTRATION_WASM,glue=process.env.CC_DESK_TEST_ONLY_REGISTRATION_GLUE,repo=process.env.CC_DESK_TEST_RELAY_ROOT;if(!wasm||!glue||!repo)throw Error('explicit registration WASM/glue and relay root required');
 const sockets=new Set<any>();
 const frames:{direction:string;length:number;sha256:string}[]=[];let outerVerified=false,outerProtocol='',outerSubprotocol='',connections=0;const requestedProtocols:string[][]=[],forwarderErrors:string[]=[];
 await context.route('https://desk.invalid/**',async r=>{const p=new URL(r.request().url()).pathname;return r.fulfill({headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:p.endsWith('.wasm')?'application/wasm':p.endsWith('.mjs')||p.endsWith('.js')?'text/javascript':'text/html',body:p==='/registration.mjs'?module:p==='/installation.mjs'?installationModule:p==='/relay.mjs'?relayModule:p==='/fixture.mjs'?fixture:p==='/worker.mjs'?workerModule:p==='/fixture_bg.wasm'?await readFile(wasm):p==='/fixture_bg.js'?await readFile(glue):'<!doctype html><title>TEST_ONLY actual Relay registration Worker</title>'});});
 // TEST_ONLY 外层 WSS 仅转发完整二进制消息，不解析准入/TLS，也不签名。
 // Playwright 回显请求的首个协议；实际协商由 Node ws 单独核验，不能冒充 Edge 握手资格。
 // Node 校验 fixture CA 与 foobar.com 名称，不代表 Edge 证书界面通过验收。
 await context.routeWebSocket(origin,async r=>{
  const protocols=r.protocols();requestedProtocols.push([...protocols]);connections++;let negotiated=false;
  const socket=new Ws(origin,protocols,{origin:'https://desk.invalid',ca:await readFile(join(repo,'tests/fixtures/root.pem')),rejectUnauthorized:true,servername:'foobar.com',lookup:(_host:unknown,_options:unknown,callback:Function)=>callback(null,[{address:'127.0.0.1',family:4}])});sockets.add(socket);
  const failClosed=(reason:string)=>{forwarderErrors.push(reason);negotiated=false;try{r.close();}catch{}socket.terminate();};
  const record=(direction:string,bytes:Buffer)=>frames.push({direction,length:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});
  socket.on('open',()=>{outerVerified=socket._socket.authorized===true;outerProtocol=socket._socket.getProtocol();outerSubprotocol=socket.protocol;negotiated=outerVerified&&socket.protocol===(protocols[0]??'')&&protocols.length<=1;if(!negotiated)failClosed('actual WSS negotiation mismatch');});
  r.onMessage(value=>{if(!negotiated){failClosed('browser data before verified WSS negotiation');return;}if(typeof value==='string'){failClosed('text browser frame');return;}try{const bytes=Buffer.from(value);record('browser-to-relay',bytes);socket.send(bytes,(error?:Error)=>{if(error)failClosed('WSS send failed');});}catch{failClosed('browser forwarding failed');}});
  r.onClose(()=>{try{socket.close();}catch{failClosed('WSS close failed');}});
  socket.on('message',(value:Buffer,isBinary:boolean)=>{if(!negotiated){failClosed('relay data before verified WSS negotiation');return;}if(!isBinary){failClosed('text relay frame');return;}try{const bytes=Buffer.from(value);record('relay-to-browser',bytes);r.send(bytes);}catch{failClosed('relay forwarding failed');}});
  socket.on('error',()=>failClosed('WSS socket error'));socket.on('close',()=>{sockets.delete(socket);try{r.close();}catch{forwarderErrors.push('browser close failed');}});
 });
 return{wasm,glue,metrics:()=>({outerVerified,outerProtocol,outerSubprotocol,requestedProtocols,connections,forwarderErrors,frames}),dispose(){for(const socket of sockets)socket.terminate();}};
}
async function start(page:Page,peerSpki:number[],origin:string,options:{hold?:boolean;totalMs?:number;holdClose?:boolean;randomCredential?:boolean;approvedPlan?:any}={}){
 await page.goto('https://desk.invalid/');
 await page.evaluate(async({peerSpki,origin,options})=>{
  const api=await import(location.origin+'/fixture.mjs'),relayApi=await import(location.origin+'/relay.mjs');const f=await api.setup(options.randomCredential===true);f.hold=options.hold===true;f.events=[];f.mainRetired=false;f.relayRetired=false;f.controller=new AbortController();
  const now=performance.timeOrigin+performance.now(),setupDeadline=now+Math.min(5000,options.totalMs??50000),registrationDeadline=now+(options.totalMs??50000);f.started=now;f.registrationDeadline=registrationDeadline;
  f.bridge=api.createBrowserRegistrationBridge({origin:location.origin,keys:f.keys,signal:f.controller.signal,setupDeadline,registrationDeadline});
  const mainClosed=f.bridge.waitClosed().then(()=>{f.mainRetired=true;});void mainClosed.catch((e:Error)=>{f.mainError=String(e);});
  f.relay=relayApi.createBrowserRegistrationRelay({origin,tunnel:new Uint8Array(16).fill(3),identity:{keyId:new Uint8Array(16).fill(2),sign:async(r:{origin:string;keyId:Uint8Array;role:number;transcript:Uint8Array},signal:AbortSignal)=>{
   if(r.origin!==origin||r.role!==2||!r.keyId.every(v=>v===2)||signal.aborted)throw Error('TEST_ONLY relay identity mismatch');
   return new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},f.keys.relayKey,Uint8Array.from(r.transcript)));
  }},socket:new WebSocket(origin),mode:'legacy',setupDeadline,ioBudgetMs:30000,registrationDeadline,signal:f.controller.signal});
  const transport=await f.relay.ready;f.relayReady=true;
  const relayClosed=f.relay.waitClosed().then(()=>{f.relayRetired=true;f.relayClosedAt=performance.timeOrigin+performance.now();});void relayClosed.catch(()=>{});
  const publicPoint='046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5';
  const localSpki=Uint8Array.from(('3059301306072a8648ce3d020106082a8648ce3d030107034200'+publicPoint).match(/../g)!,h=>parseInt(h,16));
  const channel=new MessageChannel();
  channel.port1.onmessage=async({data:d})=>{if(d.kind==='close'){transport.close();return;}if(d.kind==='write'){try{await transport.write(d.bytes);channel.port1.postMessage({kind:'written',seq:d.seq});}catch{transport.close();channel.port1.postMessage({kind:'writeError',seq:d.seq});}}};channel.port1.start();
  void(async()=>{try{for(;;){const bytes=await transport.read(16384);if(bytes===null)break;channel.port1.postMessage({kind:'bytes',bytes});}}catch{transport.close();}finally{await transport.waitClosed();channel.port1.postMessage({kind:'end'});}})();
  // 在原 Worker 启动前捕获各通知，不能从事后事件数组拼出成功或退休。
  const deferred=()=>{let resolve!:(value?:any)=>void,reject!:(error:Error)=>void;const promise=new Promise<any>((r,j)=>{resolve=r;reject=j;});void promise.catch(()=>{});return{promise,resolve,reject};};
  const outcome=deferred(),completed=deferred(),workerClosed=deferred();
  const fail=(error:Error)=>{outcome.reject(error);completed.reject(error);workerClosed.reject(error);};
  f.worker=new Worker('/worker.mjs',{type:'module'});f.worker.onmessage=({data:d}:MessageEvent)=>{
   f.events.push(d);if(d.kind==='outcome')outcome.resolve(d.value);if(d.kind==='completed')completed.resolve();if(d.kind==='retired')workerClosed.resolve();
   if(d.kind==='completionError'){outcome.reject(Error(d.error));completed.reject(Error(d.error));}if(d.kind==='retirementError')workerClosed.reject(Error(d.error));if(d.kind==='startupError')fail(Error(d.error));
  };f.worker.onerror=(e:ErrorEvent)=>{f.events.push({kind:'workerError',error:e.message});fail(Error(e.message));};
  const joined=Promise.allSettled([mainClosed,relayClosed,workerClosed.promise]).then(results=>{if(results.some(value=>value.status==='rejected'))throw Error('TEST_ONLY original registration retirement failed');f.sessionRetired=true;});void joined.catch(()=>{});
  f.session=Object.freeze({outcome:outcome.promise,completed:completed.promise,waitClosed:()=>joined,close(){f.bridge.close();f.relay.close();f.worker.postMessage({kind:'close'});}});
  if(options.approvedPlan){
   const installationApi=await import(location.origin+'/installation.mjs');
   f.approvedPlan=structuredClone(options.approvedPlan);f.store=installationApi.createBrowserEnrollmentStore({origin:location.origin,databaseName:f.approvedPlan.databaseName});
   f.activeWrites=0;f.activeCommits=0;const put=IDBObjectStore.prototype.put;
   IDBObjectStore.prototype.put=function(...args:any[]){const value=args[0];if(value?.state==='active'){f.activeWrites++;this.transaction.addEventListener('complete',()=>{f.activeCommits++;},{once:true});}return Reflect.apply(put,this,args);};
   f.installation=installationApi.createBrowserRegistrationInstallation({session:f.session,keys:f.keys,approvedPlan:f.approvedPlan,signal:f.controller.signal,deadline:registrationDeadline});
   f.installState='pending';f.installPromise=f.installation.install().then((value:unknown)=>{f.installState='installed';f.installResult=value;},(error:Error)=>{f.installState='rejected';f.installError=String(error);});
  }
  const bridge=f.bridge.worker;f.worker.postMessage({kind:'start',bridge,localSpki,peerSpki:new Uint8Array(peerSpki),holdClose:options.holdClose,io:channel.port2},[bridge.port,bridge.signer.port,channel.port2]);
 },{peerSpki,origin,options});
}
async function waitEvent(page:Page,kind:string){await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').events),{message:kind,timeout:15000}).toContainEqual(expect.objectContaining({kind}));return page.evaluate(kind=>Reflect.get(globalThis,'f').events.find((v:{kind:string})=>v.kind===kind),kind);}
async function retired(page:Page){await waitEvent(page,'retired');await expect.poll(()=>page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return f.mainRetired&&f.relayRetired;})).toBe(true);}
const scenarios=['yes','deny','wait35','wrong-pin','browser-cancel','browser-deadline','native-cancel','native-deadline','held-retirement'] as const;
for(const scenario of scenarios)test(`RegistrationRelay_ActualEdgeNative_${scenario}_001`,async({page,context},info)=>{
 test.setTimeout(scenario==='wait35'?65000:scenario.startsWith('native-')?45000:30000);
 const mode=scenario==='deny'?'deny':scenario==='native-cancel'||scenario==='native-deadline'?scenario:scenario==='yes'||scenario==='wait35'||scenario==='held-retirement'?'yes':'cancel';
 const o=oracle(mode);let r:Awaited<ReturnType<typeof route>>|undefined,nativeResult:any;
 try{
  const ready=await o.next();r=await route(context,ready.origin);
  let pin=ready.hostSpki;if(scenario==='wrong-pin'){const d=Buffer.alloc(32);d[31]=3;const e=createECDH('prime256v1');e.setPrivateKey(d);pin=Array.from(Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),e.getPublicKey()]));}
  const held=['wait35','browser-cancel','browser-deadline','native-cancel','native-deadline'].includes(scenario);
  await start(page,pin,ready.origin,{hold:held,totalMs:scenario==='browser-deadline'?2500:50000,holdClose:scenario==='held-retirement'});
  if(scenario==='wrong-pin'){
   await waitEvent(page,'completionError');expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{creates:f.creates,gets:f.gets,evidence:f.events.some((v:{kind:string})=>v.kind==='evidence')};})).toEqual({creates:0,gets:0,evidence:false});
  }else{
   const evidence=await waitEvent(page,'evidence');expect(evidence.value).toMatchObject({protocol:772,fullHandshake:true,rawPublicKey:true,alpnMatches:true});
   if(held){await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'f').release)).toBe('function');
    if(scenario==='wait35'){await new Promise(resolve=>setTimeout(resolve,35000));expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{relay:f.relayRetired,main:f.mainRetired,gets:f.gets};})).toEqual({relay:false,main:false,gets:0});}
    else if(scenario==='native-cancel'||scenario==='native-deadline'){
     nativeResult=await o.next();expect(nativeResult).toMatchObject({nativeCandidateInstalled:false,acknowledged:false,nativeTransportRetired:true,locked:true});
     expect(nativeResult.nativeElapsedMs).toBeGreaterThanOrEqual(scenario==='native-cancel'?2900:3900);expect(nativeResult.nativeElapsedMs).toBeLessThan(7000);
     // legacy Relay 不广播对端退出；下一空心跳触发 Offline，原认证器 promise 仍由调用者持有。
     await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').relayRetired),{timeout:35000}).toBe(true);
     const state=await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{elapsed:f.relayClosedAt-f.started,main:f.mainRetired,worker:f.events.some((e:{kind:string})=>e.kind==='retired'),gets:f.gets,outcome:f.events.some((e:{kind:string})=>e.kind==='outcome'||e.kind==='completed')};});
     expect(state.elapsed).toBeGreaterThanOrEqual(9000);expect(state.elapsed).toBeLessThan(33000);expect(state).toMatchObject({main:false,worker:false,gets:0,outcome:false});
    }
    else{if(scenario==='browser-cancel')await page.evaluate(()=>Reflect.get(globalThis,'f').controller.abort());await waitEvent(page,'completionError');expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{main:f.mainRetired,worker:f.events.some((e:{kind:string})=>e.kind==='retired'),gets:f.gets};})).toEqual({main:false,worker:false,gets:0});}
    await page.evaluate(()=>Reflect.get(globalThis,'f').release());
    if(scenario==='native-cancel'||scenario==='native-deadline')await waitEvent(page,'completionError');
   }
  }
  const success=mode==='yes'||mode==='deny';let outcome;
  if(success){outcome=await waitEvent(page,'outcome');expect(outcome.value.kind).toBe(mode==='yes'?'RunLocalCandidateInstalled':'Rejected');await waitEvent(page,'completed');}
  if(scenario==='held-retirement'){await waitEvent(page,'closeHeld');expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{main:f.mainRetired,worker:f.events.some((e:{kind:string})=>e.kind==='retired')};})).toEqual({main:false,worker:false});await page.evaluate(()=>Reflect.get(globalThis,'f').worker.postMessage({kind:'releaseClose'}));}
  await retired(page);
  const result=nativeResult??await o.next();expect(result).toMatchObject({nativeCandidateInstalled:mode==='yes',locked:true,hostNetworkAttempts:0,hostKeyOpens:0,acknowledged:success,persistentTrust:false,actualRelay:true,relayMode:'explicit-legacy',nativeTransportRetired:true});if(mode==='yes')expect(outcome!.value).toEqual(result.receipt);o.releaseRelay();expect((await o.exit)[0],o.output()).toBe(0);
  const browser=await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return{elapsedMs:f.relayClosedAt-f.started,creates:f.creates,gets:f.gets,relayReady:f.relayReady,events:f.events};});
  if(scenario==='browser-deadline')expect(browser.elapsedMs).toBeLessThan(3500);
  if(scenario==='native-cancel'||scenario==='native-deadline'){expect(browser.gets).toBe(0);expect(browser.events.some((e:{kind:string})=>e.kind==='outcome'||e.kind==='completed')).toBe(false);}
  const metrics=r.metrics();expect(metrics.forwarderErrors).toEqual([]);expect(metrics.outerVerified).toBe(true);expect(metrics.outerProtocol).toBe('TLSv1.3');
  if(scenario==='wait35'){expect(browser.elapsedMs).toBeGreaterThanOrEqual(35000);for(const direction of ['browser-to-relay','relay-to-browser'])expect(metrics.frames.filter(f=>f.direction===direction&&f.length===24).length).toBeGreaterThanOrEqual(3);}
  const files=[o.file,r.wasm,r.glue,process.env.CC_DESK_TEST_RELAY_EXE!,'tests/remote-browser/registration-relay.spec.ts','src-tauri/src/tests/native_cli_registration_relay.rs','remote/ui-terminal/src/browserRelay.ts'];
  const artifact={testOnly:true,scenario,result,browser,metrics,artifacts:await Promise.all(files.map(async file=>({file,sha256:createHash('sha256').update(await readFile(file)).digest('hex')})))};const path=info.outputPath('registration-relay.json');await writeFile(path,JSON.stringify(artifact,null,2));await info.attach('registration-relay',{path,contentType:'application/json'});
 }catch(error){const path=info.outputPath('failure.json');await writeFile(path,JSON.stringify({native:o.output(),metrics:r?.metrics(),browser:await page.evaluate(()=>Reflect.get(globalThis,'f')?.events).catch(()=>null)},null,2));await info.attach('failure',{path,contentType:'application/json'});throw error;}finally{await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f?.controller?.abort();f?.release?.();f?.bridge?.close();f?.relay?.close();f?.worker?.terminate();}).catch(()=>{});r?.dispose();await o.dispose();}
});

// 新 credential 经原 TLS 通道取得独立持久安装批准，原三方退休后才可提交同一组 handles。
const installationScenarios=['install','install-deny','install-document','install-expired','install-held-retirement','install-plan-mismatch','install-run-local','remember','remember-deny','remember-document','remember-expired','remember-target'] as const;
for(const scenario of installationScenarios)test(`RegistrationRelay_Persistent_${scenario}_002`,async({page,context},info)=>{
 test.setTimeout(45000);
 const rejectedByNative=['install-deny','install-document','install-expired','remember-deny','remember-document','remember-expired','remember-target'].includes(scenario);
 const mode=scenario.startsWith('remember')?scenario:rejectedByNative?scenario:scenario==='install-run-local'?'yes':'install';
 const o=oracle(mode);let r:Awaited<ReturnType<typeof route>>|undefined,reloaded:any,revoked:any;
 try{
  const ready=await o.next();expect(ready.kind).toBe('ready');expect(ready.plan).toBeDefined();
  expect(ready.plan.configuration.profile).not.toHaveProperty('credentialId');
  for(const name of ['credentialId','credentialSpki','backupEligible'])expect(ready.plan.configuration.grant.profile).not.toHaveProperty(name);
  const approvedPlan=structuredClone(ready.plan);if(scenario==='install-plan-mismatch')approvedPlan.configuration.grant.role='Observer';
  r=await route(context,ready.origin);
  await start(page,ready.hostSpki,ready.origin,{randomCredential:true,approvedPlan,holdClose:scenario==='install-held-retirement'});
  expect((await waitEvent(page,'evidence')).value).toMatchObject({protocol:772,fullHandshake:true,rawPublicKey:true,alpnMatches:true});
  const outcome=(await waitEvent(page,'outcome')).value;
  expect(outcome.kind).toBe(rejectedByNative?'Rejected':mode==='yes'?'RunLocalCandidateInstalled':'BrowserInstallationApproved');
  await waitEvent(page,'completed');
  if(scenario==='install-held-retirement'){
   await waitEvent(page,'closeHeld');
   expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');return{state:f.installState,writes:f.activeWrites,commits:f.activeCommits,status:await f.store.status(),main:f.mainRetired,worker:f.events.some((e:{kind:string})=>e.kind==='retired'),aborted:f.controller.signal.aborted};})).toEqual({state:'pending',writes:0,commits:0,status:undefined,main:false,worker:false,aborted:false});
   // 持有真实 transport 退休不改 completed；释放原 join 后才允许安装。
   await page.evaluate(()=>Reflect.get(globalThis,'f').worker.postMessage({kind:'releaseClose'}));
  }
  await retired(page);
  const result=await o.next();expect(result).toMatchObject({nativeCandidateInstalled:!rejectedByNative,locked:true,hostNetworkAttempts:0,hostKeyOpens:0,registrationTlsConnections:1,acknowledged:true,persistentTrust:false,genuineGuiApproval:false,actualRelay:true,relayMode:'explicit-legacy',nativeTransportRetired:true});
  if(scenario.startsWith('remember'))expect(result).toMatchObject({nativeRecordWritten:scenario==='remember',nativeReloadLocked:scenario==='remember'});
  if(!rejectedByNative)expect(outcome).toEqual(result.receipt);
  o.releaseRelay();expect((await o.exit)[0],o.output()).toBe(0);
  const success=scenario==='install'||scenario==='install-held-retirement'||scenario==='remember';
  await expect.poll(()=>page.evaluate(()=>Reflect.get(globalThis,'f').installState)).toBe(success?'installed':'rejected');
  const browser=await page.evaluate(async()=>{const f=Reflect.get(globalThis,'f');await f.installation.waitClosed();return{creates:f.creates,gets:f.gets,credential:Array.from(f.credential),state:f.installState,result:f.installResult,status:await f.store.status(),writes:f.activeWrites,commits:f.activeCommits,aborted:f.controller.signal.aborted,mainRetired:f.mainRetired,relayRetired:f.relayRetired,sessionRetired:f.sessionRetired,events:f.events};});
  expect(browser).toMatchObject({creates:1,gets:1,state:success?'installed':'rejected',writes:success?1:0,commits:success?1:0,aborted:false,mainRetired:true,relayRetired:true,sessionRetired:true});
  expect(browser.credential).toHaveLength(32);expect(browser.credential).not.toEqual(Array(32).fill(7));
  const credentialId=Buffer.from(browser.credential as number[]).toString('base64url');
  if(success){
   expect(browser.result).toEqual({revision:'1',state:'active'});expect(browser.status).toEqual(browser.result);
   const expected=JSON.parse(outcome.installation.configuration);expect(expected.profile.credentialId).toBe(credentialId);expect(expected.grant.profile.credentialId).toBe(credentialId);
   expect(outcome.installation.requestedUserHandle).toBe(ready.plan.requestedUserHandle);
   // 关闭原页面，下一页面只能经 store 的原 trusted provider 读取，不再次导入私钥或读取原始 IDB。
   await page.close();const next=await context.newPage();await next.goto('https://desk.invalid/');
   reloaded=await next.evaluate(async databaseName=>{
    const api=await import(location.origin+'/installation.mjs'),store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName}),signal=new AbortController().signal;
    const originalImport=crypto.subtle.importKey,originalSign=crypto.subtle.sign;let imports=0;const signedKeys:CryptoKey[]=[];
    crypto.subtle.importKey=function(...args:any[]){imports++;return Reflect.apply(originalImport,this,args);};
    crypto.subtle.sign=function(...args:any[]){signedKeys.push(args[1]);return Reflect.apply(originalSign,this,args);};
    let session:any;
    try{
     session=await store.openExisting(async()=>{},signal);if(!session)throw Error('TEST_ONLY persisted provider missing');
     const material=await session.provider.readApproved(signal),configuration=JSON.parse(new TextDecoder().decode(material.configuration));
     const transcript=new TextEncoder().encode('TEST_ONLY persisted relay handle proof');
     const signature=await material.handles.relaySign({origin:configuration.relay.endpoint,keyId:Uint8Array.from(atob(configuration.relay.keyId.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0)),role:2,transcript},signal);
     const handles=[material.handles.tlsKey,material.handles.grantKey,...signedKeys];
     const loaded={metadata:await store.status(),configuration,imports,handles:handles.map(key=>({cryptoKey:key instanceof CryptoKey,extractable:key.extractable,type:key.type,algorithm:key.algorithm.name,usages:Array.from(key.usages)})),distinctHandles:new Set(handles).size,signature:Array.from(signature),transcript:Array.from(transcript)};
     session.close();await session.waitClosed();session=undefined;return loaded;
    }finally{crypto.subtle.importKey=originalImport;crypto.subtle.sign=originalSign;if(session){session.close();await session.waitClosed();}}
   },ready.plan.databaseName);
   expect(reloaded.metadata).toEqual({revision:'1',state:'active'});expect(reloaded.configuration).toEqual(expected);expect(reloaded.imports).toBe(0);expect(reloaded.distinctHandles).toBe(3);
   expect(reloaded.handles).toEqual(Array.from({length:3},()=>({cryptoKey:true,extractable:false,type:'private',algorithm:'ECDSA',usages:['sign']})));
   expect(verify('sha256',Buffer.from(reloaded.transcript),{key:createPublicKey({key:Buffer.from(ready.plan.relaySpki,'base64url'),format:'der',type:'spki'}),dsaEncoding:'ieee-p1363'},Buffer.from(reloaded.signature))).toBe(true);
   expect(await next.evaluate(async databaseName=>{const api=await import(location.origin+'/installation.mjs');return api.createBrowserEnrollmentStore({origin:location.origin,databaseName}).revoke();},ready.plan.databaseName)).toEqual({revision:'1',state:'revoked'});
   await next.close();const after=await context.newPage();await after.goto('https://desk.invalid/');
   revoked=await after.evaluate(async databaseName=>{const api=await import(location.origin+'/installation.mjs'),store=api.createBrowserEnrollmentStore({origin:location.origin,databaseName});return{status:await store.status(),locked:(await store.openExisting(async()=>{},new AbortController().signal))===undefined};},ready.plan.databaseName);
   expect(revoked).toEqual({status:{revision:'1',state:'revoked'},locked:true});await after.close();
  }else expect(browser.status).toBeUndefined();
  const metrics=r.metrics();expect(metrics.forwarderErrors).toEqual([]);expect(metrics.outerVerified).toBe(true);expect(metrics.outerProtocol).toBe('TLSv1.3');
  const files=[o.file,r.wasm,r.glue,process.env.CC_DESK_TEST_RELAY_EXE!,'tests/remote-browser/registration-relay.spec.ts','src-tauri/src/tests/native_cli_registration_relay.rs','remote/ui-terminal/src/browserRegistrationInstallation.ts','remote/ui-terminal/src/browserEnrollmentStore.ts','remote/ui-terminal/src/browserRegistrationWorker.ts'];
  const artifact={testOnly:true,scenario,boundary:'registration-to-persistent-installation-reload-revoke; not terminal-controller or genuine approval',plan:ready.plan,result,browser,reloaded,revoked,metrics,artifacts:await Promise.all(files.map(async file=>({file,sha256:createHash('sha256').update(await readFile(file)).digest('hex')})))};
  const path=info.outputPath('registration-installation-relay.json');await writeFile(path,JSON.stringify(artifact,null,2));await info.attach('registration-installation-relay',{path,contentType:'application/json'});
 }catch(error){const path=info.outputPath('failure.json');await writeFile(path,JSON.stringify({native:o.output(),metrics:r?.metrics(),browser:await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');return f?{events:f.events,installState:f.installState,installError:f.installError,mainError:f.mainError}:undefined;}).catch(()=>null)},null,2));await info.attach('failure',{path,contentType:'application/json'});throw error;
 }finally{await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f?.installation?.close();f?.controller?.abort();f?.release?.();f?.bridge?.close();f?.relay?.close();f?.worker?.terminate();}).catch(()=>{});r?.dispose();await o.dispose();}
});
