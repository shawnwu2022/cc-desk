// Build only the public TEST_ONLY Relay bridge, with an explicitly selected Relay checkout.
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=fs.realpathSync('.');
const supplied=process.env.CC_DESK_TEST_ONLY_RELAY_ROOT;
if(!supplied)throw Error('explicit CC_DESK_TEST_ONLY_RELAY_ROOT required');
const relay=fs.realpathSync(supplied),dir=path.join(root,'.audit/browser-entry-relay-build');
const source=path.join(root,'tests/remote-browser/relay-host-oracle');
fs.mkdirSync(path.join(dir,'src'),{recursive:true});fs.mkdirSync(path.join(dir,'runtime'),{recursive:true});
fs.copyFileSync(path.join(source,'main.rs'),path.join(dir,'src/main.rs'));
fs.copyFileSync(path.join(source,'Cargo.lock'),path.join(dir,'Cargo.lock'));
const quoted=p=>JSON.stringify(p.replaceAll('\\','/'));
fs.writeFileSync(path.join(dir,'Cargo.toml'),`[package]
name = "cc-desk-test-only-browser-relay-host-oracle"
version = "0.0.0"
edition = "2021"
publish = false
[dependencies]
cc-desk-relay = {path=${quoted(relay)}}
cc-desk-remote-protocol = {path=${quoted(path.join(relay,'vendor/cc-desk-remote-protocol/v0.1.0'))}}
tokio = {version="=1.52.1",features=["rt-multi-thread","macros","net","io-util","time","sync"]}
tokio-rustls = {version="=0.26.4",default-features=false,features=["ring"]}
rustls = {version="=0.23.46",default-features=false,features=["std","ring"]}
rustls-pemfile = "=1.0.4"
tokio-tungstenite = {version="=0.29.0",default-features=false,features=["handshake"]}
ring = "=0.17.14"
futures-util = {version="=0.3.32",features=["sink"]}
`);
const temp=path.join(dir,'runtime'),target=path.join(root,'target-browser-profile');
const args=['build','--offline','--locked','--manifest-path',path.join(dir,'Cargo.toml'),'--message-format=json'];
const result=spawnSync('cargo',args,{cwd:root,env:{...process.env,TEMP:temp,TMP:temp,TMPDIR:temp,CARGO_TARGET_DIR:target,CARGO_BUILD_JOBS:'2'},encoding:'utf8',maxBuffer:32*1024*1024});
fs.writeFileSync(path.join(dir,'build.stdout'),result.stdout??'');fs.writeFileSync(path.join(dir,'build.stderr'),result.stderr??'');
if(result.status!==0)throw Error('TEST_ONLY Relay build failed: '+(result.stderr??'').slice(-4000));
const rows=result.stdout.trim().split('\n').map(JSON.parse);
const executable=rows.find(v=>v.reason==='compiler-artifact'&&v.target.name==='cc-desk-test-only-browser-relay-host-oracle'&&v.executable)?.executable;
if(!executable)throw Error('missing TEST_ONLY Relay executable');
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const git=args=>{const result=spawnSync('git',['-C',relay,...args],{encoding:'utf8'});if(result.status!==0)throw Error('Relay source identity unavailable');return result.stdout.trim();};
const record={executable,sha256:hash(executable),relayRoot:relay,relayHead:git(['rev-parse','HEAD']),relayStatus:git(['status','--porcelain']),source:hash(path.join(source,'main.rs')),lock:hash(path.join(source,'Cargo.lock'))};
fs.writeFileSync(path.join(dir,'artifacts.json'),JSON.stringify(record,null,2));console.log(JSON.stringify(record,null,2));
