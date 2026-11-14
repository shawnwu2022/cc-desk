import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {readFileSync,mkdtempSync,writeFileSync,unlinkSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {createServer} from 'node:net';
import {createPrivateKey,sign} from 'node:crypto';
import Ws from 'ws';
import {createBrowserRelay,createBrowserRegistrationRelay,RENDEZVOUS_SUBPROTOCOL} from './browserRelay.ts';
const clock=()=>performance.timeOrigin+performance.now();
const hostPoint='0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299';
const phonePoint='04bac5b11cad8f99f9c72b05cf4b9e26d244dc189f745228255a219a86d6a09eff20138bf82dc1b6d562be0fa54ab7804a3a64b6d72ccfed6b6fb6ed28bbfc117e';
function signer(host:boolean){const point=Buffer.from(host?hostPoint:phonePoint,'hex'),scalar=Buffer.from(host?'c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721':'57c92077664146e876760c9520d054aa93c3afb04e306705db6090308507b4d3','hex');const key=createPrivateKey({format:'jwk',key:{kty:'EC',crv:'P-256',d:scalar.toString('base64url'),x:point.subarray(1,33).toString('base64url'),y:point.subarray(33).toString('base64url')}});return(bytes:Uint8Array)=>Uint8Array.from(sign('sha256',bytes,{key,dsaEncoding:'ieee-p1363'}));}
function inbox(socket:Ws){const queue:Uint8Array[]=[];let waiter:((v:Uint8Array)=>void)|undefined;socket.on('message',data=>{const bytes=new Uint8Array(data as Buffer);if(waiter){const original=waiter;waiter=undefined;original(bytes);}else queue.push(bytes);});return()=>queue.length?Promise.resolve(queue.shift()!):new Promise<Uint8Array>((resolve,reject)=>{const timer=setTimeout(()=>{waiter=undefined;reject(Error('fixture receive deadline'));},2000);waiter=value=>{clearTimeout(timer);resolve(value);};});}
function outer(bytes:Uint8Array){const value=new Uint8Array(24+bytes.length);value.set([1,1,0,1]);value.fill(9,4,20);new DataView(value.buffer).setUint32(20,bytes.length);value.set(bytes,24);return value;}
async function fixture(lifetimeMs=30000,legacy=false){
 const exe=process.env.CC_DESK_TEST_RELAY_EXE,repo=process.env.CC_DESK_TEST_RELAY_ROOT,base=process.env.CC_DESK_TEST_ARTIFACT_ROOT;if(!exe||!repo||!base)throw Error('explicit relay executable, repository and project artifact root required');
 const listener=createServer();listener.listen(0,'127.0.0.1');await once(listener,'listening');const port=(listener.address() as {port:number}).port;await new Promise<void>((r,j)=>listener.close(e=>e?j(e):r()));
 const origin=`wss://foobar.com:${port}/relay`,root=mkdtempSync(join(resolve(base),'browser-rendezvous-')),policy=join(root,'policy.json'),now=Date.now();
 writeFileSync(policy,JSON.stringify({schema:1,policy_id:'TEST_ONLY_rendezvous',revision:1,issued_at_unix_ms:now,not_before_unix_ms:now,expires_unix_ms:now+lifetimeMs,listen:`127.0.0.1:${port}`,signed_wss_endpoint:origin,browser_origins:['https://desk.invalid'],devices:[{id:'01'.repeat(16),public_key_sec1:hostPoint,role:'Host'},{id:'02'.repeat(16),public_key_sec1:phonePoint,role:'Phone'}],routes:[{tunnel:'09'.repeat(16),host:'01'.repeat(16),phone:'02'.repeat(16)}]}));
 const child=spawn(exe,['--policy',policy,'--tls-chain',join(repo,'tests/fixtures/chain.pem'),'--tls-key',join(repo,'tests/fixtures/end.key'),'--tls-ca',join(repo,'tests/fixtures/root.pem')],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe']});
 const exited=once(child,'exit');let output='',errors='';child.stderr.on('data',b=>errors+=b);
 try{await new Promise<void>((ok,no)=>{const timer=setTimeout(()=>no(Error('relay start deadline')),5000);child.stdout.on('data',b=>{output+=b;if(output.includes('WSS_LISTEN=')){clearTimeout(timer);ok();}});void exited.then(()=>{clearTimeout(timer);no(Error('relay start '+output+errors));});});}catch(error){if(child.exitCode===null)child.kill();await exited;throw error;}
 const socket=()=>new Ws(origin,legacy?[]:RENDEZVOUS_SUBPROTOCOL,{origin:'https://desk.invalid',ca:readFileSync(join(repo,'tests/fixtures/root.pem')),rejectUnauthorized:true,servername:'foobar.com',lookup:(_host,_options,callback)=>callback(null,[{address:'127.0.0.1',family:4}])});
 return{origin,socket,legacy,async close(){try{unlinkSync(policy);}catch{}const watchdog=setTimeout(()=>child.kill(),4000);const[code]=await exited;clearTimeout(watchdog);assert.equal(code,0,output+errors);}};
}
async function host(f:Awaited<ReturnType<typeof fixture>>){const socket=f.socket(),next=inbox(socket);await once(socket,'open');assert.equal(socket.protocol,f.legacy?'':RENDEZVOUS_SUBPROTOCOL);const challenge=await next();assert.equal(challenge.length,66);const origin=new TextEncoder().encode(f.origin),prefix=new TextEncoder().encode('CC_DESK_RELAY_ADMISSION\0\x01');const transcript=new Uint8Array([...prefix,origin.length>>8,origin.length&255,...origin,...challenge.subarray(2),...new Uint8Array(16).fill(1),1]);socket.send(new Uint8Array([1,1,...new Uint8Array(16).fill(1),1,...signer(true)(transcript)]));assert.deepEqual(await next(),new Uint8Array([1,2]));if(f.legacy)return{socket,next,ready:Promise.resolve()};socket.send(new Uint8Array([2,0,...new Uint8Array(16).fill(9)]));return{socket,next,ready:next().then(bytes=>assert.deepEqual(bytes,new Uint8Array([2,1,...new Uint8Array(16).fill(9)])))};}
function phone(f:Awaited<ReturnType<typeof fixture>>){const socket=f.socket();const admitted=new Promise<void>((ok,no)=>{const timer=setTimeout(()=>no(Error('fixture admission deadline')),5000);const failed=()=>{clearTimeout(timer);no(Error('fixture admission closed'));};socket.addEventListener('error',failed,{once:true});socket.addEventListener('close',failed,{once:true});socket.addEventListener('message',event=>{const b=new Uint8Array(event.data as ArrayBuffer);if(b.length===2&&b[0]===1&&b[1]===2){clearTimeout(timer);socket.removeEventListener('error',failed);socket.removeEventListener('close',failed);ok();}});});void admitted.catch(()=>{});const relay=createBrowserRelay({origin:f.origin,tunnel:new Uint8Array(16).fill(9),identity:{keyId:new Uint8Array(16).fill(2),sign:async r=>signer(false)(r.transcript)},socket:socket as unknown as WebSocket,mode:'rendezvous-v1',setupDeadline:clock()+5000,ioBudgetMs:5000});return{socket,relay,admitted};}
for(const phoneFirst of [true,false])test(`BrowserRendezvous_ActualRelay_${phoneFirst?'PhoneFirst':'HostFirst'}_005`,{timeout:15000},async()=>{
 const f=await fixture();let p:ReturnType<typeof phone>|undefined,h:Awaited<ReturnType<typeof host>>|undefined;
 try{
  let ready=false;
  if(phoneFirst){p=phone(f);void p.relay.ready.then(()=>ready=true,()=>{});await p.admitted;}else{h=await host(f);void h.ready.then(()=>ready=true,()=>{});}
  await new Promise(r=>setTimeout(r,100));assert.equal(ready,false,'first peer must not start TLS while opposite peer is absent');
  if(phoneFirst)h=await host(f);else p=phone(f);
  const wire=await p!.relay.ready;await h!.ready;
  const request=new TextEncoder().encode('TEST_ONLY_BROWSER_TO_HOST');await wire.write(request);assert.deepEqual(await h!.next(),outer(request));
  const reply=new TextEncoder().encode('TEST_ONLY_HOST_TO_BROWSER');h!.socket.send(outer(reply));assert.deepEqual(await wire.read(16384),reply);
 }finally{p?.relay.close();if(h){h.socket.close();if(h.socket.readyState!==Ws.CLOSED){const watchdog=setTimeout(()=>h!.socket.terminate(),2000);await once(h.socket,'close');clearTimeout(watchdog);}}if(p)await p.relay.waitClosed();await f.close();}
});

test('RegistrationRelay_ExplicitLegacyWssCrosses30SecondsAndStopsAtOriginalEnd_006',{timeout:60000},async()=>{
 const f=await fixture(60000,true);let h:Awaited<ReturnType<typeof host>>|undefined,p:ReturnType<typeof createBrowserRegistrationRelay>|undefined,heartbeat:ReturnType<typeof setInterval>|undefined;
 try{
  const peer=await host(f);h=peer;const controller=new AbortController(),started=clock(),end=started+40000,socket=f.socket();
  p=createBrowserRegistrationRelay({origin:f.origin,tunnel:new Uint8Array(16).fill(9),identity:{keyId:new Uint8Array(16).fill(2),sign:async r=>signer(false)(r.transcript)},socket:socket as unknown as WebSocket,mode:'legacy',setupDeadline:clock()+5000,ioBudgetMs:30000,registrationDeadline:end,signal:controller.signal});
  const wire=await p.ready;await h.ready;
  // TEST_ONLY opposite peer; actual relay still validates each empty frame's route and budgets.
  heartbeat=setInterval(()=>{if(clock()<end&&peer.socket.readyState===Ws.OPEN)peer.socket.send(outer(new Uint8Array()));},10000);
  await new Promise(resolve=>setTimeout(resolve,35000));assert.equal(socket.readyState,Ws.OPEN);assert.equal(h.socket.readyState,Ws.OPEN);
  const request=new TextEncoder().encode('TEST_ONLY_AFTER_35_SECONDS');await wire.write(request);
  let received=await h.next(),emptyCount=0;while(received.length===24){emptyCount++;assert.deepEqual(received,outer(new Uint8Array()));received=await h.next();}
  assert.deepEqual(received,outer(request));assert.ok(emptyCount>=3);
  const reply=new TextEncoder().encode('TEST_ONLY_STILL_BOUND');h.socket.send(outer(reply));assert.deepEqual(await wire.read(16384),reply);
  await p.waitClosed();const elapsed=clock()-started;assert.ok(elapsed>=39000&&elapsed<45000);assert.equal(socket.readyState,Ws.CLOSED);
  console.log(JSON.stringify({testOnly:true,actualRelay:true,mode:'explicit-legacy',verifiedCa:true,idleMilliseconds:35000,totalMilliseconds:elapsed,opaqueEmptyFrames:emptyCount,originalEndPreserved:true}));
 }finally{
  if(heartbeat!==undefined)clearInterval(heartbeat);p?.close();if(h){h.socket.close();if(h.socket.readyState!==Ws.CLOSED){const watchdog=setTimeout(()=>h!.socket.terminate(),2000);await once(h.socket,'close');clearTimeout(watchdog);}}if(p)await p.waitClosed();await f.close();
 }
});
