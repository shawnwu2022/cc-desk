import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {projectPath,validateRegistrationExports} from './build-browser-registration-runtime.mjs';

export const root=fs.realpathSync(fileURLToPath(new URL('../',import.meta.url)));
export const trialDist=projectPath(root,'.audit/remote-trial-dist');
export const trialTarget=projectPath(root,'.audit/remote-trial-target');
const selected={CC_DESK_TRIAL_CARGO:'cargo.exe',CC_DESK_TRIAL_RUSTC:'rustc.exe',CC_DESK_TRIAL_CLANG:'clang.exe',CC_DESK_TRIAL_AR:'llvm-ar.exe',CC_DESK_TRIAL_BINDGEN:'wasm-bindgen.exe'};
export function selectedTools(env){
 const result={};
 for(const [key,name] of Object.entries(selected)){
  const value=env[key];
  if(!value||!path.isAbsolute(value)||path.basename(value).toLowerCase()!==name||!fs.statSync(value).isFile())throw Error(`Explicit existing tool required: ${key}`);
  // rustup 根据 argv[0] 选择 cargo/rustc；保留调用方显式选择的入口名。
  result[key]=path.resolve(value);
 }
 if(path.dirname(result.CC_DESK_TRIAL_CARGO)!==path.dirname(result.CC_DESK_TRIAL_RUSTC))throw Error('Cargo and rustc must share the selected tool directory');
 return result;
}
export function offlineEnvironment(env,tools){
 // 只选择明确给定的工具，不安装、不回落到其他 LLVM 或 bindgen。
 if(env.RUSTC_WRAPPER||env.RUSTC_WORKSPACE_WRAPPER||env.CARGO_BUILD_RUSTC_WRAPPER||env.CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER)throw Error('Unselected Rust compiler wrapper');
 const result={...env,...tools,CARGO_NET_OFFLINE:'true',npm_config_offline:'true',npm_config_audit:'false',npm_config_fund:'false',
  RUSTC:tools.CC_DESK_TRIAL_RUSTC,WASM_BINDGEN:tools.CC_DESK_TRIAL_BINDGEN,
  CC_wasm32_unknown_unknown:tools.CC_DESK_TRIAL_CLANG,AR_wasm32_unknown_unknown:tools.CC_DESK_TRIAL_AR};
 result['CC_wasm32-unknown-unknown']=tools.CC_DESK_TRIAL_CLANG;
 result['AR_wasm32-unknown-unknown']=tools.CC_DESK_TRIAL_AR;
 const key=Object.keys(result).find(key=>key.toLowerCase()==='path')??'PATH';
 result[key]=path.dirname(tools.CC_DESK_TRIAL_CARGO)+path.delimiter+(result[key]??'');
 return result;
}
export function run(executable,args,env){
 const child=spawnSync(executable,args,{cwd:root,env,stdio:'inherit',windowsHide:true});
 if(child.error||child.status!==0)throw Error(`Build step failed: ${path.basename(executable)} (${child.status})`,{cause:child.error});
}
const sha=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
export function validateInputInventory(inputs,registration){
 const roles=registration?['cc_desk_remote_security','rustls']:['cc_desk_remote_client','cc_desk_remote_security','rustls'];
 if(inputs.length!==roles.length)throw Error('Runtime input inventory mismatch');
 const directory=path.join(trialTarget,'browser','wasm32-unknown-unknown','debug');
 const main=registration?'cc_desk_remote_security':'cc_desk_remote_client';
 const seen=new Set();
 for(const input of inputs){
  const file=projectPath(root,input.file);
  const primary=path.basename(file)===`lib${main}.rlib`;
  if(path.dirname(file)!==(primary?directory:path.join(directory,'deps')))throw Error('Runtime input path mismatch');
  const match=/^lib([a-z_]+)-[0-9a-f]{16}\.rlib$/.exec(path.basename(file));
  const role=primary?main:match?.[1];
  if(!role||(!primary&&role===main)||!roles.includes(role)||seen.has(role))throw Error('Runtime input role mismatch');
  seen.add(role);
 }
}
export function verifyRuntime(directory,dist=undefined){
 const manifest=JSON.parse(fs.readFileSync(path.join(directory,'manifest.json'),'utf8'));
 const registration=manifest.kind==='OriginalBrowserRegistrationRuntime';
 if(!registration&&manifest.kind!=='OriginalBrowserRuntime')throw Error('Unknown runtime kind');
 for(const group of ['sources','inputs','files'])if(!Array.isArray(manifest[group])||!manifest[group].length)throw Error(`Missing ${group}`);
 const walk=dir=>fs.readdirSync(path.join(root,dir),{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?walk(path.posix.join(dir,entry.name)):entry.name.endsWith('.rs')?[path.posix.join(dir,entry.name)]:[]);
 const common=['remote/Cargo.lock','remote/Cargo.toml','remote/crates/security/Cargo.toml',...walk('remote/crates/security/src'),...walk('protocol/remote/inner/v1/src')];
 const expected=registration?['remote/browser-registration-runtime.rs','scripts/build-browser-registration-runtime.mjs','protocol/remote/inner/v1/Cargo.toml',...common]:['remote/browser-runtime.rs','scripts/build-browser-runtime.mjs','remote/crates/client/Cargo.toml',...walk('remote/crates/client/src'),...common];
 if(JSON.stringify(manifest.sources.map(x=>x.file).sort())!==JSON.stringify(expected.sort()))throw Error('Runtime source inventory mismatch');
 for(const entry of manifest.sources){
  const file=projectPath(root,entry.file);
  if(sha(file)!==entry.sha256)throw Error(`Runtime source/input mismatch: ${entry.file}`);
 }
 validateInputInventory(manifest.inputs,registration);
 for(const entry of manifest.inputs)if(sha(projectPath(root,entry.file))!==entry.sha256)throw Error('Runtime input hash mismatch');
 if(manifest.cargoArtifacts&&sha(projectPath(root,manifest.cargoArtifacts.file))!==manifest.cargoArtifacts.sha256)throw Error('Cargo artifact evidence mismatch');
 if(JSON.stringify(manifest.files.map(x=>x.name).sort())!==JSON.stringify(['fixture_bg.js','fixture_bg.wasm']))throw Error('Runtime file inventory');
 for(const entry of manifest.files){
  const file=path.join(directory,entry.name);
  if(fs.statSync(file).size!==entry.bytes||sha(file)!==entry.sha256)throw Error(`Runtime artifact mismatch: ${entry.name}`);
  if(dist&&sha(path.join(dist,entry.name))!==entry.sha256)throw Error(`Packaged artifact mismatch: ${entry.name}`);
 }
 const module=new WebAssembly.Module(fs.readFileSync(path.join(directory,'fixture_bg.wasm')));
 const exports=WebAssembly.Module.exports(module),imports=WebAssembly.Module.imports(module);
 if([...exports,...imports].some(x=>/TEST_ONLY/i.test((x.module??'')+x.name)))throw Error('Test-only runtime symbol');
 if(registration)validateRegistrationExports(exports);
 else for(const name of ['browser_client_start','browser_client_event','browser_tls_start','browser_tls_poll','browser_tls_cancel','browser_tls_event']){
  if(!exports.some(x=>x.name===name&&x.kind==='function'))throw Error('Missing production export');
 }
 if(JSON.stringify(exports.map(x=>x.name))!==JSON.stringify(manifest.exports))throw Error('Runtime export manifest mismatch');
 if(dist&&sha(path.join(dist,'manifest.json'))!==sha(path.join(directory,'manifest.json')))throw Error('Packaged manifest mismatch');
 return {kind:manifest.kind,sources:manifest.sources.length,inputs:manifest.inputs.length,files:manifest.files};
}
