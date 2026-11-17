import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash,createECDH} from 'node:crypto';
import {resolve} from 'node:path';
import {build} from 'esbuild';
import {chromium} from 'playwright';

// 合成 HTTPS origin 只供浏览器行为测试，不发生 HTTPS 握手或修改信任。
const executablePath=process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE;
assert.ok(executablePath,'必须显式选择已安装官方浏览器');
const root=process.env.CC_DESK_TEST_PRODUCTION_RUNTIME??'public/remote-runtime',manifest=JSON.parse(await readFile(resolve(root,'manifest.json'),'utf8'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
assert.equal(manifest.kind,'OriginalBrowserRuntime');
for(const file of manifest.sources)assert.equal(hash(await readFile(file.file)),file.sha256,`source ${file.file}`);
for(const file of manifest.files)assert.equal(hash(await readFile(resolve(root,file.name))),file.sha256,`artifact ${file.name}`);
for(const file of manifest.inputs)assert.equal(hash(await readFile(resolve(file.file))),file.sha256,`input ${file.file}`);
const wasm=await readFile(resolve(root,'fixture_bg.wasm'));
const exports=WebAssembly.Module.exports(new WebAssembly.Module(wasm)).map(x=>x.name);
for(const name of ['browser_client_start','browser_client_event','browser_tls_start','browser_tls_poll','browser_tls_cancel','browser_tls_event'])assert.ok(exports.includes(name),name);
assert.equal(exports.some(name=>name.includes('TEST_ONLY')),false);
console.log('PASS BrowserProduction_CurrentManifestAndExports_001',JSON.stringify({sources:manifest.sources.length,sha256:hash(wasm)}));
const module=(await build({stdin:{contents:"export * from './remote/ui-terminal/src/browserProfileSource.ts';export * from './remote/ui-terminal/src/browserPairingProfile.ts';",resolveDir:process.cwd()},bundle:true,format:'esm',platform:'browser',write:false})).outputFiles[0].text;
const vectors=[1,2].map(n=>{const d=Buffer.alloc(32);d[31]=n;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const p=ec.getPublicKey();return {jwk:{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:p.subarray(1,33).toString('base64url'),y:p.subarray(33).toString('base64url')},spki:Buffer.concat([Buffer.from('3059301306072a8648ce3d020106082a8648ce3d030107034200','hex'),p]).toString('base64url')};});
const origin='https://desk.invalid';
const base={version:1,profile:{origin,rpId:'desk.invalid',credentialId:'Bw'},localSpki:vectors[0].spki,hostSpki:vectors[1].spki,trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:vectors[1].spki,principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:7000,peerInstall:{budgetMs:3000,controlLease:'live-v1',snapshotPull:'snapshot-pull-v1'},profile:{origin,rpId:'desk.invalid',credentialId:'Bw',credentialSpki:vectors[1].spki,backupEligible:false,allowBackedUp:false}},relay:{endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,3).toString('base64url'),keyId:Buffer.alloc(16,2).toString('base64url')},artifacts:{wasm:origin+'/runtime.wasm',glue:origin+'/runtime.js'}};
const mismatch=`import {createApprovedBrowserProfileReader} from '/module.js';const candidate=${JSON.stringify(base)},provided=structuredClone(candidate);delete provided.grant.peerInstall.snapshotPull;const [tlsKey,grantKey]=await Promise.all(${JSON.stringify(vectors.map(v=>v.jwk))}.map(jwk=>crypto.subtle.importKey('jwk',jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));export const readApprovedBrowserInputs=createApprovedBrowserProfileReader(new TextEncoder().encode(JSON.stringify(candidate)),{readApproved:async()=>({configuration:new TextEncoder().encode(JSON.stringify(provided)),handles:{tlsKey,grantKey,relaySign:async()=>new Uint8Array(64),present:async()=>{}}})});`;
const browser=await chromium.launch({executablePath,headless:true});
try{
 for(const mode of ['missing','undefined','extra','mismatch']){
  const context=await browser.newContext({serviceWorkers:'block'});let workers=0,sockets=0;const requests=[];
  try{
   await context.routeWebSocket('**/*',socket=>socket.close());
   await context.route('**/*',async route=>{const url=new URL(route.request().url());requests.push(url.href);if(url.origin!==origin)return route.abort();const headers={'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'};
    if(url.pathname==='/browser-profile.js'){const body=mode==='extra'?'export const readApprovedBrowserInputs=async()=>undefined;export const forbidden=true;':mode==='mismatch'?mismatch:'export const readApprovedBrowserInputs=async()=>undefined;';return route.fulfill({status:mode==='missing'?404:200,headers,contentType:'text/javascript',body});}
    return route.fulfill({headers,contentType:url.pathname==='/module.js'?'text/javascript':'text/html',body:url.pathname==='/module.js'?module:'<!doctype html><title>isolated operator test</title>'});
   });
   const page=await context.newPage();page.on('worker',()=>workers++);page.on('websocket',()=>sockets++);await page.goto(origin);
   assert.deepEqual(await page.evaluate(()=>[isSecureContext,crossOriginIsolated,location.protocol]),[true,true,'https:']);
   const result=await page.evaluate(async()=>{const {controlledBrowserProfileSource}=await import('/module.js');try{return await controlledBrowserProfileSource().open(new AbortController().signal)===undefined?'locked':'unexpected';}catch{return 'rejected';}});
   assert.equal(result,['missing','undefined'].includes(mode)?'locked':'rejected');assert.equal(workers,0);assert.equal(sockets,0);assert.ok(requests.includes(origin+'/browser-profile.js'));assert.equal(requests.some(x=>x.startsWith('wss:')),false);
      if(mode==='mismatch'){
    const selected=await page.evaluate(async ({base,vectors})=>{const {createApprovedBrowserProfileReader}=await import('/module.js');const [tlsKey,grantKey]=await Promise.all(vectors.map(v=>crypto.subtle.importKey('jwk',v.jwk,{name:'ECDSA',namedCurve:'P-256'},false,['sign'])));const bytes=new TextEncoder().encode(JSON.stringify(base));const value=await createApprovedBrowserProfileReader(bytes,{readApproved:async()=>({configuration:bytes,handles:{tlsKey,grantKey,relaySign:async()=>new Uint8Array(64),present:async()=>{}}})})(new AbortController().signal);return {pull:value?.grant.peerInstall.snapshotPull,sameKeys:value?.profile.tlsKey===tlsKey&&value?.profile.grantKey===grantKey};},{base,vectors});
    assert.deepEqual(selected,{pull:'snapshot-pull-v1',sameKeys:true});assert.equal(workers,0);assert.equal(sockets,0);
    console.log('PASS BrowserProduction_OriginalProviderSelection_003');
   }
   console.log(`PASS BrowserProduction_Operator_${mode}_002`,JSON.stringify({result,workers,sockets}));
  }finally{await context.close();}
 }
}finally{await browser.close();}
