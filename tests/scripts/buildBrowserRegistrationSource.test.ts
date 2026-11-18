import {expect,test} from 'vitest'
import {spawnSync} from 'node:child_process'
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,existsSync} from 'node:fs'
import {resolve,relative,basename} from 'node:path'
import {createHash} from 'node:crypto'

// 用真实构建脚本测试唯一导出与默认 Locked；所有生成文件都保留在项目 audit。
test('RegistrationBundle_Standalone_001',()=>{
 const audit=resolve('.audit/registration-source-build-tests');mkdirSync(audit,{recursive:true})
 const root=mkdtempSync(resolve(audit,'standalone-')),entry=resolve(root,'entry.ts'),out=resolve(root,'out')
 const adapter=relative(root,resolve('remote/ui-terminal/src/browserRegistrationProfile.ts')).replaceAll('\\','/')
 const config={version:1,plan:{databaseName:'CC_DESK_BROWSER_ENROLLMENT_V1',expectedRevision:null,relaySpki:Buffer.alloc(91,3).toString('base64url'),requestedUserHandle:'AQID',configuration:{version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid'},localSpki:Buffer.alloc(91,1).toString('base64url'),hostSpki:Buffer.alloc(91,4).toString('base64url'),trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:Buffer.alloc(91,2).toString('base64url'),principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',allowBackedUp:false}},relay:{endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},artifacts:{wasm:'https://desk.invalid/terminal.wasm',glue:'https://desk.invalid/terminal.js'}}},artifacts:{wasm:'https://desk.invalid/registration.wasm',glue:'https://desk.invalid/registration.js'}}
 // TEST_ONLY 全局只隔离受控 provider；它的批准字节在调用进程中独立提供。
 writeFileSync(entry,`import {createProductBrowserRegistrationReader} from ${JSON.stringify(adapter)}; export const readApprovedBrowserRegistrationInputs=createProductBrowserRegistrationReader(new TextEncoder().encode(${JSON.stringify(JSON.stringify(config))}),{async readApproved(signal){if(signal.aborted)throw Error('TEST_ONLY cancelled');globalThis.TEST_ONLY_READS=(globalThis.TEST_ONLY_READS??0)+1;return globalThis.TEST_ONLY_APPROVAL;}});`)
 const result=spawnSync(process.execPath,['scripts/build-browser-registration-source.mjs','--entry',entry,'--out-dir',out],{encoding:'utf8',windowsHide:true})
 expect(result.status,result.stderr).toBe(0)
 const bytes=readFileSync(resolve(out,'browser-registration.js')),manifest=JSON.parse(readFileSync(resolve(out,'manifest.json'),'utf8'))
 expect(manifest.exports).toEqual(['readApprovedBrowserRegistrationInputs']);expect(manifest.artifact.sha256).toBe(createHash('sha256').update(bytes).digest('hex'))
 expect(manifest.artifact.bytes).toBe(bytes.length);expect(manifest.sources.length).toBeGreaterThan(1)
 for(const source of manifest.sources)expect(source.sha256).toBe(createHash('sha256').update(readFileSync(resolve(source.path))).digest('hex'))
 // 用实际产物 data URL 导入，任何相对导入、浏览器副作用或多余导出都会失败。
 const script=`const m=await import(${JSON.stringify('data:text/javascript;base64,'+bytes.toString('base64'))});if(Object.keys(m).join()!=='readApprovedBrowserRegistrationInputs')throw Error('exports');const signal=new AbortController().signal;if(await m.readApprovedBrowserRegistrationInputs(signal)!==undefined)throw Error('unexpected approval');console.log('standalone Locked');`
 const smoke=spawnSync(process.execPath,['--input-type=module','-e',script],{encoding:'utf8',windowsHide:true})
 expect(smoke.status,smoke.stderr).toBe(0);expect(smoke.stdout).toContain('standalone Locked')
 const approvedScript=`import {createECDH} from 'node:crypto';const handles=[];for(const n of [1,2,3]){const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();handles.push(await crypto.subtle.importKey('jwk',{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},{name:'ECDSA',namedCurve:'P-256'},false,['sign']));}const keys={tlsKey:handles[0],grantKey:handles[1],relayKey:handles[2]};globalThis.TEST_ONLY_APPROVAL={configuration:new TextEncoder().encode(${JSON.stringify(JSON.stringify(config))}),keys};const m=await import(${JSON.stringify('data:text/javascript;base64,'+bytes.toString('base64'))});if(globalThis.TEST_ONLY_READS)throw Error('import read provider');const signal=new AbortController().signal;const result=await m.readApprovedBrowserRegistrationInputs(signal);if(Object.keys(keys).some(k=>result.keys[k]!==keys[k])||result.approvedPlan.databaseName!=='CC_DESK_BROWSER_ENROLLMENT_V1')throw Error('binding');if(await m.readApprovedBrowserRegistrationInputs(signal)!==result||globalThis.TEST_ONLY_READS!==1)throw Error('replay');console.log('standalone original handles');`
 const approvedSmoke=spawnSync(process.execPath,['--input-type=module','-e',approvedScript],{encoding:'utf8',windowsHide:true})
 expect(approvedSmoke.status,approvedSmoke.stderr).toBe(0);expect(approvedSmoke.stdout).toContain('standalone original handles')
},30000)

// 非空目录与项目外路径在写入前拒绝，不能破坏已有产物或扩展写入范围。
test.each(['nonempty','outside'])('RegistrationBundle_OutputBoundary_003_%s',kind=>{
 const audit=resolve('.audit/registration-source-build-tests');mkdirSync(audit,{recursive:true})
 const root=mkdtempSync(resolve(audit,'output-')),entry=resolve(root,'entry.ts')
 const out=kind==='outside'?resolve(process.cwd(),'..','TEST_ONLY_OUTSIDE_'+basename(root)):resolve(root,'out')
 writeFileSync(entry,'export async function readApprovedBrowserRegistrationInputs(){return undefined;}')
 if(kind==='nonempty'){mkdirSync(out);writeFileSync(resolve(out,'browser-registration.js'),'TEST_ONLY original bytes')}
 const result=spawnSync(process.execPath,['scripts/build-browser-registration-source.mjs','--entry',entry,'--out-dir',out],{encoding:'utf8',windowsHide:true})
 expect(result.status).not.toBe(0)
 if(kind==='nonempty')expect(readFileSync(resolve(out,'browser-registration.js'),'utf8')).toBe('TEST_ONLY original bytes')
 else expect(existsSync(out)).toBe(false)
 expect(existsSync(resolve(out,'manifest.json'))).toBe(false)
},30000)

// 错导出、外部/动态模块依赖或超出 loader 上限不得写出可部署产物。
test.each(['wrong','extra','external','dynamic','large'])('RegistrationBundle_Reject_002_%s',kind=>{
 const audit=resolve('.audit/registration-source-build-tests');mkdirSync(audit,{recursive:true})
 const root=mkdtempSync(resolve(audit,kind+'-')),entry=resolve(root,'entry.ts'),out=resolve(root,'out')
 let code='export async function readApprovedBrowserRegistrationInputs(){return undefined;}'
 if(kind==='wrong')code='export async function read(){return undefined;}'
 if(kind==='extra')code+='export const extra=true;'
 if(kind==='external')code='import {read} from "https://untrusted.invalid/source.js";export const readApprovedBrowserRegistrationInputs=read;'
 if(kind==='dynamic')code='export async function readApprovedBrowserRegistrationInputs(signal){return import(signal.url);}'
 if(kind==='large')code=`export async function readApprovedBrowserRegistrationInputs(){return ${JSON.stringify('x'.repeat(1048577))};}`
 writeFileSync(entry,code)
 const result=spawnSync(process.execPath,['scripts/build-browser-registration-source.mjs','--entry',entry,'--out-dir',out],{encoding:'utf8',windowsHide:true})
 expect(result.status).not.toBe(0);expect(existsSync(resolve(out,'browser-registration.js'))).toBe(false)
},30000)
