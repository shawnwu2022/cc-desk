import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createServer} from 'node:http';
import {resolve} from 'node:path';
import {build} from 'esbuild';
import {chromium} from 'playwright';

// 独立临时浏览器上下文；不读取用户资料、忽略证书或替换 DedicatedWorker。
const executablePath=process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE;
const artifacts=process.env.CC_DESK_TEST_BROWSER_ARTIFACTS;
assert.ok(executablePath && artifacts,'明确指定现有浏览器和已核测试 WASM 目录');
const mutation=process.env.CC_DESK_TEST_REMOVE_HTTPS_GATE==='1';
const bundle=async entry=>(await build({entryPoints:[entry],bundle:true,format:'esm',platform:'browser',write:false,plugins:mutation?[{name:'test-only-https-gate-mutation',setup(builder){builder.onLoad({filter:/browserProfileSource\.ts$/},async args=>{const text=await readFile(args.path,'utf8'),gate="&&new URL(location.origin).protocol==='https:'";assert.equal(text.split(gate).length,2);return{contents:text.replace(gate,''),loader:'ts'};});}}]:[]})).outputFiles[0].contents;
const worker=await bundle('remote/ui-terminal/src/browserEntry.worker.ts');
const profile=await bundle('remote/ui-terminal/src/browserProfileSource.ts');
const wasm=await readFile(resolve(artifacts,'fixture_bg.wasm'));
const glue=await readFile(resolve(artifacts,'fixture_bg.js'),'utf8');
const requests=[];
const server=createServer((req,res)=>{
 const path=req.url;requests.push(path);
 res.setHeader('Cross-Origin-Opener-Policy','same-origin');
 res.setHeader('Cross-Origin-Embedder-Policy','require-corp');
 res.setHeader('Cache-Control','no-store');
 if(path==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><title>CC Desk isolated Worker verification</title>');}
 else if(path==='/worker.js'||path==='/profile.js'){res.setHeader('Content-Type','text/javascript');res.end(path==='/worker.js'?worker:profile);}
 else if(path==='/runtime.wasm'){res.setHeader('Content-Type','application/wasm');res.end(wasm);}
 else if(path==='/runtime.js'){
  res.setHeader('Content-Type','text/javascript');
  // 只追加模块求值证据；原官方 glue 全文不变，不伪造运行或认证成功。
  res.end(glue+'\npostMessage({kind:"TEST_ONLY_glue_evaluated"});');
 }else{res.statusCode=404;res.end();}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;
let browser;
try{
 browser=await chromium.launch({executablePath,headless:true});
 const context=await browser.newContext();const page=await context.newPage();await page.goto(origin);
 assert.deepEqual(await page.evaluate(()=>[isSecureContext,crossOriginIsolated]),[true,true]);
 const missing=await page.evaluate(async()=>{const {controlledBrowserProfileSource}=await import('/profile.js');return await controlledBrowserProfileSource().open(new AbortController().signal);});
 assert.equal(missing,undefined);assert.equal(requests.includes('/browser-profile.js'),false);
 console.log('PASS BrowserReal_HttpOperatorDoesNotFetchConfiguration_001');
 for(const mode of ['foreign','missing','import']){
  const before=requests.length;
  const result=await page.evaluate(async mode=>{
   const worker=new Worker('/worker.js',{type:'module'});
   const wire=new MessageChannel(),application=new MessageChannel(),tls=new MessageChannel(),grant=new MessageChannel();
   const messages=[],applicationMessages=[];let transportClosed=false;let applicationDone;const applicationRetired=new Promise(resolve=>{applicationDone=resolve;});application.port1.onmessage=event=>{applicationMessages.push(event.data);if(event.data?.kind==='retired')applicationDone();};
   wire.port1.onmessage=event=>{if(event.data?.kind==='close'){transportClosed=true;wire.port1.postMessage({kind:'retired',uncertain:false});}};
   let watchdog;const timeout=new Promise((_,reject)=>{watchdog=setTimeout(()=>reject(Error('original Worker retirement timeout')),6000);});const done=new Promise((resolve,reject)=>{worker.onmessage=event=>{messages.push(event.data);if(event.data?.kind==='retired')resolve(event.data);};worker.onerror=event=>reject(Error(event.message));});
   const timer=setTimeout(()=>worker.postMessage({kind:'close'}),3000);
   try{
    worker.postMessage({kind:'start',origin:location.origin,artifacts:{wasm:mode==='foreign'?'http://foreign.invalid/runtime.wasm':location.origin+(mode==='missing'?'/missing.wasm':'/runtime.wasm'),glue:location.origin+'/runtime.js'},localSpki:new Uint8Array(),hostSpki:new Uint8Array(),trust:{},tls:{port:tls.port2,origin:location.origin,shared:new SharedArrayBuffer(92),setupDeadline:performance.timeOrigin+performance.now()+2000},grant:{port:grant.port2},transport:wire.port2,application:application.port2},[tls.port2,grant.port2,wire.port2,application.port2]);
    const [retired]=await Promise.race([Promise.all([done,applicationRetired]),timeout]);return {retired,messages,transportClosed,applicationMessages};
   }finally{clearTimeout(timer);clearTimeout(watchdog);worker.terminate();wire.port1.close();application.port1.close();tls.port1.close();grant.port1.close();}
  },mode);
  assert.equal(result.retired.uncertain,undefined);assert.equal(result.transportClosed,true);assert.ok(result.applicationMessages.some(x=>x.kind==='invalidated'));assert.ok(result.applicationMessages.some(x=>x.kind==='retired'&&x.uncertain===(mode==='import'?undefined:false)));
  assert.equal(result.applicationMessages.some(x=>x.kind==='descriptor'),false);const hit=requests.slice(before);
  assert.equal(hit.includes('/runtime.js'),mode==='import');
  assert.equal(result.messages.some(x=>x.kind==='TEST_ONLY_glue_evaluated'),mode==='import');
  if(mode==='foreign')assert.equal(hit.some(x=>x.endsWith('.wasm')),false);
  if(mode==='import')assert.ok(hit.includes('/runtime.wasm'));
  console.log(`PASS BrowserReal_EntryWorker_${mode}_002`,JSON.stringify({requests:hit,transportClosed:result.transportClosed}));
 }
 await context.close();
}finally{try{if(browser)await browser.close();}finally{await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}}
