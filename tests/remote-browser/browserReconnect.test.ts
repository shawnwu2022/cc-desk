// @vitest-environment node
import {beforeAll,afterEach,expect,test,vi} from 'vitest'
import {readFileSync} from 'node:fs'
import {createECDH} from 'node:crypto'
import {browserReconnectSession} from '../../remote/ui-terminal/src/browserReconnect'
import {createBrowserEnrollmentLease} from '../../remote/ui-terminal/src/browserEnrollmentLease'
import type {ApprovedBrowserInputs} from '../../remote/ui-terminal/src/browserEntry'
const state=vi.hoisted(()=>({port:undefined as any,owners:[] as any[]}))
const owner=()=>{const value={ready:Promise.resolve({}),close:vi.fn(),waitClosed:vi.fn(async()=>{}),worker:{port:{},cancel:new SharedArrayBuffer(4)}};state.owners.push(value);return value}
vi.mock('../../remote/ui-terminal/src/browserRelay.ts',()=>({RENDEZVOUS_SUBPROTOCOL:'test',createBrowserRelay:()=>owner(),createBrowserDataKeepaliveRelay:()=>({owner:owner(),activation:{}})}))
vi.mock('../../remote/ui-terminal/src/browserTransport.ts',()=>({serveBrowserTransport:()=>owner()}))
vi.mock('../../remote/ui-terminal/src/browserTlsSigner.ts',()=>({createBrowserTlsSignerBroker:()=>owner()}))
vi.mock('../../remote/ui-terminal/src/browserGrant.ts',async importOriginal=>({...await importOriginal<typeof import('../../remote/ui-terminal/src/browserGrant')>(),createBrowserGrantBroker:()=>owner()}))
vi.mock('../../remote/ui-terminal/src/browserClient.ts',()=>({browserClientPort:()=>state.port}))
let key:CryptoKey,grantKey:CryptoKey
beforeAll(async()=>{const keys:CryptoKey[]=[];for(const scalar of [1,2]){const d=Buffer.alloc(32);d[31]=scalar;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();keys.push(await crypto.subtle.importKey('jwk',{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},{name:'ECDSA',namedCurve:'P-256'},false,['sign']))};[key,grantKey]=keys})
afterEach(()=>vi.unstubAllGlobals())
function fixture(){
 state.owners=[];const sockets:any[]=[],workers:any[]=[]
 class Doc extends EventTarget{visibilityState='visible'}
 class Win{static [Symbol.hasInstance](v:unknown){return v===globalThis}}
 class Socket extends EventTarget{readyState=1;constructor(){super();sockets.push(this)}close(){this.readyState=3;this.dispatchEvent(new Event('close'))}}
 class Worker extends EventTarget{terminated=0;constructor(){super();workers.push(this)}postMessage(v:any){if(v.kind==='close')queueMicrotask(()=>this.dispatchEvent(new MessageEvent('message',{data:{kind:'retired'}})))}terminate(){this.terminated++}}
 class Channel{port1={};port2={}}
 const events=new EventTarget()
 for(const[n,v]of Object.entries({Window:Win,Document:Doc,document:new Doc(),top:globalThis,self:globalThis,isSecureContext:true,crossOriginIsolated:true,location:{origin:'https://desk.invalid'},WebSocket:Socket,Worker,MessageChannel:Channel,addEventListener:events.addEventListener.bind(events),removeEventListener:events.removeEventListener.bind(events)}))vi.stubGlobal(n,v)
 const snapshot=JSON.parse(readFileSync('protocol/remote/inner/v1/test-vectors/projection-snapshot.json','utf8'))
 const ports:any[]=[];let failedClose=false;
 const makePort=()=>{const item=structuredClone(snapshot);item.body.stamp.syncEpoch=String(BigInt(item.body.stamp.syncEpoch)+BigInt(ports.length));item.body.stamp.binding=(ports.length+17).toString(16).padStart(2,'0').repeat(32);const batch={stamp:item.body.stamp,parts:[new TextEncoder().encode(JSON.stringify(item))]};const listeners=new Set<()=>void>();const port={descriptor:Promise.resolve({target:batch.stamp.target,binding:batch.stamp.binding,role:'Controller'}),snapshotPull:vi.fn(async()=>batch),snapshot:vi.fn(async()=>batch),
 exchange:vi.fn(async(bytes:Uint8Array)=>{const v=JSON.parse(new TextDecoder().decode(bytes));const common={profile:1};let reply:any;
 if(v.message==='ProjectionAck')reply={...common,message:'ProjectionAcknowledged',stamp:v.stamp};
 else if(v.message==='Window')reply={...common,message:'WindowGranted',request:v.request,window:'08'.repeat(32),notAfterHostMs:'6000'};
 else if(v.message==='InputChunk')reply={...common,message:'InputReceipt',operation:v.operation,sequence:v.sequence,outcome:'HostWritten',proof:'aa'.repeat(32)};
 else throw Error('unexpected request');return new TextEncoder().encode(JSON.stringify(reply));}),
 close:vi.fn(async()=>{if(failedClose)throw Error('retirement unknown')}),onInvalidate:(fn:()=>void)=>{listeners.add(fn);return()=>listeners.delete(fn)},invalidate:()=>{for(const fn of [...listeners])fn()}};ports.push(port);state.port=port;return port};makePort();

 const lifetime=createBrowserEnrollmentLease(),retire=vi.fn(async()=>{lifetime.revoke();await lifetime.waitClosed()})
 const input:ApprovedBrowserInputs={enrollment:{...lifetime.lease,retire},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:new Uint8Array([1]),tlsKey:key,grantKey},localSpki:new Uint8Array(91).fill(1),hostSpki:new Uint8Array(91).fill(2),trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:new Uint8Array(91).fill(3),principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000,controlLease:'live-v1',snapshotPull:'snapshot-pull-v1'},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:new Uint8Array([1]),credentialSpki:new Uint8Array(91).fill(4),backupEligible:false,allowBackedUp:false}},relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:new Uint8Array(16).fill(9),identity:{keyId:new Uint8Array(16).fill(8),sign:async()=>new Uint8Array(64)}},artifacts:{wasm:'https://desk.invalid/client.wasm',glue:'https://desk.invalid/client.js'},present:async()=>{}}
 return {input,lifetime,retire,sockets,workers,ports,makePort,failClose:()=>{failedClose=true}};
}
async function until(check:()=>boolean){const end=performance.now()+3000;while(!check()){if(performance.now()>end)throw Error('test wait');await new Promise(r=>setTimeout(r,5))}}
const policy={mode:'safe-idle-retry-v1' as const,budgetMs:10000};

test('BrowserRetry_OriginalSourceChildDrainThenSameClientNextGeneration_001',async()=>{
 const f=fixture(),owner=browserReconnectSession(f.input,policy,new AbortController().signal);
 try{const client=await owner.start(),deadline=owner.observe().localDeadline;
 f.makePort();f.ports[0].invalidate();await until(()=>owner.observe().generation===2&&owner.observe().state==='Connected');
 expect(owner.observe().client).toBe(client);expect(owner.observe().localDeadline).toBe(deadline);expect(f.retire).not.toHaveBeenCalled();expect(f.lifetime.lease.signal.aborted).toBe(false);
 expect(f.workers).toHaveLength(2);expect(f.workers[0].terminated).toBe(1);expect(owner.observe().replacements).toBe(1);
 }finally{await owner.close()}
 expect(f.retire).toHaveBeenCalledTimes(1);await owner.waitClosed();expect(f.workers.every(w=>w.terminated===1)).toBe(true);
});

test('BrowserRetry_HostWrittenWithoutSettlementDoesNotReopenOrReplay_002',async()=>{
 const f=fixture(),owner=browserReconnectSession(f.input,policy,new AbortController().signal);
 try{const client=await owner.start();await client.window();await client.submit('07'.repeat(16),'1',new Uint8Array([120]));f.ports[0].invalidate();await owner.waitClosed();
 expect(owner.observe().state).toBe('Closed');expect(owner.observe().replacements).toBe(0);expect(f.workers).toHaveLength(1);expect(client.canReconnectFrom({} as any)).toBe(false);
 expect(f.ports[0].exchange.mock.calls.filter((v:any)=>JSON.parse(new TextDecoder().decode(v[0])).message==='InputChunk')).toHaveLength(1);
 }finally{await owner.close()}
});

test('BrowserRetry_CancelBackoffNeverRefundsAttemptOrReopens_003',async()=>{
 const f=fixture(),cancel=new AbortController(),owner=browserReconnectSession(f.input,policy,cancel.signal);
 await owner.start();f.makePort();f.ports[0].invalidate();await until(()=>owner.observe().state==='Waiting');cancel.abort();await owner.waitClosed();
 expect(owner.observe().replacements).toBe(1);expect(f.workers).toHaveLength(1);expect(owner.observe().state).toBe('Closed');
});

test('BrowserRetry_UnknownOriginalDrainPreventsReplacement_004',async()=>{
 const f=fixture(),owner=browserReconnectSession(f.input,policy,new AbortController().signal);await owner.start();f.failClose();f.ports[0].invalidate();await expect(owner.waitClosed()).rejects.toThrow();
 expect(owner.observe().state).toBe('Unknown');expect(owner.observe().replacements).toBe(0);expect(f.workers).toHaveLength(1);
});


test('BrowserRetry_LateOpenCleanupStaysOwnedUntilOriginalResult_005',async()=>{
 const f=fixture(),descriptor=await f.ports[0].descriptor;let release!:(value:any)=>void;f.ports[0].descriptor=new Promise(r=>{release=r});
 const owner=browserReconnectSession(f.input,policy,new AbortController().signal),opening=owner.start();const rejected=expect(opening).rejects.toThrow();
 await until(()=>f.workers.length===1);let done=false;const closing=owner.close().then(()=>{done=true});await new Promise(r=>setTimeout(r,20));expect(done).toBe(false);
 release(descriptor);await rejected;await closing;await owner.waitClosed();expect(f.retire).toHaveBeenCalledTimes(1);expect(f.workers[0].terminated).toBe(1);expect(owner.observe().state).toBe('Closed');
});

test('BrowserRetry_NewSetupFailureDoesNotLoopAuthentication_006',async()=>{
 const f=fixture(),owner=browserReconnectSession(f.input,policy,new AbortController().signal);await owner.start();const next=f.makePort();next.snapshotPull.mockRejectedValue(Error('new authentication/application failure'));f.ports[0].invalidate();await owner.waitClosed();
 expect(owner.observe().state).toBe('Closed');expect(owner.observe().replacements).toBe(1);expect(f.workers).toHaveLength(2);expect(f.retire).toHaveBeenCalledTimes(1);
});

test('BrowserRetry_ThreeReplacementBudgetNeverResets_007',async()=>{
 const f=fixture(),owner=browserReconnectSession(f.input,policy,new AbortController().signal);const client=await owner.start(),deadline=owner.observe().localDeadline;
 try{for(let i=0;i<3;i++){f.makePort();f.ports[i].invalidate();await until(()=>owner.observe().generation===i+2&&owner.observe().state==='Connected');expect(owner.observe().client).toBe(client);expect(owner.observe().localDeadline).toBe(deadline)}
 f.ports[3].invalidate();await owner.waitClosed();expect(owner.observe().replacements).toBe(3);expect(f.workers).toHaveLength(4);
 }finally{await owner.close()}
});

test('BrowserRetry_FixedLocalDeadlineAndParentRevokePreventNewGeneration_008',async()=>{
 for(const revoke of [false,true]){const f=fixture(),owner=browserReconnectSession(f.input,{...policy,budgetMs:100},new AbortController().signal);await owner.start();
 if(revoke)f.lifetime.revoke();else{f.makePort();f.ports[0].invalidate()}
 await owner.waitClosed();expect(owner.observe().state).toBe('Closed');expect(f.workers).toHaveLength(1)}
});


test('BrowserRetry_LegacyCannotAcquireAutomaticRetry_009',async()=>{
 for(const mode of [undefined,'legacy'] as const){
  const f=fixture();const input={...f.input,relay:{...f.input.relay,mode}};
  let owner:ReturnType<typeof browserReconnectSession>|undefined,rejected=false;
  try{owner=browserReconnectSession(input,policy,new AbortController().signal)}catch{rejected=true}
  if(owner)await owner.close();
  expect(rejected).toBe(true);expect(f.sockets).toHaveLength(0);expect(f.workers).toHaveLength(0);
 }
});
