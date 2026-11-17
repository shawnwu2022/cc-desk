import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {Worker as NodeWorker,MessageChannel as NodeChannel} from 'node:worker_threads';
import {createECDH,createPrivateKey,createHash,sign} from 'node:crypto';
import {createRequire} from 'node:module';
import {browserConnectionSource} from './browserSource.ts';
import {createApprovedBrowserProfileReader} from './browserPairingProfile.ts';
import {createBrowserEnrollmentAuthority} from './browserEnrollment.ts';
import {TerminalClient} from './client.ts';
const config=JSON.parse(process.argv[2]);
const Ws=createRequire(import.meta.url)('ws');
const scalar=Buffer.alloc(32);scalar[31]=1;const e=createECDH('prime256v1');e.setPrivateKey(scalar);const point=e.getPublicKey();const credential={jwk:{kty:'EC',crv:'P-256',x:point.subarray(1,33).toString('base64url'),y:point.subarray(33).toString('base64url'),d:scalar.toString('base64url')},spki:Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),point])},credentialId=new Uint8Array(32).fill(7);
function platform(options:{mutation?:string;hold?:boolean;delay?:number}={}){const saved=new Map<string,PropertyDescriptor|undefined>();class WindowFixture{static [Symbol.hasInstance](v:unknown){return v===globalThis;}}class Assertion{}class Credential{type='public-key';rawId=credentialId.slice().buffer;response!:Assertion;}let calls=0,release:()=>void=()=>{},began!:()=>void,previous:Credential|undefined,replay=false;const started=new Promise<void>(resolve=>{began=resolve;});
 class DocumentFixture extends EventTarget{visibilityState='visible';}const documentSource=new DocumentFixture(),windowEvents=new EventTarget();
 const credentials={async get(request:CredentialRequestOptions){calls++;began();if(replay&&previous)return previous;const publicKey=request.publicKey!;const challengeBytes=Buffer.from(publicKey.challenge as ArrayBuffer);if(options.mutation==='challenge')challengeBytes[0]^=1;const challenge=challengeBytes.toString('base64url'),client=Buffer.from(JSON.stringify({type:'webauthn.get',challenge,origin:options.mutation==='origin'?'https://evil.invalid':'https://desk.invalid',crossOrigin:false}));const auth=Buffer.concat([createHash('sha256').update('desk.invalid').digest(),Buffer.from([options.mutation==='uv'?1:5,0,0,0,0])]);const signature=sign('sha256',Buffer.concat([auth,createHash('sha256').update(client).digest()]),createPrivateKey({key:credential.jwk,format:'jwk'}));if(options.mutation==='signature')signature[signature.length-1]^=1;const response=new Assertion();Object.assign(response,{authenticatorData:Uint8Array.from(auth).buffer,clientDataJSON:Uint8Array.from(client).buffer,signature:Uint8Array.from(signature).buffer});const value=new Credential();value.response=response;if(options.mutation==='credential')value.rawId=new Uint8Array(32).fill(6).buffer;previous=value;if(options.delay)await new Promise(resolve=>setTimeout(resolve,options.delay));
  // 合成credentials故意不响应Abort，用于证明原get Promise没有被假取消。
  if(options.hold)await new Promise<void>(resolve=>{release=resolve;});return value;}};
 for(const[n,v]of Object.entries({Document:DocumentFixture,document:documentSource,addEventListener:windowEvents.addEventListener.bind(windowEvents),removeEventListener:windowEvents.removeEventListener.bind(windowEvents),dispatchEvent:windowEvents.dispatchEvent.bind(windowEvents),Window:WindowFixture,isSecureContext:true,crossOriginIsolated:true,location:{origin:'https://desk.invalid'},self:globalThis,top:globalThis,PublicKeyCredential:Credential,AuthenticatorAssertionResponse:Assertion,navigator:{credentials}})){saved.set(n,Object.getOwnPropertyDescriptor(globalThis,n));Object.defineProperty(globalThis,n,{configurable:true,value:v});}return Object.assign(()=>{for(const[n,d]of saved){if(d)Object.defineProperty(globalThis,n,d);else Reflect.deleteProperty(globalThis,n);}},{hidden:()=>{documentSource.visibilityState='hidden';documentSource.dispatchEvent(new Event('visibilitychange'));},lifecycle:(name:string)=>{(name==='freeze'?documentSource:windowEvents).dispatchEvent(new Event(name));},calls:()=>calls,started,release:()=>release(),replay:()=>{replay=true;}});}

const phonePoint=Buffer.from('04bac5b11cad8f99f9c72b05cf4b9e26d244dc189f745228255a219a86d6a09eff20138bf82dc1b6d562be0fa54ab7804a3a64b6d72ccfed6b6fb6ed28bbfc117e','hex'),phoneJwk={kty:'EC',crv:'P-256',x:phonePoint.subarray(1,33).toString('base64url'),y:phonePoint.subarray(33).toString('base64url'),d:'V8kgd2ZBRuh2dgyVINBUqpPDr7BOMGcF22CQMIUHtNM'},phoneKey=createPrivateKey({format:'jwk',key:phoneJwk});

const restore=platform(),saved=new Map<string,PropertyDescriptor|undefined>();
const sockets:WebSocket[]=[],workers:NodeWorker[]=[],exits:Promise<number>[]=[];
const requests:string[]=[];let presentations=0,retired=0,terminated=0;
// 仅适配 Node 平台；生产 Source 自己创建并拥有 socket、端口、broker 和 Worker。
class Socket extends Ws {
 constructor(url:string,protocols?:string|string[]){super(url,protocols,{origin:'https://desk.invalid',ca:readFileSync(process.env.CC_DESK_TEST_RELAY_ROOT+'/tests/fixtures/root.pem'),rejectUnauthorized:true,servername:'foobar.com',lookup:(_h:unknown,_o:unknown,cb:any)=>cb(null,[{address:'127.0.0.1',family:4}])});sockets.push(this as unknown as WebSocket);this.on('error',(e:Error)=>console.error('TEST_ONLY_SOURCE_SOCKET',e.message));}
}
class Channel extends NodeChannel {
 constructor(){super();const original=this.port1.postMessage.bind(this.port1);this.port1.postMessage=((value:any,...rest:any[])=>{if(['snapshot','snapshotPull','exchange'].includes(value?.kind))requests.push(value.kind);return original(value,...rest as [any]);}) as typeof this.port1.postMessage;}
}
class WorkerBridge extends EventTarget {
 original?:NodeWorker;
 constructor(url:URL){super();assert.ok(url.pathname.endsWith('/browserEntry.worker.ts'));}
 postMessage(value:any,_transfer?:unknown[]){
  if(value.kind==='close'){this.original?.postMessage(value);return;}
  assert.equal(value.kind,'start');assert.equal(this.original,undefined);
  const original=new NodeWorker(new URL('./browserTlsWorker.fixture.ts',import.meta.url),{workerData:{transportPort:value.transport,application:value.application,wasm:readFileSync(process.env.CC_DESK_TEST_ONLY_DATA_WASM!),glueUrl:pathToFileURL(process.env.CC_DESK_TEST_ONLY_DATA_GLUE!).href,bridge:value.tls,grantBridge:value.grant,localSpki:value.localSpki,peerSpki:value.hostSpki,clientTrust:value.trust},transferList:[value.transport,value.application,value.tls.port,value.grant.port]});
  this.original=original;workers.push(original);exits.push(new Promise(resolve=>original.once('exit',resolve)));
  original.on('message',message=>{if(message.kind==='retired'){retired++;this.dispatchEvent(new MessageEvent('message',{data:message}));}else if(['retireFailed','startupError','runtimeTrace'].includes(message.kind)){console.error('TEST_ONLY_SOURCE_WORKER',JSON.stringify(message));this.dispatchEvent(new Event('error'));}});
  original.on('error',error=>{console.error(error);this.dispatchEvent(new Event('error'));});
 }
 terminate(){terminated++;return this.original?.terminate();}
}
for(const [name,value] of Object.entries({WebSocket:Socket,Worker:WorkerBridge,MessageChannel:Channel})){saved.set(name,Object.getOwnPropertyDescriptor(globalThis,name));Object.defineProperty(globalThis,name,{configurable:true,value});}
const controller=new AbortController(),enrollment=createBrowserEnrollmentAuthority(async()=>{enrollment.revoke();await enrollment.waitClosed();});
let connection:Awaited<ReturnType<ReturnType<typeof browserConnectionSource>['open']>>;
const timer=setTimeout(()=>controller.abort(),14000);
try{
 const tlsKey=await crypto.subtle.importKey('jwk',phoneJwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 const grantKey=await crypto.subtle.importKey('jwk',credential.jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign']);
 const b64=(bytes:Uint8Array)=>Buffer.from(bytes).toString('base64url');
 const publicConfig={version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:b64(credentialId)},localSpki:b64(Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),phonePoint])),hostSpki:b64(Buffer.from(config.hostSpki,'hex')),trust:{hostKeyEpoch:'5',clientKeyEpoch:'7',origin:config.origin},grant:{issuerSpki:b64(credential.spki),principal:'04'.repeat(32),scope:config.scope,role:'Controller',budgetMs:7000,peerInstall:{budgetMs:3000,controlLease:'live-v1',snapshotPull:'snapshot-pull-v1'},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:b64(credentialId),credentialSpki:b64(credential.spki),backupEligible:false,allowBackedUp:false}},relay:{endpoint:config.origin,tunnel:b64(new Uint8Array(16).fill(3)),keyId:b64(new Uint8Array(16).fill(2))},artifacts:{wasm:'https://desk.invalid/test.wasm',glue:'https://desk.invalid/test.js'}};
 const bytes=new TextEncoder().encode(JSON.stringify(publicConfig));
 await enrollment.approve(async()=>({configuration:bytes,handles:{tlsKey,grantKey,relaySign:async request=>Uint8Array.from(sign('sha256',request.transcript,{key:phoneKey,dsaEncoding:'ieee-p1363'})),present:async()=>{presentations++;}}}));
 const input=await createApprovedBrowserProfileReader(bytes,enrollment.provider)(controller.signal);
 assert.equal(input?.grant.peerInstall?.snapshotPull,'snapshot-pull-v1');
 connection=await browserConnectionSource(input).open(controller.signal);
 assert.ok(connection);const terminal=new TerminalClient(connection.target,connection.binding);terminal.connect(connection.connection);
 await terminal.snapshot();assert.equal(terminal.frame?.cells.length,1920);
 assert.equal(requests[0],'snapshotPull');assert.equal(requests.includes('snapshot'),false);
 await terminal.window();const receipt=await terminal.submit('42'.repeat(16),'1',new Uint8Array([110]));assert.equal(receipt.message,'InputReceipt');assert.equal(receipt.outcome,'HostWritten');await terminal.settle();assert.equal(terminal.phase,'Idle');
 connection.connection.close();await connection.connection.waitClosed();await enrollment.waitClosed();await Promise.all(exits);
 assert.equal(workers.length,1);assert.equal(retired,1);assert.equal(terminated,1);assert.equal(sockets.length,1);assert.equal(sockets[0].readyState,3);assert.equal(presentations,1);
 console.log(JSON.stringify({kind:'short-input',productionSource:true,sourceSnapshotKind:requests[0],remoteBytes:1,cells:1920,originalRetired:true,workerRetired:retired,workerTerminated:terminated,socketClosed:true,enrollmentRetired:true}));
}finally{
 console.error('TEST_ONLY_SOURCE_REQUESTS',JSON.stringify(requests));
 clearTimeout(timer);controller.abort();enrollment.revoke();connection?.connection.close();await connection?.connection.waitClosed().catch(()=>{});await enrollment.waitClosed().catch(()=>{});await Promise.all(workers.map(worker=>worker.terminate()));
 for(const[name,descriptor]of saved){if(descriptor)Object.defineProperty(globalThis,name,descriptor);else Reflect.deleteProperty(globalThis,name);}restore();
}
