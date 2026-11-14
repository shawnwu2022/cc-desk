import {test,expect,type BrowserContext,type Page} from '@playwright/test';
import {build} from 'esbuild';
import {createECDH,createHash} from 'node:crypto';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createRequire} from 'node:module';
import {join} from 'node:path';
const Ws=createRequire(join(process.cwd(),'package.json'))('ws');
import {readFile,writeFile} from 'node:fs/promises';
let module:string,fixture:string,relayModule:string
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 module=(await build({entryPoints:['remote/ui-terminal/src/browserRegistrationBridge.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
 relayModule=(await build({entryPoints:['remote/ui-terminal/src/browserRelay.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text;
 const keys=[1,2,3,4].map(n=>{const d=Buffer.alloc(32);d[31]=n;const e=createECDH('prime256v1');e.setPrivateKey(d);const p=e.getPublicKey();return{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')}})
 fixture=`export * from '/registration.mjs';
 export async function setup(){
 const keys=await Promise.all(${JSON.stringify(keys)}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));
 const encode=b=>btoa(String.fromCharCode(...b)).replace(/\\+/g,'-').replace(/\\//g,'_').replace(/=+$/,'');
 const decode=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
 const hash=async b=>new Uint8Array(await crypto.subtle.digest('SHA-256',b));
 const cbor=v=>{const head=(m,n)=>n<24?[m*32+n]:n<256?[m*32+24,n]:[m*32+25,n>>8,n&255];if(typeof v==='string'){const b=new TextEncoder().encode(v);return [...head(3,b.length),...b]};if(typeof v==='number')return v>=0?head(0,v):head(1,-1-v);if(v instanceof Uint8Array)return [...head(2,v.length),...v];if(v instanceof Map)return [...head(5,v.size),...Array.from(v).flatMap(([k,v])=>[...cbor(k),...cbor(v)])];throw Error('TEST_ONLY CBOR')};
 const der=raw=>{const part=b=>{let i=0;while(i<31&&b[i]===0)i++;let v=b.slice(i);if(v[0]&128)v=new Uint8Array([0,...v]);return [2,v.length,...v]};const a=part(raw.slice(0,32)),b=part(raw.slice(32));return new Uint8Array([48,a.length+b.length,...a,...b])};
 const f={keys:{tlsKey:keys[0],grantKey:keys[1],relayKey:keys[2]},creates:0,gets:0,options:[],hold:false,mode:'normal'};globalThis.f=f;
 Object.defineProperty(navigator.credentials,'create',{configurable:true,value:async options=>{f.creates++;f.options.push({attestation:options.publicKey.attestation,uv:options.publicKey.authenticatorSelection.userVerification,algorithm:options.publicKey.pubKeyCredParams[0].alg});const p=${JSON.stringify(keys[3])};const cose=new Map([[1,2],[3,-7],[-1,1],[-2,decode(p.x)],[-3,decode(p.y)]]);const client=new TextEncoder().encode(JSON.stringify({type:'webauthn.create',challenge:encode(new Uint8Array(options.publicKey.challenge)),origin:location.origin,crossOrigin:false}));const auth=new Uint8Array([...(await hash(new TextEncoder().encode('desk.invalid'))),f.mode==='no-uv'?65:69,0,0,0,0,...new Uint8Array(16),0,32,...new Uint8Array(32).fill(7),...cbor(cose)]);const att=new Uint8Array(cbor(new Map([['fmt','none'],['authData',auth],['attStmt',new Map()]])));const response=Object.create(AuthenticatorAttestationResponse.prototype);Object.defineProperties(response,{clientDataJSON:{value:client.buffer},attestationObject:{value:att.buffer}});const value=Object.create(PublicKeyCredential.prototype);Object.defineProperties(value,{type:{value:'public-key'},rawId:{value:new Uint8Array(32).fill(7).buffer},response:{value:response},getClientExtensionResults:{value:()=>({})}});if(f.hold)await new Promise(r=>{f.release=r});return value}});
 Object.defineProperty(navigator.credentials,'get',{configurable:true,value:async options=>{f.gets++;const client=new TextEncoder().encode(JSON.stringify({type:'webauthn.get',challenge:encode(new Uint8Array(options.publicKey.challenge)),origin:location.origin,crossOrigin:false}));const auth=new Uint8Array([...(await hash(new TextEncoder().encode('desk.invalid'))),5,0,0,0,0]);const raw=new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},keys[f.mode==='wrong-key'?2:3],new Uint8Array([...auth,...(await hash(client))])));const response=Object.create(AuthenticatorAssertionResponse.prototype);Object.defineProperties(response,{clientDataJSON:{value:client.buffer},authenticatorData:{value:auth.buffer},signature:{value:der(raw).buffer},userHandle:{value:null}});const value=Object.create(PublicKeyCredential.prototype);Object.defineProperties(value,{type:{value:'public-key'},rawId:{value:new Uint8Array(32).fill(7).buffer},response:{value:response},getClientExtensionResults:{value:()=>({})}});return value}});
 return f;}`
})

const entry=`import {startBrowserRegistrationWorker} from './remote/ui-terminal/src/browserRegistrationWorker.ts';
let releaseClose;const controller=new AbortController();
self.onmessage=async({data:d})=>{
 if(d.kind==='releaseClose'){releaseClose?.();return;}if(d.kind!=='start')return;
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
function oracle(mode:string){
 const file=process.env.CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_ORACLE;if(!file)throw Error('explicit native registration oracle executable required');
 const child=spawn(file,['NativeRegistration_RelayOracle_903','--ignored','--nocapture'],{stdio:['pipe','pipe','pipe'],windowsHide:true,env:{...process.env,CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE:mode}}),exit=once(child,'exit');
 const values:any[]=[],waiters:{resolve:(v:any)=>void;reject:(e:Error)=>void}[]=[];let buffer='',output='',ended=false;
 child.stderr.on('data',b=>{output+=b});child.stdout.on('data',b=>{buffer+=b;output+=b;for(;;){const n=buffer.indexOf('\n');if(n<0)break;const line=buffer.slice(0,n);buffer=buffer.slice(n+1);const at=line.indexOf('CC_DESK_REGISTRATION_ORACLE:');if(at<0)continue;const value=JSON.parse(line.slice(at+'CC_DESK_REGISTRATION_ORACLE:'.length));const waiter=waiters.shift();if(waiter)waiter.resolve(value);else values.push(value);}});
 void exit.then(()=>{ended=true;for(const waiter of waiters)waiter.reject(Error(output));});
 return{file,exit,releaseRelay:()=>child.stdin.end('TEST_ONLY_BROWSER_RETIRED\n'),output:()=>output,next:()=>values.length?Promise.resolve(values.shift()):ended?Promise.reject(Error(output)):new Promise<any>((resolve,reject)=>waiters.push({resolve,reject})),async dispose(){if(!ended){if(!child.stdin.writableEnded)child.stdin.end('TEST_ONLY_TEST_ABORTED\n');const timer=setTimeout(()=>child.kill(),2000);await exit;clearTimeout(timer);}}};
}
async function route(context:BrowserContext,origin:string){
 const wasm=process.env.CC_DESK_TEST_ONLY_REGISTRATION_WASM,glue=process.env.CC_DESK_TEST_ONLY_REGISTRATION_GLUE,repo=process.env.CC_DESK_TEST_RELAY_ROOT;if(!wasm||!glue||!repo)throw Error('explicit registration WASM/glue and relay root required');
 const sockets=new Set<any>();
 const frames:{direction:string;length:number;sha256:string}[]=[];let outerVerified=false,outerProtocol='';
 await context.route('https://desk.invalid/**',async r=>{const p=new URL(r.request().url()).pathname;return r.fulfill({headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'},contentType:p.endsWith('.wasm')?'application/wasm':p.endsWith('.mjs')||p.endsWith('.js')?'text/javascript':'text/html',body:p==='/registration.mjs'?module:p==='/relay.mjs'?relayModule:p==='/fixture.mjs'?fixture:p==='/worker.mjs'?workerModule:p==='/fixture_bg.wasm'?await readFile(wasm):p==='/fixture_bg.js'?await readFile(glue):'<!doctype html><title>TEST_ONLY actual Relay registration Worker</title>'});});
 // TEST_ONLY 外层 WSS 仅转发完整二进制消息，不解析准入/TLS，也不签名。
 // Node 校验 fixture CA 与 foobar.com 名称，不代表 Edge 证书界面通过验收。
 await context.routeWebSocket(origin,async r=>{
  const socket=new Ws(origin,[],{origin:'https://desk.invalid',ca:await readFile(join(repo,'tests/fixtures/root.pem')),rejectUnauthorized:true,servername:'foobar.com',lookup:(_host:unknown,_options:unknown,callback:Function)=>callback(null,[{address:'127.0.0.1',family:4}])});sockets.add(socket);
  const record=(direction:string,bytes:Buffer)=>frames.push({direction,length:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});
  socket.on('open',()=>{outerVerified=socket._socket.authorized===true;outerProtocol=socket._socket.getProtocol();expect(socket.protocol).toBe('');});
  r.onMessage(value=>{if(typeof value==='string')throw Error('binary-only forwarder');const bytes=Buffer.from(value);record('browser-to-relay',bytes);socket.send(bytes);});
  r.onClose(()=>socket.close());
  socket.on('message',(value:Buffer,isBinary:boolean)=>{if(!isBinary)throw Error('binary-only relay');const bytes=Buffer.from(value);record('relay-to-browser',bytes);r.send(bytes);});
  socket.on('error',()=>r.close());socket.on('close',()=>{sockets.delete(socket);r.close();});
 });
 return{wasm,glue,metrics:()=>({outerVerified,outerProtocol,frames}),dispose(){for(const socket of sockets)socket.terminate();}};
}
async function start(page:Page,peerSpki:number[],origin:string,options:{hold?:boolean;totalMs?:number;holdClose?:boolean}={}){
 await page.goto('https://desk.invalid/');
 await page.evaluate(async({peerSpki,origin,options})=>{
  const api=await import(location.origin+'/fixture.mjs'),relayApi=await import(location.origin+'/relay.mjs');const f=await api.setup();f.hold=options.hold===true;f.events=[];f.mainRetired=false;f.relayRetired=false;f.controller=new AbortController();
  const now=performance.timeOrigin+performance.now(),setupDeadline=now+Math.min(5000,options.totalMs??50000),registrationDeadline=now+(options.totalMs??50000);f.started=now;f.registrationDeadline=registrationDeadline;
  f.bridge=api.createBrowserRegistrationBridge({origin:location.origin,keys:f.keys,signal:f.controller.signal,setupDeadline,registrationDeadline});
  void f.bridge.waitClosed().then(()=>{f.mainRetired=true;},(e:Error)=>{f.mainError=String(e);});
  f.relay=relayApi.createBrowserRegistrationRelay({origin,tunnel:new Uint8Array(16).fill(3),identity:{keyId:new Uint8Array(16).fill(2),sign:async(r:{origin:string;keyId:Uint8Array;role:number;transcript:Uint8Array},signal:AbortSignal)=>{
   if(r.origin!==origin||r.role!==2||!r.keyId.every(v=>v===2)||signal.aborted)throw Error('TEST_ONLY relay identity mismatch');
   return new Uint8Array(await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},f.keys.relayKey,Uint8Array.from(r.transcript)));
  }},socket:new WebSocket(origin),mode:'legacy',setupDeadline,ioBudgetMs:30000,registrationDeadline,signal:f.controller.signal});
  const transport=await f.relay.ready;f.relayReady=true;
  void f.relay.waitClosed().then(()=>{f.relayRetired=true;f.relayClosedAt=performance.timeOrigin+performance.now();});
  const publicPoint='046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5';
  const localSpki=Uint8Array.from(('3059301306072a8648ce3d020106082a8648ce3d030107034200'+publicPoint).match(/../g)!,h=>parseInt(h,16));
  const channel=new MessageChannel();
  channel.port1.onmessage=async({data:d})=>{if(d.kind==='close'){transport.close();return;}if(d.kind==='write'){try{await transport.write(d.bytes);channel.port1.postMessage({kind:'written',seq:d.seq});}catch{transport.close();channel.port1.postMessage({kind:'writeError',seq:d.seq});}}};channel.port1.start();
  void(async()=>{try{for(;;){const bytes=await transport.read(16384);if(bytes===null)break;channel.port1.postMessage({kind:'bytes',bytes});}}catch{transport.close();}finally{await transport.waitClosed();channel.port1.postMessage({kind:'end'});}})();
  f.worker=new Worker('/worker.mjs',{type:'module'});f.worker.onmessage=(e:MessageEvent)=>{f.events.push(e.data);};f.worker.onerror=(e:ErrorEvent)=>{f.events.push({kind:'workerError',error:e.message});};
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
  const metrics=r.metrics();expect(metrics.outerVerified).toBe(true);expect(metrics.outerProtocol).toBe('TLSv1.3');
  if(scenario==='wait35'){expect(browser.elapsedMs).toBeGreaterThanOrEqual(35000);for(const direction of ['browser-to-relay','relay-to-browser'])expect(metrics.frames.filter(f=>f.direction===direction&&f.length===24).length).toBeGreaterThanOrEqual(3);}
  const files=[o.file,r.wasm,r.glue,process.env.CC_DESK_TEST_RELAY_EXE!,'tests/remote-browser/registration-relay.spec.ts','src-tauri/src/tests/native_cli_registration_relay.rs','remote/ui-terminal/src/browserRelay.ts'];
  const artifact={testOnly:true,scenario,result,browser,metrics,artifacts:await Promise.all(files.map(async file=>({file,sha256:createHash('sha256').update(await readFile(file)).digest('hex')})))};const path=info.outputPath('registration-relay.json');await writeFile(path,JSON.stringify(artifact,null,2));await info.attach('registration-relay',{path,contentType:'application/json'});
 }catch(error){const path=info.outputPath('failure.json');await writeFile(path,JSON.stringify({native:o.output(),metrics:r?.metrics(),browser:await page.evaluate(()=>Reflect.get(globalThis,'f')?.events).catch(()=>null)},null,2));await info.attach('failure',{path,contentType:'application/json'});throw error;}finally{await page.evaluate(()=>{const f=Reflect.get(globalThis,'f');f?.controller?.abort();f?.release?.();f?.bridge?.close();f?.relay?.close();f?.worker?.terminate();}).catch(()=>{});r?.dispose();await o.dispose();}
});
