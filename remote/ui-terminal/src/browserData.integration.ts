import {decodeReply,projectionAck} from './wire.ts';
import {createApprovedBrowserProfileReader,type PublicBrowserPairingConfiguration} from './browserPairingProfile.ts';
import type {ApprovedBrowserInputs} from './browserEntry.ts';
import {createConnection} from 'node:net';import {command,input} from './wire.ts';
import {browserClientPort} from './browserClient.ts';import {nativeConnection} from './native.ts';import {TerminalClient} from './client.ts';
import {test} from 'node:test';import assert from 'node:assert/strict';import {readFileSync} from 'node:fs';import {pathToFileURL} from 'node:url';import {Worker,MessageChannel} from 'node:worker_threads';import {on,once} from 'node:events';import {spawn} from 'node:child_process';import {createECDH,createPrivateKey,createHash,sign} from 'node:crypto';
import {createBrowserTlsSignerBroker} from './browserTlsSigner.ts';import {createBrowserGrantBroker} from './browserGrant.ts';import {produceBrowserApproval} from './browserApproval.ts';
const wasm=readFileSync(process.env.CC_DESK_TEST_ONLY_DATA_WASM??'MISSING_EXPLICIT_GRANT_WASM'),glueUrl=pathToFileURL(process.env.CC_DESK_TEST_ONLY_DATA_GLUE??'MISSING_EXPLICIT_GRANT_GLUE').href;
// 公开TEST_ONLY scalar1/2/3；不生成、登记或使用生产凭据。
function fixture(scalar:number){const d=Buffer.alloc(32);d[31]=scalar;const e=createECDH('prime256v1');e.setPrivateKey(d);const point=e.getPublicKey();const jwk={kty:'EC',crv:'P-256',x:point.subarray(1,33).toString('base64url'),y:point.subarray(33).toString('base64url'),d:d.toString('base64url')};return{jwk,spki:Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),point])};}
const issuer=fixture(1),tls=fixture(2),credential=fixture(3),credentialId=new Uint8Array(32).fill(7),scope={session:'05'.repeat(16),run:'PUBLIC_TEST_RUN',generation:'7'};
function platform(options:{mutation?:string;hold?:boolean;delay?:number}={}){const saved=new Map<string,PropertyDescriptor|undefined>();class WindowFixture{static [Symbol.hasInstance](v:unknown){return v===globalThis;}}class Assertion{}class Credential{type='public-key';rawId=credentialId.slice().buffer;response!:Assertion;}let calls=0,release:()=>void=()=>{},began!:()=>void,previous:Credential|undefined,replay=false;const started=new Promise<void>(resolve=>{began=resolve;});
 class DocumentFixture extends EventTarget{visibilityState='visible';}const documentSource=new DocumentFixture(),windowEvents=new EventTarget();
 const credentials={async get(request:CredentialRequestOptions){calls++;began();if(replay&&previous)return previous;const publicKey=request.publicKey!;const challengeBytes=Buffer.from(publicKey.challenge as ArrayBuffer);if(options.mutation==='challenge')challengeBytes[0]^=1;const challenge=challengeBytes.toString('base64url'),client=Buffer.from(JSON.stringify({type:'webauthn.get',challenge,origin:options.mutation==='origin'?'https://evil.invalid':'https://desk.invalid',crossOrigin:false}));const auth=Buffer.concat([createHash('sha256').update('desk.invalid').digest(),Buffer.from([options.mutation==='uv'?1:5,0,0,0,0])]);const signature=sign('sha256',Buffer.concat([auth,createHash('sha256').update(client).digest()]),createPrivateKey({key:credential.jwk,format:'jwk'}));if(options.mutation==='signature')signature[signature.length-1]^=1;const response=new Assertion();Object.assign(response,{authenticatorData:Uint8Array.from(auth).buffer,clientDataJSON:Uint8Array.from(client).buffer,signature:Uint8Array.from(signature).buffer});const value=new Credential();value.response=response;if(options.mutation==='credential')value.rawId=new Uint8Array(32).fill(6).buffer;previous=value;if(options.delay)await new Promise(resolve=>setTimeout(resolve,options.delay));
  // 合成credentials故意不响应Abort，用于证明原get Promise没有被假取消。
  if(options.hold)await new Promise<void>(resolve=>{release=resolve;});return value;}};
 for(const[n,v]of Object.entries({Document:DocumentFixture,document:documentSource,addEventListener:windowEvents.addEventListener.bind(windowEvents),removeEventListener:windowEvents.removeEventListener.bind(windowEvents),dispatchEvent:windowEvents.dispatchEvent.bind(windowEvents),Window:WindowFixture,isSecureContext:true,crossOriginIsolated:true,location:{origin:'https://desk.invalid'},self:globalThis,top:globalThis,PublicKeyCredential:Credential,AuthenticatorAssertionResponse:Assertion,navigator:{credentials}})){saved.set(n,Object.getOwnPropertyDescriptor(globalThis,n));Object.defineProperty(globalThis,n,{configurable:true,value:v});}return Object.assign(()=>{for(const[n,d]of saved){if(d)Object.defineProperty(globalThis,n,d);else Reflect.deleteProperty(globalThis,n);}},{hidden:()=>{documentSource.visibilityState='hidden';documentSource.dispatchEvent(new Event('visibilitychange'));},lifecycle:(name:string)=>{(name==='freeze'?documentSource:windowEvents).dispatchEvent(new Event(name));},calls:()=>calls,started,release:()=>release(),replay:()=>{replay=true;}});}
async function host(mode='normal'){const executable=process.env.CC_DESK_TEST_ONLY_DATA_HOST;if(!executable)throw Error('explicit TEST_ONLY Host required');const child=spawn(executable,['--exact','browser_data_fixture_tests::BrowserData_OriginalHostFixture_001','--nocapture'],{env:{...process.env,CC_DESK_TEST_ONLY_DATA_MODE:mode},stdio:['ignore','pipe','pipe'],windowsHide:true});const exited=once(child,'exit');let buffer='',stderr='';const messages:Record<string,unknown>[]=[];let ready!:(value:{port:number;hostSpki:string;scope:typeof scope;controlPort:number})=>void,reject!:(e:Error)=>void;const first=new Promise<{port:number;hostSpki:string;scope:typeof scope;controlPort:number}>((a,b)=>{ready=a;reject=b;});const total=setTimeout(()=>child.kill(),8000);child.on('error',reject);child.on('close',()=>reject(Error('Host ended before ready')));child.stdout.on('data',(bytes:Buffer)=>{buffer+=bytes.toString();if(buffer.length>8192){child.kill();reject(Error('Host bound'));return;}for(;;){const at=buffer.indexOf('\n');if(at<0)break;const line=buffer.slice(0,at);buffer=buffer.slice(at+1);const begin=line.indexOf('{');if(begin<0)continue;try{const value=JSON.parse(line.slice(begin));messages.push(value);if(value.kind==='ready')ready(value);}catch{reject(Error('Host framing'));}}});child.stderr.on('data',(b:Buffer)=>{stderr+=b.toString();if(stderr.length>8192)child.kill();});let value;try{value=await first;}catch(error){if(child.exitCode===null&&child.signalCode===null)child.kill();await exited;clearTimeout(total);throw error;}return{...value,messages,async until(predicate:(value:Record<string,unknown>)=>boolean){const end=performance.now()+2000;while(!messages.some(predicate)&&performance.now()<end)await new Promise(resolve=>setImmediate(resolve));const found=messages.find(predicate);assert.ok(found,JSON.stringify({messages,stderr}));return found;},async wait(kind:string){const end=performance.now()+2000;while(!messages.some(v=>v.kind===kind)&&performance.now()<end)await new Promise(resolve=>setImmediate(resolve));const result=messages.find(v=>v.kind===kind);assert.ok(result,JSON.stringify({kind,messages,stderr}));return result;},async dispose(){if(child.exitCode===null&&child.signalCode===null)child.kill();await exited;clearTimeout(total);}};}
async function client(server:{port:number;hostSpki:string;scope:typeof scope;controlPort:number},extra:Record<string,unknown>={},approved?:ApprovedBrowserInputs){const key=(jwk:JsonWebKey)=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);const profile=approved?.profile??{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId,tlsKey:await key(tls.jwk),grantKey:await key(extra.wrongGrantKey?credential.jwk:issuer.jwk)};const setupBudget=Number(extra.setupBudget??1500),deadline=performance.timeOrigin+performance.now()+setupBudget;
 const selectedScope=approved?.grant.scope??server.scope,application=new MessageChannel();let resolveRetired!:()=>void,rejectRetired!:(error:Error)=>void;const retired=new Promise<void>((a,b)=>{resolveRetired=a;rejectRetired=b;});void retired.catch(()=>{});
 const tlsBroker=createBrowserTlsSignerBroker(profile,{timeoutMs:setupBudget,setupDeadline:deadline});let presentations=0,releasePresentation:()=>void=()=>{},presented!:()=>void;const presentationStarted=new Promise<void>(resolve=>{presented=resolve;});
 let grantBroker:ReturnType<typeof createBrowserGrantBroker>;try{grantBroker=createBrowserGrantBroker(profile,{configuration:approved?.grant??{issuerSpki:issuer.spki,principal:'04'.repeat(32),scope:selectedScope,role:'Controller',budgetMs:Number(extra.grantBudget??1800),profile:{origin:profile.origin,rpId:profile.rpId,credentialId,credentialSpki:credential.spki,backupEligible:false,allowBackedUp:false},...(extra.configuration as object??{})},signal:extra.signal as AbortSignal|undefined,present:approved?.present??(async(value)=>{presentations++;presented();assert.equal(value.principalId,'04'.repeat(32));assert.deepEqual(value.scope,selectedScope);if(typeof extra.onPresent==='function')extra.onPresent();if(extra.holdPresentation)await new Promise<void>(resolve=>{releasePresentation=resolve;});if(extra.presentationDelay)await new Promise(resolve=>setTimeout(resolve,Number(extra.presentationDelay)));})});}catch(error){tlsBroker.close();await tlsBroker.waitClosed();throw error;}
 let holdRpc=false,heldRpc:Record<string,unknown>|undefined,heldResolve:((value:Record<string,unknown>)=>void)|undefined;const postRpc=application.port1.postMessage.bind(application.port1);application.port1.postMessage=(value,...args)=>{if(holdRpc&&['snapshot','exchange'].includes(value?.kind)){holdRpc=false;heldRpc=value;heldResolve?.(value);return;}Reflect.apply(postRpc,application.port1,[value,...args]);};
 let worker:Worker;try{worker=new Worker(new URL('./browserTlsWorker.fixture.ts',import.meta.url),{workerData:{application:application.port2,wasm,glueUrl,bridge:tlsBroker.worker,grantBridge:grantBroker.worker,localSpki:approved?.localSpki??tls.spki,peerSpki:approved?.hostSpki??Buffer.from(server.hostSpki,'hex'),port:server.port,clientTrust:approved?.trust??{hostKeyEpoch:'9007199254740993',clientKeyEpoch:'18446744073709551615',origin:'wss://relay.invalid/'},traceRuntime:true,...(extra.worker as object??{})},transferList:[application.port2,tlsBroker.worker.port as never,grantBroker.worker.port as never]});}catch(error){tlsBroker.close();grantBroker.close();await tlsBroker.waitClosed();await grantBroker.waitClosed();throw error;}const events=on(worker,'message',{signal:AbortSignal.timeout(6500)}),messages:Record<string,unknown>[]=[],inbox:Record<string,unknown>[]=[];
 worker.on('message',m=>{if(m.kind==='retired')resolveRetired();if(m.kind==='retireFailed')rejectRetired(Error('Worker unknown retirement'));});const app=browserClientPort(application.port1 as unknown as MessagePort,()=>{tlsBroker.close();grantBroker.close();worker.postMessage({kind:'close'});},()=>retired,grantBroker.worker.cancel!);
 async function next(kind:string){for(;;){const at=inbox.findIndex(v=>v.kind===kind||['startupError','retireFailed','runtimeTrace'].includes(String(v.kind))||((kind==='grantAccepted'||kind==='peerInstalled')&&(v.kind==='grantError'||v.kind==='peerError')));if(at>=0){const value=inbox.splice(at,1)[0]!;if(value.kind!==kind)throw Error(String(value.error??value.trace??value.kind));return value;}const item=await events.next();if(item.done)throw Error('Worker ended');messages.push(item.value[0]);inbox.push(item.value[0]);}}
 return{app,retired,holdRpc:()=>{holdRpc=true;return new Promise<Record<string,unknown>>(resolve=>{heldResolve=resolve;});},releaseRpc:()=>{if(heldRpc){const original=heldRpc;heldRpc=undefined;postRpc(original);}},worker,tlsBroker,grantBroker,next,messages,profile,deadline,presentationStarted,releasePresentation:()=>releasePresentation(),presentations:()=>presentations,async observe(){worker.postMessage({kind:'observe'});return next('observed');},async dispose(){releasePresentation();worker.postMessage({kind:'releaseAll'});worker.postMessage({kind:'releaseLoad'});tlsBroker.close();grantBroker.close();worker.postMessage({kind:'close'});await tlsBroker.waitClosed();await grantBroker.waitClosed();await events.return!();await worker.terminate();}};
}


test('BrowserData_OriginalApplicationRoundtrip_001',async()=>{
 const p=platform(),h=await host();let f:Awaited<ReturnType<typeof client>>|undefined;
 try{
  f=await client(h,{configuration:{peerInstall:{budgetMs:1000}}});
  const descriptor=await f.app.descriptor;assert.equal(descriptor.role,'Controller');
  const connection=nativeConnection(f.app),terminal=new TerminalClient(descriptor.target,descriptor.binding);terminal.connect(connection);
  await terminal.snapshot();assert.equal(terminal.phase,'Synced');assert.ok(terminal.frame);
  await terminal.window();assert.equal(terminal.phase,'Ready');
  const receipt=await terminal.submit('02'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_STAGE1'));
  assert.equal(receipt.message,'InputReceipt');assert.equal(receipt.outcome,'HostWritten');assert.equal(terminal.phase,'NeedsSettlement');
  const query=await terminal.query();assert.equal(query.status,'Known');assert.equal(query.outcome,'HostWritten');assert.equal(query.proof,receipt.proof);
  await terminal.settle();assert.equal(terminal.phase,'Idle');const cursor=await terminal.cursor();assert.equal(cursor.pending,null);
  terminal.disconnect();await connection.waitClosed();
  const effects=await h.wait('effects');assert.equal(effects.writeCount,1);assert.equal(effects.bytes,Buffer.from('TEST_ONLY_STAGE1').toString('hex'));
 }catch(error){throw Error(JSON.stringify({error:String(error),host:h.messages,worker:f?.messages}));}finally{try{await f?.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

async function ready(h:Awaited<ReturnType<typeof host>>,extra:Record<string,unknown>={}){
 const f=await client(h,{configuration:{peerInstall:{budgetMs:1000}},...extra});
 let lastInput:Uint8Array|undefined,lastWindow:Record<string,unknown>|undefined;
 const descriptor=await f.app.descriptor,connection=nativeConnection({...f.app,exchange:async(payload,budget)=>{if(JSON.parse(new TextDecoder().decode(payload)).message==='InputChunk')lastInput=payload.slice();const bytes=await f.app.exchange(payload,budget);const reply=JSON.parse(new TextDecoder().decode(bytes));if(reply.message==='WindowGranted')lastWindow=reply;return bytes;}}),terminal=new TerminalClient(descriptor.target,descriptor.binding);
 let late=()=>{};terminal.connect({...connection,onInvalidate(listener){late=listener;return connection.onInvalidate!(listener);}});await terminal.snapshot();await terminal.window();assert.equal(terminal.phase,'Ready');return{f,descriptor,connection,terminal,late:()=>late(),inputPayload:()=>lastInput!,windowReply:()=>lastWindow!};
}
async function revoke(port:number){const socket=createConnection({host:'127.0.0.1',port});const closed=once(socket,'close');socket.end(new Uint8Array([1]));await closed;}
test('BrowserData_SameProducerReconnectNeverReplays_002',async()=>{
 const p=platform(),h=await host('reconnect');let a:Awaited<ReturnType<typeof ready>>|undefined,b:Awaited<ReturnType<typeof client>>|undefined;
 try{
  a=await ready(h);const receipt=await a.terminal.submit('03'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_RECONNECT'));
  assert.equal(receipt.message,'InputReceipt');assert.equal(receipt.outcome,'HostWritten');a.terminal.disconnect();await a.connection.waitClosed();
  b=await client(h,{configuration:{peerInstall:{budgetMs:1000}}});const descriptor=await b.app.descriptor;
  assert.deepEqual(descriptor.target,a.descriptor.target);assert.notEqual(descriptor.binding,a.descriptor.binding);assert.equal(descriptor.fence,'2');
  const next=nativeConnection(b.app);a.terminal.connect(next,descriptor.binding);assert.equal(a.terminal.phase,'NeedsSettlement');
  const cursor=await a.terminal.cursor();assert.equal(cursor.pending?.operation,'03'.repeat(16));
  const query=await a.terminal.query();assert.equal(query.outcome,'HostWritten');assert.equal(query.proof,receipt.proof);
  await a.terminal.settle();assert.equal(a.terminal.phase,'Idle');await a.terminal.snapshot();await a.terminal.window();assert.equal(a.terminal.phase,'Ready');a.late();assert.equal(a.terminal.phase,'Ready');
  a.terminal.disconnect();await next.waitClosed();const effects=await h.wait('effects');assert.equal(effects.writeCount,1);assert.equal(effects.bytes,Buffer.from('TEST_ONLY_RECONNECT').toString('hex'));
 }catch(error){throw Error(JSON.stringify({error:String(error),host:h.messages}));}finally{try{await b?.dispose();}finally{try{await a?.f.dispose();}finally{try{await h.dispose();}finally{p();}}}}
});
for(const mode of ['hidden','pagehide','freeze','expired','revoke'])test('BrowserData_ReadyInvalidates_'+mode,async()=>{
 const p=platform(),h=await host(mode==='expired'||mode==='revoke'?mode:'normal');let a:Awaited<ReturnType<typeof ready>>|undefined;
 try{
  a=await ready(h);const invalid=new Promise<void>(resolve=>a!.f.app.onInvalidate!(resolve));
  if(mode==='hidden')p.hidden();else if(mode==='pagehide'||mode==='freeze')p.lifecycle(mode);else if(mode==='revoke')await revoke(h.controlPort);
  await invalid;assert.notEqual(a.terminal.phase,'Ready');await assert.rejects(a.terminal.submit('04'.repeat(16),'1',new TextEncoder().encode('MUST_NOT_WRITE')));
  await a.f.retired;const effects=await h.wait('effects');assert.equal(effects.writeCount,0);
 }finally{try{await a?.f.dispose();}finally{try{await h.dispose();}finally{p();}}}
});
for(const mode of ['WindowBeforeACK','InputBeforeACK','UnknownCommand','ExtraField','DuplicateKey','Oversize'])test('BrowserData_OriginalRejects_'+mode,async()=>{
 const p=platform(),h=await host();let f:Awaited<ReturnType<typeof client>>|undefined;
 try{
  f=await client(h,{configuration:{peerInstall:{budgetMs:1000}}});await f.app.descriptor;
  const encode=(v:unknown)=>new TextEncoder().encode(JSON.stringify(v));
  const payload=mode==='WindowBeforeACK'?command('Window','1'):mode==='InputBeforeACK'?input('05'.repeat(16),'1','06'.repeat(32),'2000',new TextEncoder().encode('MUST_NOT_WRITE')):mode==='UnknownCommand'?encode({profile:1,message:'Create',request:'1'}):mode==='ExtraField'?encode({profile:1,message:'Window',request:'1',rights:'All'}):mode==='DuplicateKey'?new TextEncoder().encode('{"profile":1,"message":"Window","request":"1","request":"2"}'):new Uint8Array(32749);
  await assert.rejects(f.app.exchange(payload,1000));await f.app.close();const effects=await h.wait('effects');assert.equal(effects.writeCount,0);
 }finally{try{await f?.dispose();}finally{try{await h.dispose();}finally{p();}}}
});
test('BrowserData_CancelHeldWriteRetainsOriginalWork_003',async()=>{
 const p=platform(),h=await host();let a:Awaited<ReturnType<typeof ready>>|undefined;
 try{
  a=await ready(h);a.f.worker.postMessage({kind:'holdNextWrite'});await a.f.next('holdingNext');
  const operation=a.terminal.submit('07'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_HELD'));void operation.catch(()=>{});
  const held=await a.f.next('writeHeld');await h.wait('dispatched');let retired=false;void a.f.retired.then(()=>{retired=true;},()=>{retired=true;});
  p.hidden();assert.notEqual(a.terminal.phase,'Ready');const observed=await a.f.observe();assert.equal(observed.retired,false);assert.equal(retired,false);
  a.f.worker.postMessage({kind:'releaseWrite',sequence:held.sequence});await assert.rejects(operation);await a.f.retired;
  const effects=await h.wait('effects');assert.ok(Number(effects.writeCount)<=1);
 }finally{try{await a?.f.dispose();}finally{try{await h.dispose();}finally{p();}}}
});
test('BrowserData_WasmRootUncertainRetirementPropagates_004',async()=>{
 const p=platform(),h=await host();let f:Awaited<ReturnType<typeof client>>|undefined;
 try{f=await client(h,{configuration:{peerInstall:{budgetMs:1000}},worker:{applicationUncertain:true}});const connection=nativeConnection(f.app);await assert.rejects(f.app.descriptor);connection.close();await assert.rejects(connection.waitClosed(),/uncertain|unknown/);}
 finally{try{await f?.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

test('BrowserData_UnknownLocalResultReconnectsWithoutReplay_005',async()=>{
 const p=platform(),h=await host('reconnect');let a:Awaited<ReturnType<typeof ready>>|undefined,b:Awaited<ReturnType<typeof client>>|undefined;
 try{
  a=await ready(h);a.f.worker.postMessage({kind:'holdNextWrite'});await a.f.next('holdingNext');
  const operation=a.terminal.submit('08'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_UNCERTAIN'));void operation.catch(()=>{});
  const held=await a.f.next('writeHeld');await h.until(value=>value.kind==='dispatched'&&value.count===4);
  a.terminal.disconnect();a.f.worker.postMessage({kind:'releaseWrite',sequence:held.sequence});await assert.rejects(operation);await a.connection.waitClosed();
  b=await client(h,{configuration:{peerInstall:{budgetMs:1000}}});const descriptor=await b.app.descriptor,next=nativeConnection(b.app);a.terminal.connect(next,descriptor.binding);
  assert.equal(a.terminal.phase,'NeedsSettlement');const cursor=await a.terminal.cursor();assert.equal(cursor.pending?.operation,'08'.repeat(16));
  const query=await a.terminal.query();assert.equal(query.status,'Known');assert.equal(query.outcome,'HostWritten');await a.terminal.settle();assert.equal(a.terminal.phase,'Idle');
  a.terminal.disconnect();await next.waitClosed();const effects=await h.wait('effects');assert.equal(effects.writeCount,1);assert.equal(effects.bytes,Buffer.from('TEST_ONLY_UNCERTAIN').toString('hex'));
 }catch(error){throw Error(JSON.stringify({error:String(error),host:h.messages}));}finally{try{await b?.dispose();}finally{try{await a?.f.dispose();}finally{try{await h.dispose();}finally{p();}}}}
});
for(const source of ['perf','wall','gap'])test('BrowserData_ClockContinuityInvalidates_'+source,async()=>{
 const p=platform(),h=await host();let a:Awaited<ReturnType<typeof ready>>|undefined;
 try{a=await ready(h);const invalid=new Promise<void>(resolve=>a!.f.app.onInvalidate!(resolve));a.f.worker.postMessage({kind:'clockFault',source});await invalid;assert.equal(a.terminal.phase,'NotConnected');await assert.rejects(a.terminal.submit('09'.repeat(16),'1',new Uint8Array([65])));await a.f.retired;const effects=await h.wait('effects');assert.equal(effects.writeCount,0);}
 finally{try{await a?.f.dispose();}finally{try{await h.dispose();}finally{p();}}}
});
test('BrowserData_ClockSmallSamplesDoNotAccumulateRounding_006',async()=>{
 const p=platform(),h=await host();let a:Awaited<ReturnType<typeof ready>>|undefined;
 try{a=await ready(h);a.f.worker.postMessage({kind:'clockSteps'});const observed=await a.f.next('clockStepped');assert.ok(Number(observed.elapsed)>=999);assert.ok(Number(observed.elapsed)<1100);a.terminal.disconnect();await a.connection.waitClosed();}
 finally{try{await a?.f.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

for(const variant of ['ExactReplay','Conflict','WrongSequence','OldACK','OldWindow'])test('BrowserData_OriginalApplicationFences_'+variant,async()=>{
 const p=platform(),h=await host();let a:Awaited<ReturnType<typeof ready>>|undefined;
 try{
  a=await ready(h);const encode=(v:unknown)=>new TextEncoder().encode(JSON.stringify(v));let expectedWrites=0;
  if(variant==='ExactReplay'||variant==='Conflict'){
   const first=await a.terminal.submit('0b'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_ONCE'));assert.equal(first.message,'InputReceipt');expectedWrites=1;
   const payload=a.inputPayload();const value=JSON.parse(new TextDecoder().decode(payload));if(variant==='Conflict')value.bytes='RElGRkVSRU5U';
   const reply=decodeReply(await a.f.app.exchange(variant==='ExactReplay'?payload:encode(value),1000));
   if(variant==='ExactReplay')assert.deepEqual(reply,first);else{assert.equal(reply.message,'InputReceipt');assert.equal(reply.outcome,'Unknown');assert.equal(reply.proof,null);const original=await a.terminal.query();assert.equal(original.outcome,'HostWritten');assert.equal(original.proof,first.proof);assert.equal(a.terminal.phase,'NeedsSettlement');}
  }else if(variant==='WrongSequence'){
   const reply=await a.terminal.submit('0c'.repeat(16),'2',new Uint8Array([65]));assert.equal(reply.message,'InputReceipt');assert.equal(reply.outcome,'Unknown');assert.equal(reply.proof,null);assert.equal(a.terminal.phase,'NeedsSettlement');await a.terminal.cursor();assert.equal(a.terminal.phase,'NeedsSettlement');await assert.rejects(a.terminal.window());await assert.rejects(a.terminal.settle());
  }else if(variant==='OldACK'){
   const old=a.terminal.frame!.stamp;await a.terminal.snapshot();await assert.rejects(a.f.app.exchange(projectionAck(old),1000));
  }else{
   const old=a.windowReply();await a.terminal.snapshot();await a.terminal.window();await assert.rejects(a.f.app.exchange(input('0d'.repeat(16),'1',String(old.window),String(old.notAfterHostMs),new Uint8Array([65])),1000));
  }
  a.terminal.disconnect();await a.connection.waitClosed();const effects=await h.wait('effects');assert.equal(effects.writeCount,expectedWrites);
 }catch(error){throw Error(JSON.stringify({error:String(error),host:h.messages}));}finally{try{await a?.f.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

test('BrowserData_QueuedWindowRequestCannotRenewOriginalDeadline_007',async()=>{
 const p=platform(),h=await host();let f:Awaited<ReturnType<typeof client>>|undefined;
 try{
  f=await client(h,{configuration:{peerInstall:{budgetMs:1000}}});await f.app.descriptor;
  const held=f.holdRpc(),operation=f.app.snapshot(command('Snapshot','1'),50);void operation.catch(()=>{});const message=await held;
  const end=Number(message.deadline);assert.ok(Number.isFinite(end));
  while(performance.timeOrigin+performance.now()<end)await new Promise(resolve=>setTimeout(resolve,Math.max(0,end-performance.timeOrigin-performance.now())));
  assert.equal(Atomics.load(new Int32Array(f.grantBroker.worker.cancel!),0),0);
  f.releaseRpc();await assert.rejects(operation);await f.app.close();
  const effects=await h.wait('effects');assert.equal(effects.dispatched,0);assert.equal(effects.writeCount,0);
 }finally{try{await f?.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

test('BrowserData_ClockFailureBeforeEvidenceCannotPublish_008',async()=>{
 const p=platform(),h=await host();let f:Awaited<ReturnType<typeof client>>|undefined;
 try{f=await client(h,{configuration:{peerInstall:{budgetMs:1000}},worker:{clockFaultAtEvidence:true}});await f.next('clockFaultBeforeEvidence');await f.next('retired');const observed=await f.observe();assert.equal(observed.evidenced,false);assert.equal(observed.managementPublished,false);assert.equal(observed.peerPublished,false);assert.equal(f.presentations(),0);assert.equal(p.calls(),0);}
 finally{try{await f?.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

// 独立 TEST_ONLY 批准记录先由 Host 原 scope 和固定公共向量构造，候选绝不回填供应端。
async function pairingSupply(server:Awaited<ReturnType<typeof host>>,mutate?:(candidate:any)=>void,manual=false){
 const b64=(v:Uint8Array)=>Buffer.from(v).toString('base64url');
 const identity={origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:b64(credentialId)};
 const trusted:PublicBrowserPairingConfiguration={version:1,profile:identity,localSpki:b64(tls.spki),hostSpki:b64(Buffer.from(server.hostSpki,'hex')),
  trust:{hostKeyEpoch:'9007199254740993',clientKeyEpoch:'18446744073709551615',origin:'wss://relay.invalid/'},
  grant:{issuerSpki:b64(issuer.spki),principal:'04'.repeat(32),scope:{...server.scope},role:'Controller',budgetMs:1800,peerInstall:{budgetMs:1000,...(manual?{controlLease:'manual-v1' as const}:{})},profile:{...identity,credentialSpki:b64(credential.spki),backupEligible:false,allowBackedUp:false}},
  relay:{endpoint:'wss://relay.invalid/',tunnel:b64(new Uint8Array(16).fill(9)),keyId:b64(new Uint8Array(16).fill(7))},
  artifacts:{wasm:'https://desk.invalid/fixture_bg.wasm',glue:'https://desk.invalid/fixture_bg.js'}};
 const configuration=new TextEncoder().encode(JSON.stringify(trusted));
 const candidate=JSON.parse(JSON.stringify(trusted));mutate?.(candidate);
 const key=(jwk:JsonWebKey)=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 const calls={supplier:0,connect:0,present:0,relaySign:0},signal=new AbortController().signal;
 const handles={tlsKey:await key(tls.jwk),grantKey:await key(issuer.jwk),relaySign:async()=>{calls.relaySign++;throw Error('TEST_ONLY relay not part of data fixture');},present:async(value:Readonly<Record<string,unknown>>)=>{calls.present++;assert.deepEqual(value.scope,trusted.grant.scope);assert.equal(value.principalId,trusted.grant.principal);}};
 const read=createApprovedBrowserProfileReader(new TextEncoder().encode(JSON.stringify(candidate)),{async readApproved(received){calls.supplier++;assert.equal(arguments.length,1);assert.equal(received,signal);return{configuration,handles};}});
 return{calls,handles,signal,read,async connect(){const approved=await read(signal);assert.ok(approved);assert.equal(approved.profile.tlsKey,handles.tlsKey);assert.equal(approved.profile.grantKey,handles.grantKey);assert.equal(approved.present,handles.present);assert.equal(approved.relay.identity.sign,handles.relaySign);assert.deepEqual(approved.grant.scope,server.scope);calls.connect++;return client(server,{},approved);}};
}

test('BrowserData_PairingSupplierOriginalHostRoundtrip_009',async()=>{
 const p=platform(),h=await host();let f:Awaited<ReturnType<typeof client>>|undefined;
 try{
  const supply=await pairingSupply(h);f=await supply.connect();assert.equal(f.profile.tlsKey,supply.handles.tlsKey);assert.equal(f.profile.grantKey,supply.handles.grantKey);const descriptor=await f.app.descriptor;
  const connection=nativeConnection(f.app),terminal=new TerminalClient(descriptor.target,descriptor.binding);terminal.connect(connection);
  await terminal.snapshot();await terminal.window();assert.equal(terminal.phase,'Ready');
  const receipt=await terminal.submit('21'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_SUPPLIER_ONCE'));
  assert.equal(receipt.message,'InputReceipt');assert.equal(receipt.outcome,'HostWritten');
  const queried=await terminal.query();assert.equal(queried.outcome,'HostWritten');assert.equal(queried.proof,receipt.proof);
  await terminal.settle();assert.equal((await terminal.cursor()).pending,null);
  terminal.disconnect();await connection.waitClosed();
  const effects=await h.wait('effects');assert.equal(effects.writeCount,1);assert.equal(effects.bytes,Buffer.from('TEST_ONLY_SUPPLIER_ONCE').toString('hex'));
  assert.equal(supply.calls.supplier,1);assert.equal(supply.calls.connect,1);assert.equal(supply.calls.present,1);assert.equal(p.calls(),1);
 }catch(error){throw Error(JSON.stringify({error:String(error),host:h.messages,worker:f?.messages}));}finally{try{await f?.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

for(const mismatch of ['session','run','generation','hostSpki'])test('BrowserData_PairingSupplierRejectsBeforeConnect_'+mismatch,async()=>{
 const p=platform(),h=await host();let f:Awaited<ReturnType<typeof client>>|undefined;
 const originalSign=crypto.subtle.sign;let signatures=0;
 try{
  const supply=await pairingSupply(h,candidate=>{if(mismatch==='hostSpki')candidate.hostSpki=Buffer.from(issuer.spki).toString('base64url');else candidate.grant.scope[mismatch]=mismatch==='session'?'ff'.repeat(16):mismatch==='run'?'PUBLIC_TEST_OTHER_RUN':'8';});
  crypto.subtle.sign=(...args)=>{signatures++;return originalSign.apply(crypto.subtle,args);};
  await assert.rejects(async()=>{f=await supply.connect();},/browser pairing profile unavailable/);
  await assert.rejects(supply.read(new AbortController().signal),/browser pairing profile unavailable/);
  assert.deepEqual(supply.calls,{supplier:1,connect:0,present:0,relaySign:0});assert.equal(signatures,0);assert.equal(p.calls(),0);
 }finally{crypto.subtle.sign=originalSign;try{await f?.dispose();}finally{try{await h.dispose();}finally{p();}}}
});

// 真实 WASM/TLS/认证与原应用 owner；平台事件来自 Node 测试替身，不是手机验收。
test('BrowserControlLease_AuthenticatedWasmApplicationMode_001',async()=>{
 const p=platform(),h=await host('manual');let f:Awaited<ReturnType<typeof client>>|undefined;
 try{f=await client(h,{configuration:{peerInstall:{budgetMs:1000,controlLease:'manual-v1'}}});const descriptor=await f.app.descriptor;assert.equal(descriptor.role,'Controller');await f.app.close();await f.retired;const effects=await h.wait('effects');assert.equal(effects.writeCount,0);}finally{await f?.dispose();await h.dispose();p();}
});
test('BrowserControlLease_AuthenticatedModeMismatchCloses_002',async(t)=>{
 for(const serverManual of [false,true])await t.test(String(serverManual),async()=>{
  const p=platform(),h=await host(serverManual?'manual':'normal');let f:Awaited<ReturnType<typeof client>>|undefined;
  try{f=await client(h,{configuration:{peerInstall:{budgetMs:1000,...(serverManual?{}:{controlLease:'manual-v1'})}}});await assert.rejects(f.app.descriptor);assert.equal(f.presentations(),0);await f.retired;}finally{await f?.dispose();await h.dispose();p();}
 });
});

test('BrowserControlLease_ExplicitPairingPreservedAndLifecycleCloses_003',async(t)=>{
 for(const event of ['visibilitychange','pagehide','freeze'])await t.test(event,async()=>{
  const p=platform(),h=await host('manual');let f:Awaited<ReturnType<typeof client>>|undefined;
  try{const supply=await pairingSupply(h,undefined,true);f=await supply.connect();await f.app.descriptor;if(event==='visibilitychange')p.hidden();else p.lifecycle(event);await f.retired;const effects=await h.wait('effects');assert.equal(effects.writeCount,0);}finally{await f?.dispose();await h.dispose();p();}
 });
});

test('BrowserLeaseOwner_RealWasmManualRenewalAndSettlement_001',async()=>{
 const p=platform(),h=await host('manual');let f:Awaited<ReturnType<typeof client>>|undefined;
 try{
  f=await client(h,{configuration:{peerInstall:{budgetMs:1000,controlLease:'manual-v1'}}});const descriptor=await f.app.descriptor,connection=nativeConnection(f.app),terminal=new TerminalClient(descriptor.target,descriptor.binding);terminal.connect(connection);
  const lease=await terminal.leaseState(),first=await terminal.renewLease();assert.equal(first.lease,lease.lease);assert.equal(first.fence,descriptor.fence);assert.equal(first.renewalSequence,lease.nextRenewalSequence);assert.ok(BigInt(first.notAfterHostMs)>=BigInt(lease.notAfterHostMs));
  await terminal.snapshot();await terminal.window();const receipt=await terminal.submit('32'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_MANUAL_LEASE'));assert.equal(receipt.message,'InputReceipt');assert.equal(receipt.outcome,'HostWritten');
  const second=await terminal.renewLease();assert.equal(second.renewalSequence,String(BigInt(first.renewalSequence)+1n));assert.equal(terminal.phase,'NeedsSettlement');assert.equal((await terminal.query()).proof,receipt.proof);await terminal.settle();assert.equal(terminal.phase,'Idle');terminal.disconnect();await connection.waitClosed();const effects=await h.wait('effects');assert.equal(effects.writeCount,1);assert.equal(effects.bytes,Buffer.from('TEST_ONLY_MANUAL_LEASE').toString('hex'));
 }finally{await f?.dispose();await h.dispose();p();}
});
test('BrowserLeaseOwner_RealLegacyExpiryAndCancellationFailClosed_002',async(t)=>{
 for(const variant of ['legacy','expiry','cancel'])await t.test(variant,async()=>{
  const p=platform(),h=await host(variant==='legacy'?'normal':variant==='expiry'?'manual-expired':'manual');let f:Awaited<ReturnType<typeof client>>|undefined;
  try{f=await client(h,{configuration:{peerInstall:{budgetMs:1000,...(variant==='legacy'?{}:{controlLease:'manual-v1'})}}});const descriptor=await f.app.descriptor,connection=nativeConnection(f.app),terminal=new TerminalClient(descriptor.target,descriptor.binding);terminal.connect(connection);
   if(variant==='legacy')await assert.rejects(terminal.leaseState());else{await terminal.leaseState();if(variant==='cancel')p.lifecycle('freeze');await f.retired;await assert.rejects(terminal.renewLease());}
   assert.equal(terminal.phase,'NotConnected');await connection.waitClosed();const effects=await h.wait('effects');assert.equal(effects.writeCount,0);
  }finally{await f?.dispose();await h.dispose();p();}
 });
});

// 真实五秒节拍；Snapshot 是显式测试活动，不是 outer 心跳或纯空闲验收。
test('AutoLease_RealWasmFiveSecondTickAndOriginalRetirement_001',async()=>{
 const p=platform(),h=await host('manual-auto');let f:Awaited<ReturnType<typeof client>>|undefined,terminal:TerminalClient|undefined;
 try{
  f=await client(h,{configuration:{peerInstall:{budgetMs:1000,controlLease:'manual-v1'}}});const descriptor=await f.app.descriptor,original=f.app,renewals:string[]=[];
  const connection=nativeConnection({...original,exchange:(bytes,budget)=>{const value=JSON.parse(new TextDecoder().decode(bytes));if(value.message==='LeaseRenew')renewals.push(value.renewalSequence);return original.exchange(bytes,budget);}});terminal=new TerminalClient(descriptor.target,descriptor.binding);terminal.connect(connection);await terminal.leaseState();await terminal.startAutoRenewal();const began=performance.now();
  while(performance.now()-began<5200){if(terminal.phase!=='Busy'){try{await terminal.snapshot();}catch(error){if(terminal.phase!=='Busy')throw error;}}await new Promise(r=>setTimeout(r,250));}
  assert.deepEqual(renewals,['1','2']);assert.equal(terminal.autoRenewalState,'Waiting');await terminal.window();const receipt=await terminal.submit('34'.repeat(16),'1',new TextEncoder().encode('TEST_ONLY_AUTO_TICK'));assert.equal(receipt.message,'InputReceipt');assert.equal(receipt.outcome,'HostWritten');assert.equal(terminal.phase,'NeedsSettlement');await terminal.settle();await terminal.stopAutoRenewal();assert.equal(terminal.autoRenewalState,'Stopped');const effects=await h.wait('effects');assert.equal(effects.writeCount,1);assert.equal(effects.bytes,Buffer.from('TEST_ONLY_AUTO_TICK').toString('hex'));
 }finally{terminal?.disconnect();await f?.dispose();await h.dispose();p();}
});

test('BrowserLiveControl_BareTlsWithoutOuterActivationRefusesPublication_001',async()=>{
 const p=platform(),h=await host('live');let f:Awaited<ReturnType<typeof client>>|undefined;
 try{f=await client(h,{configuration:{peerInstall:{budgetMs:1000,controlLease:'live-v1'}}});await assert.rejects(f.app.descriptor);await f.app.close();await f.retired;const effects=await h.wait('effects');assert.equal(effects.writeCount,0);}finally{await f?.dispose();await h.dispose();p();}
});
test('BrowserLiveControl_AuthenticatedModeMismatchCloses_002',async(t)=>{
 for(const serverManual of [false,true])await t.test(String(serverManual),async()=>{
  const p=platform(),h=await host(serverManual?'live':'manual');let f:Awaited<ReturnType<typeof client>>|undefined;
  try{f=await client(h,{configuration:{peerInstall:{budgetMs:1000,controlLease:serverManual?'manual-v1':'live-v1'}}});await assert.rejects(f.app.descriptor);assert.equal(f.presentations(),0);await f.retired;}finally{await f?.dispose();await h.dispose();p();}
 });
});
