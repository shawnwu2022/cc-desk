import {beforeAll,expect,test,vi} from 'vitest'
import {createECDH} from 'node:crypto'
import {createApprovedBrowserRegistrationReader,createProductBrowserRegistrationReader,parseBrowserRegistrationConfiguration} from '../../remote/ui-terminal/src/browserRegistrationProfile.ts'
import {BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME} from '../../remote/ui-terminal/src/browserProductStartup.ts'

const configuration={version:1,plan:{
 databaseName:BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME,expectedRevision:null,relaySpki:Buffer.alloc(91,3).toString('base64url'),requestedUserHandle:'AQID',
 configuration:{version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid'},
  localSpki:Buffer.alloc(91,1).toString('base64url'),hostSpki:Buffer.alloc(91,4).toString('base64url'),
  trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},
  grant:{issuerSpki:Buffer.alloc(91,2).toString('base64url'),principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',allowBackedUp:false}},
  relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},
  artifacts:{wasm:'https://desk.invalid/terminal.wasm',glue:'https://desk.invalid/terminal.js'},
 }},artifacts:{wasm:'https://desk.invalid/registration.wasm',glue:'https://desk.invalid/registration.js'}}
let keys:{tlsKey:CryptoKey;grantKey:CryptoKey;relayKey:CryptoKey}
let extractableKey:CryptoKey
beforeAll(async()=>{
 // 只导入公开 TEST_ONLY scalar；不创建真实身份或调用 authenticator。
 const imported:CryptoKey[]=[]
 for(const scalar of [1,2,3,4]){
  const d=Buffer.alloc(32);d[31]=scalar;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey()
  imported.push(await crypto.subtle.importKey('jwk',{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},{name:'ECDSA',namedCurve:'P-256'},scalar===4,['sign']))
 }
 keys={tlsKey:imported[0],grantKey:imported[1],relayKey:imported[2]};extractableKey=imported[3]
})

// 公开 JSON 只产生冻结值，不能包含 credential 或授权标记。
test('RegistrationSource_PublicSnapshot_001',()=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),parsed=parseBrowserRegistrationConfiguration(bytes);bytes.fill(0)
 expect(parsed).toEqual(configuration);expect(Object.isFrozen(parsed.plan.configuration.grant.scope)).toBe(true)
 expect(Object.isFrozen(parsed.artifacts)).toBe(true);expect('credentialId' in parsed.plan.configuration.profile).toBe(false)
})

// 原 provider 唯一读取只接受 signal，输出保留原三把 key 且不签名。
test('RegistrationSource_OriginalHandles_002',async()=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),approved=Buffer.from(bytes),supplied={configuration:approved,keys:{...keys}}
 const readApproved=vi.fn(async()=>supplied),sign=vi.spyOn(crypto.subtle,'sign'),signal=new AbortController().signal
 try{
  const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved});bytes.fill(0)
  const result=await reader(signal);expect(readApproved).toHaveBeenCalledExactlyOnceWith(signal)
  expect(result?.keys.tlsKey).toBe(keys.tlsKey);expect(result?.keys.grantKey).toBe(keys.grantKey);expect(result?.keys.relayKey).toBe(keys.relayKey)
  approved.fill(0);supplied.keys.tlsKey=extractableKey
  expect(await reader(new AbortController().signal)).toBe(result);expect(result?.keys.tlsKey).toBe(keys.tlsKey)
  expect(result?.approvedPlan).toEqual(configuration.plan);expect(result?.artifacts).toEqual(configuration.artifacts)
  expect(sign).not.toHaveBeenCalled();expect(readApproved).toHaveBeenCalledTimes(1)
 }finally{sign.mockRestore()}
})

// 身份、持久化目标、权限及两类 runtime 地址必须全部匹配独立批准。
test.each([
 ['plan.databaseName','OTHER_TEST_ONLY'],['plan.expectedRevision','2'],['plan.requestedUserHandle','BAUG'],['plan.relaySpki',Buffer.alloc(91,5).toString('base64url')],
 ['plan.configuration.localSpki',Buffer.alloc(91,5).toString('base64url')],['plan.configuration.hostSpki',Buffer.alloc(91,5).toString('base64url')],
 ['plan.configuration.trust.hostKeyEpoch','3'],['plan.configuration.trust.clientKeyEpoch','3'],
 ['plan.configuration.grant.issuerSpki',Buffer.alloc(91,5).toString('base64url')],['plan.configuration.grant.principal','06'.repeat(32)],
 ['plan.configuration.grant.scope.session','06'.repeat(16)],['plan.configuration.grant.scope.run','OTHER_TEST_ONLY'],['plan.configuration.grant.scope.generation','8'],
 ['plan.configuration.grant.role','Observer'],['plan.configuration.grant.budgetMs',9999],['plan.configuration.grant.peerInstall.budgetMs',999],['plan.configuration.grant.profile.allowBackedUp',true],
 ['plan.configuration.relay.mode','legacy'],['plan.configuration.relay.tunnel',Buffer.alloc(16,6).toString('base64url')],['plan.configuration.relay.keyId',Buffer.alloc(16,6).toString('base64url')],
 ['plan.configuration.artifacts.wasm','https://desk.invalid/other.wasm'],['plan.configuration.artifacts.glue','https://desk.invalid/other.js'],
 ['artifacts.wasm','https://desk.invalid/other.wasm'],['artifacts.glue','https://desk.invalid/other.js'],
])('RegistrationSource_Mismatch_003_%#',async(path,value)=>{
 const candidate=structuredClone(configuration) as any,parts=(path as string).split('.');let owner=candidate
 for(const name of parts.slice(0,-1))owner=owner[name];owner[parts[parts.length-1]]=value
 const readApproved=vi.fn(async()=>({configuration:Buffer.from(JSON.stringify(configuration)),keys}))
 const reader=createApprovedBrowserRegistrationReader(Buffer.from(JSON.stringify(candidate)),{readApproved})
 await expect(reader(new AbortController().signal)).rejects.toThrow(/^browser registration profile unavailable$/)
 expect(readApproved).toHaveBeenCalledTimes(1)
})

// 即使成组修改通过公开语法，也不能建立来源身份或降级协议。
test.each(['origin','rp','relay','mode'] as const)('RegistrationSource_GroupMismatch_004_%s',async kind=>{
 const candidate=structuredClone(configuration) as any,c=candidate.plan.configuration
 if(kind==='origin'){
  c.profile.origin=c.grant.profile.origin='https://other.invalid';c.profile.rpId=c.grant.profile.rpId='other.invalid'
  for(const artifacts of [c.artifacts,candidate.artifacts])for(const key of ['wasm','glue'])artifacts[key]=artifacts[key].replace('desk.invalid','other.invalid')
 }else if(kind==='rp')c.profile.rpId=c.grant.profile.rpId='invalid'
 else if(kind==='relay')c.trust.origin=c.relay.endpoint='wss://other.invalid/route'
 else delete c.relay.mode
 const expected=structuredClone(configuration);if(kind==='mode')expected.plan.configuration.relay.mode='legacy'
 expect(parseBrowserRegistrationConfiguration(Buffer.from(JSON.stringify(candidate)))).toEqual(candidate)
 const reader=createApprovedBrowserRegistrationReader(Buffer.from(JSON.stringify(candidate)),{readApproved:async()=>({configuration:Buffer.from(JSON.stringify(expected)),keys})})
 await expect(reader(new AbortController().signal)).rejects.toThrow('browser registration profile unavailable')
})

// 重复/越界/额外字段不能借成功 receipt、credential 或 runtime URL 进入计划。
test.each([
 ['version',2],['plan.databaseName','../outside'],['plan.expectedRevision','0'],['plan.requestedUserHandle','AQID=='],['plan.relaySpki','AA'],
 ['plan.configuration.profile.credentialId','AQ'],['plan.configuration.grant.profile.credentialSpki',Buffer.alloc(91).toString('base64url')],['plan.configuration.grant.profile.backupEligible',false],
 ['plan.configuration.hostSpki','AA'],['plan.configuration.relay.mode','automatic'],['plan.configuration.extra',true],['plan.receipt','1'],['keys',{}],
 ['artifacts.wasm','http://desk.invalid/a.wasm'],['artifacts.glue','https://other.invalid/a.js'],['artifacts.glue','https://desk.invalid/a.js?token=TEST_ONLY'],
 ['artifacts.glue','https://desk.invalid/a%2f.js'],['artifacts.glue','https://desk.invalid:0/a.js'],['artifacts.extra','https://desk.invalid/a.js'],
])('RegistrationSource_RejectValue_005_%#',(path,value)=>{
 const candidate=structuredClone(configuration) as any,parts=(path as string).split('.');let owner=candidate
 for(const name of parts.slice(0,-1))owner=owner[name];owner[parts[parts.length-1]]=value
 expect(()=>parseBrowserRegistrationConfiguration(Buffer.from(JSON.stringify(candidate)))).toThrow(/^browser registration configuration$/)
})

// 严格 JSON 在合法文档内也拒绝同值重复字段、非法 UTF-8、尾部和超限。
test.each(['duplicate','escaped','nested','utf8','trailing','size'])('RegistrationSource_RejectBytes_006_%s',kind=>{
 let text=JSON.stringify(configuration)
 if(kind==='duplicate')text='{"version":1,'+text.slice(1)
 if(kind==='escaped')text='{"versi\\u006fn":1,'+text.slice(1)
 if(kind==='nested')text=text.replace('"requestedUserHandle":"AQID"','"requestedUserHandle":"AQID","requestedUserHandle":"AQID"')
 if(kind==='trailing')text+=' trailing';if(kind==='size')text=' '.repeat(16385)
 expect(()=>parseBrowserRegistrationConfiguration(kind==='utf8'?new Uint8Array([255]):Buffer.from(text))).toThrow('browser registration configuration')
})

// 缺 provider/批准保持 Locked，而且构造本身不调用来源。
test('RegistrationSource_MissingApproval_007',async()=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),signal=new AbortController().signal,readApproved=vi.fn(async()=>undefined)
 const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved});expect(readApproved).not.toHaveBeenCalled()
 await expect(createApprovedBrowserRegistrationReader(bytes)(signal)).resolves.toBeUndefined()
 await expect(reader(signal)).resolves.toBeUndefined();await expect(reader(signal)).resolves.toBeUndefined();expect(readApproved).toHaveBeenCalledTimes(1)
})

// 取消后保留并等待原 promise；更换 signal 不能重读或发布迟到批准。
test('RegistrationSource_CancelPending_008',async()=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),controller=new AbortController()
 let release!:(v:{configuration:Uint8Array;keys:typeof keys})=>void
 const readApproved=vi.fn(()=>new Promise<{configuration:Uint8Array;keys:typeof keys}>(resolve=>{release=resolve}))
 const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved}),first=reader(controller.signal);await Promise.resolve()
 const second=reader(new AbortController().signal);let settled=0;void first.catch(()=>{settled++});void second.catch(()=>{settled++})
 controller.abort();await Promise.resolve();await Promise.resolve();expect(settled).toBe(0);expect(readApproved).toHaveBeenCalledTimes(1)
 release({configuration:bytes,keys});await expect(first).rejects.toThrow('browser registration profile unavailable');await expect(second).rejects.toThrow('browser registration profile unavailable')
 await expect(reader(new AbortController().signal)).rejects.toThrow('browser registration profile unavailable');expect(readApproved).toHaveBeenCalledTimes(1)
})

// 等待原批准完成后只返回同一冻结结果，不替换来源或 key handles。
test('RegistrationSource_DelayedOriginal_009',async()=>{
 const bytes=Buffer.from(JSON.stringify(configuration));let release!:(v:{configuration:Uint8Array;keys:typeof keys})=>void
 const readApproved=vi.fn(()=>new Promise<{configuration:Uint8Array;keys:typeof keys}>(resolve=>{release=resolve})),provider={readApproved}
 const reader=createApprovedBrowserRegistrationReader(bytes,provider),first=reader(new AbortController().signal);await Promise.resolve()
 provider.readApproved=vi.fn(async()=>({configuration:bytes,keys:{...keys,tlsKey:extractableKey}}))
 const second=reader(new AbortController().signal);release({configuration:bytes,keys})
 const original=await first;expect(await second).toBe(original);expect(original?.keys.tlsKey).toBe(keys.tlsKey)
 expect(readApproved).toHaveBeenCalledTimes(1);expect(provider.readApproved).not.toHaveBeenCalled()
})

// 预取消/无效首读消耗本次来源，缓存成功也不能忽略首 signal 的后来取消。
test.each(['preabort','invalid','cached'] as const)('RegistrationSource_CancelReplay_010_%s',async kind=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),readApproved=vi.fn(async()=>({configuration:bytes,keys})),controller=new AbortController()
 const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved})
 if(kind==='cached')expect(await reader(controller.signal)).toBeDefined()
 controller.abort();await expect(reader(kind==='invalid'?{} as AbortSignal:controller.signal)).rejects.toThrow('browser registration profile unavailable')
 await expect(reader(new AbortController().signal)).rejects.toThrow('browser registration profile unavailable')
 expect(readApproved).toHaveBeenCalledTimes(kind==='cached'?1:0)
})

// 来源 getter/额外字段/坏 bytes、同一或可导出 key 都拒绝且不泄露错误详情。
test.each(['throw','bytes','same','extractable','missing','extra','getter','key-getter'] as const)('RegistrationSource_InvalidProvider_011_%s',async kind=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),supplied:any={configuration:bytes,keys:{...keys}},getter=vi.fn(()=>bytes)
 if(kind==='bytes')supplied.configuration=Buffer.from('{"receipt":"1"}')
 if(kind==='same')supplied.keys.relayKey=keys.tlsKey
 if(kind==='extractable')supplied.keys.tlsKey=extractableKey
 if(kind==='missing')delete supplied.keys.relayKey
 if(kind==='extra')supplied.approved=true
 if(kind==='getter')Object.defineProperty(supplied,'configuration',{enumerable:true,get:getter})
 if(kind==='key-getter')Object.defineProperty(supplied.keys,'tlsKey',{enumerable:true,get:getter})
 const readApproved=vi.fn(async()=>{if(kind==='throw')throw Error('PRIVATE_PROVIDER_DETAIL');return supplied})
 const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved})
 for(let attempt=0;attempt<2;attempt++)await expect(reader(new AbortController().signal)).rejects.toThrow(/^browser registration profile unavailable$/)
 expect(readApproved).toHaveBeenCalledTimes(1);expect(getter).not.toHaveBeenCalled()
})

// 产品 helper 仅接受从最初就批准的固定库；通用 source 不偷偷迁移目标。
test('RegistrationSource_ProductDatabase_012',async()=>{
 const candidate=structuredClone(configuration);candidate.plan.databaseName='OTHER_TEST_ONLY'
 const bytes=Buffer.from(JSON.stringify(candidate)),readApproved=vi.fn(async()=>({configuration:bytes,keys}))
 expect(()=>createProductBrowserRegistrationReader(bytes,{readApproved})).toThrow('browser registration profile unavailable');expect(readApproved).not.toHaveBeenCalled()
 expect((await createApprovedBrowserRegistrationReader(bytes,{readApproved})(new AbortController().signal))?.approvedPlan.databaseName).toBe('OTHER_TEST_ONLY')
 const product=Buffer.from(JSON.stringify(configuration))
 expect((await createProductBrowserRegistrationReader(product,{readApproved:async()=>({configuration:product,keys})})(new AbortController().signal))?.approvedPlan.databaseName).toBe(BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME)
})

// 字段顺序不重要，缺失 mode 与显式 mode 则已在独立比较中保留差异。
test('RegistrationSource_FieldOrder_013',async()=>{
 const bytes=Buffer.from(JSON.stringify(Object.fromEntries(Object.entries(configuration).reverse()),null,2))
 const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved:async()=>({configuration:Buffer.from(JSON.stringify(configuration)),keys})})
 expect((await reader(new AbortController().signal))?.approvedPlan).toEqual(configuration.plan)
})

// 后来的取消只拒绝该读取，不污染仍有效的原成功，也不能重读 provider。
test('RegistrationSource_LaterAbort_014',async()=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),readApproved=vi.fn(async()=>({configuration:bytes,keys}))
 const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved}),original=await reader(new AbortController().signal)
 const later=new AbortController();later.abort()
 await expect(reader(later.signal)).rejects.toThrow('browser registration profile unavailable')
 expect(await reader(new AbortController().signal)).toBe(original);expect(readApproved).toHaveBeenCalledTimes(1)
})

// 解析 provider 字节前已捕获原 handles，不能在同步解析回调中换成其他 key。
test('RegistrationSource_CaptureKeys_015',async()=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),supplied={configuration:bytes,keys:{...keys}}
 const reader=createApprovedBrowserRegistrationReader(bytes,{readApproved:async()=>supplied}),decode=globalThis.atob
 const spy=vi.spyOn(globalThis,'atob').mockImplementation(value=>{supplied.keys.tlsKey=extractableKey;return decode(value)})
 try{expect((await reader(new AbortController().signal))?.keys.tlsKey).toBe(keys.tlsKey)}finally{spy.mockRestore()}
})

// 来源方法也只允许 own data property，不执行 getter 或接受继承来的批准方法。
test.each(['getter','inherited','extra'])('RegistrationSource_ProviderShape_016_%s',kind=>{
 const bytes=Buffer.from(JSON.stringify(configuration)),readApproved=vi.fn(async()=>undefined),getter=vi.fn(()=>readApproved)
 const provider:any=kind==='inherited'?Object.create({readApproved}):{readApproved}
 if(kind==='getter')Object.defineProperty(provider,'readApproved',{enumerable:true,get:getter})
 if(kind==='extra')provider.candidate=bytes
 expect(()=>createApprovedBrowserRegistrationReader(bytes,provider)).toThrow('browser registration profile unavailable')
 expect(getter).not.toHaveBeenCalled();expect(readApproved).not.toHaveBeenCalled()
})
