import {test} from 'node:test';import assert from 'node:assert/strict';import {spawn} from 'node:child_process';import {once} from 'node:events';import WebSocketFixture from 'ws';import {readFileSync} from 'node:fs';import {createPrivateKey,sign} from 'node:crypto';
import {createBrowserRelay} from './browserRelay.ts';
test('BrowserRelay_OriginalRelayAdmissionAndForwarding_001',async()=>{
 const executable=process.env.CC_DESK_TEST_ONLY_ORIGINAL_RELAY_ORACLE;if(!executable)throw Error('explicit TEST_ONLY Rust oracle required');
 const child=spawn(executable,[],{env:{...process.env,CC_DESK_TEST_ONLY_BROWSER_RELAY:'1'},stdio:['pipe','pipe','pipe'],windowsHide:true});
 const exited=once(child,'exit');let output='',errors='';child.stderr.on('data',b=>{errors+=b;});
 const port=await new Promise<number>((resolve,reject)=>{child.stdout.on('data',b=>{output+=b;const match=output.match(/\"port\":(\d+)/);if(match)resolve(Number(match[1]));});void exited.then(()=>reject(Error(output+errors)));});
 child.stdin.end('0\n');await new Promise<void>((resolve,reject)=>{if(output.includes('hostAdmitted')){resolve();return;}child.stdout.on('data',()=>{if(output.includes('hostAdmitted'))resolve();});void exited.then(()=>reject(Error(output+errors)));});
 const ca=readFileSync(new URL('../../crates/transport/tests/fixtures/root.pem',import.meta.url));
 // Browser-shaped Node WS fixture, fixed CA and verified foobar.com name.
 const socket=new WebSocketFixture(`wss://foobar.com:${port}/relay`,{origin:'https://desk.invalid',ca,rejectUnauthorized:true,servername:'foobar.com',lookup:(_hostname,_options,callback)=>callback(null,[{address:'127.0.0.1',family:4}])});
 const scalar=Buffer.from('c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721','hex'),point=Buffer.from('0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299','hex');
 const key=createPrivateKey({key:{kty:'EC',crv:'P-256',d:scalar.toString('base64url'),x:point.subarray(1,33).toString('base64url'),y:point.subarray(33).toString('base64url')},format:'jwk'});
 const relay=createBrowserRelay({origin:`wss://foobar.com:${port}/relay`,tunnel:new Uint8Array(16).fill(9),identity:{keyId:new Uint8Array(16).fill(7),sign:async r=>Uint8Array.from(sign('sha256',r.transcript,{key,dsaEncoding:'ieee-p1363'}))},socket:socket as unknown as WebSocket,setupDeadline:performance.timeOrigin+performance.now()+5000,ioBudgetMs:5000});
 try{const transport=await relay.ready;await transport.write(new TextEncoder().encode('TEST_ONLY_BROWSER_TO_RUST'));const response=await transport.read(16384);assert.equal(new TextDecoder().decode(response!),'TEST_ONLY_RUST_TO_BROWSER');relay.close();await relay.waitClosed();const [code]=await exited;assert.equal(code,0,output+errors);assert.match(output,/originalRelayRetired/);}catch(error){throw Error(String(error)+'\n'+output+errors);}finally{relay.close();if(child.exitCode===null){child.kill();await exited;}}
});
