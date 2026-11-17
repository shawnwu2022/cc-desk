// @vitest-environment node
import {beforeAll,afterEach,expect,test,vi} from 'vitest'
import {readFileSync} from 'node:fs'
import {createECDH} from 'node:crypto'
import {browserConnectionSource} from '../../remote/ui-terminal/src/browserSource'
import {createBrowserEnrollmentLease} from '../../remote/ui-terminal/src/browserEnrollmentLease'
import {command} from '../../remote/ui-terminal/src/wire'
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
for(const mode of ['legacy','pull','reject'] as const)test(`BrowserSource_OriginalCloseOwnership_${mode}`,async()=>{
 state.owners=[];const sockets:any[]=[],workers:any[]=[]
 class Doc extends EventTarget{visibilityState='visible'}
 class Win{static [Symbol.hasInstance](v:unknown){return v===globalThis}}
 class Socket extends EventTarget{readyState=1;constructor(){super();sockets.push(this)}close(){this.readyState=3;this.dispatchEvent(new Event('close'))}}
 class Worker extends EventTarget{terminated=0;constructor(){super();workers.push(this)}postMessage(v:any){if(v.kind==='close')queueMicrotask(()=>this.dispatchEvent(new MessageEvent('message',{data:{kind:'retired'}})))}terminate(){this.terminated++}}
 class Channel{port1={};port2={}}
 const events=new EventTarget()
 for(const[n,v]of Object.entries({Window:Win,Document:Doc,document:new Doc(),top:globalThis,self:globalThis,isSecureContext:true,crossOriginIsolated:true,location:{origin:'https://desk.invalid'},WebSocket:Socket,Worker,MessageChannel:Channel,addEventListener:events.addEventListener.bind(events),removeEventListener:events.removeEventListener.bind(events)}))vi.stubGlobal(n,v)
 const snapshot=JSON.parse(readFileSync('protocol/remote/inner/v1/test-vectors/projection-snapshot.json','utf8'))
 const batch={stamp:snapshot.body.stamp,parts:[new TextEncoder().encode(JSON.stringify(snapshot))]}
 state.port={descriptor:Promise.resolve({target:batch.stamp.target,binding:batch.stamp.binding,role:'Controller'}),snapshot:vi.fn(async()=>batch),snapshotPull:vi.fn(async()=>{if(mode==='reject')throw Error('authenticated pull refused');return batch}),exchange:vi.fn(),close:vi.fn(async()=>{}),onInvalidate:()=>()=>{}}
 const lifetime=createBrowserEnrollmentLease(),retire=vi.fn(async()=>{lifetime.revoke();await lifetime.waitClosed()})
 const input:ApprovedBrowserInputs={enrollment:{...lifetime.lease,retire},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:new Uint8Array([1]),tlsKey:key,grantKey},localSpki:new Uint8Array(91).fill(1),hostSpki:new Uint8Array(91).fill(2),trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:new Uint8Array(91).fill(3),principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000,controlLease:'live-v1',...(mode==='legacy'?{}:{snapshotPull:'snapshot-pull-v1'})},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:new Uint8Array([1]),credentialSpki:new Uint8Array(91).fill(4),backupEligible:false,allowBackedUp:false}},relay:{endpoint:'wss://relay.invalid/route',tunnel:new Uint8Array(16).fill(9),identity:{keyId:new Uint8Array(16).fill(8),sign:async()=>new Uint8Array(64)}},artifacts:{wasm:'https://desk.invalid/client.wasm',glue:'https://desk.invalid/client.js'},present:async()=>{}}
 const opened=await browserConnectionSource(input).open(new AbortController().signal);expect(opened).toBeDefined();const connection=opened!.connection
 try{
  const result=connection.snapshot(command('Snapshot','1'),{begin(){},part(){}})
  if(mode==='reject')await expect(result).rejects.toThrow();else await result
 }finally{connection.close();await connection.waitClosed()}
 expect(state.port.snapshot).toHaveBeenCalledTimes(mode==='legacy'?1:0)
 expect(state.port.snapshotPull).toHaveBeenCalledTimes(mode==='legacy'?0:1)
 expect(sockets).toHaveLength(1);expect(sockets[0].readyState).toBe(3)
 expect(workers).toHaveLength(1);expect(workers[0].terminated).toBe(1)
 expect(retire).toHaveBeenCalledTimes(1);await lifetime.waitClosed()
 for(const item of state.owners){expect(item.close).toHaveBeenCalled();expect(item.waitClosed).toHaveBeenCalled()}
})
