import {test,expect,type Page} from '@playwright/test'
import {build} from 'esbuild'
import {createECDH} from 'node:crypto'
import type {BrowserProfileHandles,PublicBrowserPairingConfiguration} from '../../remote/ui-terminal/src/browserPairingProfile'

type FixtureModule=typeof import('../../remote/ui-terminal/src/browserPairingProfile')&{
 fixture():Promise<{configuration:PublicBrowserPairingConfiguration;bytes:Uint8Array;handles:BrowserProfileHandles}>
}
let helperModule:string,fixtureModule:string
test.use({serviceWorkers:'block'})

test.beforeAll(async()=>{
 const result=await build({entryPoints:['remote/ui-terminal/src/browserPairingProfile.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})
 helperModule=result.outputFiles[0].text
 // 已公开的 TEST_ONLY scalar1/2，与原单元向量相同，不创建或读取账户凭据。
 const keys=[1,2].map(scalar=>{const d=Buffer.alloc(32);d[31]=scalar;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();return {jwk:{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},spki:Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),p]).toString('base64url')}})
 const configuration:PublicBrowserPairingConfiguration={
  version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw'},
  localSpki:keys[0].spki,hostSpki:keys[1].spki,trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},
  grant:{issuerSpki:keys[1].spki,principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},
   profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw',credentialSpki:keys[1].spki,backupEligible:false,allowBackedUp:false}},
  relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},
  artifacts:{wasm:'https://desk.invalid/client.wasm',glue:'https://desk.invalid/client.js'},
 }
 fixtureModule=`export * from '/pairing-helper.mjs';
 export async function fixture(){
  const configuration=${JSON.stringify(configuration)};
  const keys=await Promise.all(${JSON.stringify(keys.map(k=>k.jwk))}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));
  return {configuration,bytes:new TextEncoder().encode(JSON.stringify(configuration)),handles:{tlsKey:keys[0],grantKey:keys[1],relaySign:async()=>{throw Error('TEST_ONLY must not sign')},present:async()=>{throw Error('TEST_ONLY must not present')}}};
 }`
})

async function prepare(page:Page){
 const unexpected:string[]=[],errors:string[]=[]
 page.on('pageerror',error=>errors.push(error.message))
 await page.routeWebSocket('**/*',socket=>{unexpected.push(socket.url());socket.close()})
 await page.route('**/*',async route=>{
  const url=new URL(route.request().url())
  const headers={'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp','Cache-Control':'no-store'}
  if(url.origin==='https://desk.invalid'&&['/','/fixture.mjs','/pairing-helper.mjs'].includes(url.pathname)){
   await route.fulfill({status:200,headers,contentType:url.pathname==='/'?'text/html':'text/javascript',body:url.pathname==='/'?'<!doctype html><title>PUBLIC_TEST_ONLY pairing fixture</title>':url.pathname==='/fixture.mjs'?fixtureModule:helperModule})
  }else{unexpected.push(url.href);await route.abort()}
 })
 await page.goto('https://desk.invalid/')
 expect(await page.evaluate(()=>({secure:isSecureContext,isolated:crossOriginIsolated}))).toEqual({secure:true,isolated:true})
 return ()=>{expect(unexpected).toEqual([]);expect(errors).toEqual([])}
}

// 如果 helper 换句柄、重读、丢失复制隔离或意外签名，本测试会失败。
test('Pairing_ActualEdgeOriginalKeysAndFrozenSnapshot_001',async({page})=>{
 const verify=await prepare(page)
 const result=await page.evaluate(async()=>{
  const api:FixtureModule=await import(location.origin+'/fixture.mjs'),f=await api.fixture()
  const trustedBytes=new TextEncoder().encode(JSON.stringify(f.configuration)),signal=new AbortController().signal
  let reads=0,exactSignal=false,argumentCount=0
  const reader=api.createApprovedBrowserProfileReader(f.bytes,{async readApproved(...args:[AbortSignal]){reads++;argumentCount=args.length;exactSignal=args[0]===signal;return{configuration:trustedBytes,handles:f.handles}}})
  f.bytes.fill(0)
  const [first,second]=await Promise.all([reader(signal),reader(new AbortController().signal)])
  if(!first||!second)throw Error('missing approved inputs')
  const hostByte=first.hostSpki[0];first.hostSpki.fill(0);first.profile.credentialId.fill(0);trustedBytes.fill(0)
  const cached=await reader(new AbortController().signal)
  return {reads,exactSignal,argumentCount,originalKeys:first.profile.tlsKey===f.handles.tlsKey&&first.profile.grantKey===f.handles.grantKey,
   realNonExportable:first.profile.tlsKey instanceof CryptoKey&&!first.profile.tlsKey.extractable&&!first.profile.grantKey.extractable,
   originalCallbacks:first.relay.identity.sign===f.handles.relaySign&&first.present===f.handles.present,
   isolated:second.hostSpki[0]===hostByte&&cached?.hostSpki[0]===hostByte&&cached?.profile.credentialId[0]===7,
   frozen:Object.isFrozen(first)&&Object.isFrozen(first.grant.scope),run:cached?.grant.scope.run}
 })
 expect(result).toEqual({reads:1,exactSignal:true,argumentCount:1,originalKeys:true,realNonExportable:true,originalCallbacks:true,isolated:true,frozen:true,run:'PUBLIC_TEST_ONLY'})
 verify()
})

// 配置语法合法仍不能越过可信快照；失败结果不可借新 signal 重读。
test('Pairing_ActualEdgeMismatchAndFailureRemainLocked_002',async({page})=>{
 const verify=await prepare(page)
 const result=await page.evaluate(async()=>{
  const api:FixtureModule=await import(location.origin+'/fixture.mjs'),f=await api.fixture()
  const changed=structuredClone(f.configuration) as unknown as {grant:{scope:{run:string}}}
  changed.grant.scope.run='OTHER_PUBLIC_TEST_ONLY'
  let reads=0
  const reader=api.createApprovedBrowserProfileReader(new TextEncoder().encode(JSON.stringify(changed)),{async readApproved(){reads++;return{configuration:f.bytes,handles:f.handles}}})
  const message=async(work:Promise<unknown>)=>{try{await work;return 'UNEXPECTED_SUCCESS'}catch(error){return String(error)}}
  const failures=[await message(reader(new AbortController().signal)),await message(reader(new AbortController().signal))]
  let invalid=false
  try{api.parseBrowserPairingConfiguration(new TextEncoder().encode('{"version":1,'+JSON.stringify(f.configuration).slice(1)))}catch{invalid=true}
  let providerReads=0
  const failed=api.createApprovedBrowserProfileReader(f.bytes,{async readApproved(){providerReads++;throw Error('PRIVATE_PROVIDER_DETAIL')}})
  failures.push(await message(failed(new AbortController().signal)),await message(failed(new AbortController().signal)))
  return {reads,providerReads,invalid,failures}
 })
 expect(result).toEqual({reads:1,providerReads:1,invalid:true,failures:Array(4).fill('Error: browser pairing profile unavailable')})
 verify()
})

// 删除原 signal 的封闭检查或用 race 假装 provider 退休，都会破坏此断言。
test('Pairing_ActualEdgeCancelledReadRetainsOriginalPending_003',async({page})=>{
 const verify=await prepare(page)
 const result=await page.evaluate(async()=>{
  const api:FixtureModule=await import(location.origin+'/fixture.mjs'),f=await api.fixture(),controller=new AbortController()
  let release!:(value:{configuration:Uint8Array;handles:BrowserProfileHandles})=>void,reads=0,settled=0
  const reader=api.createApprovedBrowserProfileReader(f.bytes,{readApproved:async()=>{reads++;return new Promise(resolve=>{release=resolve})}})
  const observe=(work:Promise<unknown>)=>work.then(()=>{settled++;return 'UNEXPECTED_SUCCESS'},error=>{settled++;return String(error)})
  const first=observe(reader(controller.signal));await Promise.resolve()
  const second=observe(reader(new AbortController().signal))
  controller.abort();await new Promise(resolve=>setTimeout(resolve,0))
  const pending=settled===0
  release({configuration:f.bytes,handles:f.handles})
  const failures=await Promise.all([first,second,observe(reader(new AbortController().signal))])
  return {pending,reads,failures}
 })
 expect(result).toEqual({pending:true,reads:1,failures:Array(3).fill('Error: browser pairing profile unavailable')})
 verify()
})

// 后续调用自己的取消不能取得缓存成功，也不能撤销另一个仍有效的调用。
test('Pairing_ActualEdgePerCallerCancellationAndMissingProvider_004',async({page})=>{
 const verify=await prepare(page)
 const result=await page.evaluate(async()=>{
  const api:FixtureModule=await import(location.origin+'/fixture.mjs'),f=await api.fixture()
  let release!:(value:{configuration:Uint8Array;handles:BrowserProfileHandles})=>void,reads=0
  const reader=api.createApprovedBrowserProfileReader(f.bytes,{readApproved:async()=>{reads++;return new Promise(resolve=>{release=resolve})}})
  const original=reader(new AbortController().signal);await Promise.resolve()
  const controller=new AbortController(),later=reader(controller.signal).then(()=>false,()=>true)
  controller.abort();release({configuration:f.bytes,handles:f.handles})
  const firstSucceeded=(await original)?.grant.scope.run==='PUBLIC_TEST_ONLY',laterRejected=await later
  const cachedRejected=await reader(controller.signal).then(()=>false,()=>true)
  let missingReads=0
  const missing=api.createApprovedBrowserProfileReader(f.bytes,{async readApproved(){missingReads++;return undefined}})
  const empty=(await missing(new AbortController().signal))===undefined&&(await missing(new AbortController().signal))===undefined
  let preAbortedReads=0
  const preAborted=api.createApprovedBrowserProfileReader(f.bytes,{async readApproved(){preAbortedReads++;return undefined}})
  const preRejected=await preAborted(controller.signal).then(()=>false,()=>true)
  const retryRejected=await preAborted(new AbortController().signal).then(()=>false,()=>true)
  return {reads,firstSucceeded,laterRejected,cachedRejected,missingReads,empty,preAbortedReads,preRejected,retryRejected}
 })
 expect(result).toEqual({reads:1,firstSucceeded:true,laterRejected:true,cachedRejected:true,missingReads:1,empty:true,preAbortedReads:0,preRejected:true,retryRejected:true})
 verify()
})
