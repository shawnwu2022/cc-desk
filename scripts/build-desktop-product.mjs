import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {root,run,verifyRuntime} from './remote-trial-build-policy.mjs';
const inspect=(exe,args,env)=>{const r=spawnSync(exe,args,{env,encoding:'utf8',windowsHide:true});if(r.error||r.status!==0)throw Error('Required browser build tool failed: '+exe);return r.stdout;};
export function desktopEnvironment(input,version=inspect){
 const env={...input,RUSTUP_TOOLCHAIN:'1.98.1',CARGO_NET_OFFLINE:'true'};
 for(const [plain,dashed] of [['CC_wasm32_unknown_unknown','CC_wasm32-unknown-unknown'],['AR_wasm32_unknown_unknown','AR_wasm32-unknown-unknown']]){
  if(env[dashed]&&env[plain]!==env[dashed])throw Error('Conflicting browser target tool');
  env[dashed]=env[plain];
 }
 for(const name of ['WASM_BINDGEN','CC_wasm32_unknown_unknown','AR_wasm32_unknown_unknown'])if(!env[name]||!path.isAbsolute(env[name])||!fs.statSync(env[name]).isFile())throw Error('Explicit existing browser tool required: '+name);
 if(!/^rustc 1\.98\.1 /m.test(version(env.RUSTC??'rustc',['-Vv'],env)))throw Error('Rust 1.98.1 required');
 if(version(env.WASM_BINDGEN,['--version'],env).trim()!=='wasm-bindgen 0.2.129')throw Error('wasm-bindgen 0.2.129 required');
 for(const tool of [env.CC_wasm32_unknown_unknown,env.AR_wasm32_unknown_unknown])if(!/version 21\.1\.8\b/.test(version(tool,['--version'],env)))throw Error('LLVM 21.1.8 required');
 return env;
}
export function excludeReleaseClientBootstrap(site){
 const runtime=path.join(site,'remote-runtime');
 for(const dir of [site,runtime]){
  const stat=fs.lstatSync(dir);
  if(!stat.isDirectory()||stat.isSymbolicLink()||path.relative(dir,fs.realpathSync(dir)))throw Error('Mobile release staging requires confined directories');
 }
 const bootstrap=path.join(runtime,'fixture.js');
 let stat;
 try{stat=fs.lstatSync(bootstrap);}catch(error){if(error.code==='ENOENT')return;throw error;}
 if(!stat.isFile()||stat.isSymbolicLink())throw Error('Unexpected mobile release bootstrap');
 // The Worker loads fixture_bg.js and fixture_bg.wasm directly, not the
 // wasm-bindgen auto-start entry. Exclude only that known generated file.
 fs.unlinkSync(bootstrap);
}

export function buildDesktopProduct({releaseSite=false}={}){
 // 标准 Windows 包包含能力，但不创建登记、授权或网络会话。
 if(process.platform!=='win32')throw Error('Desktop remote host packaging is Windows-only');
 const browserTarget=path.join(root,'.audit','desktop-product-browser');
 const env=desktopEnvironment(process.env);env.CARGO_TARGET_DIR=browserTarget;
 for(const script of ['prepare-session-sdk.mjs','prepare-opencode-archive-sdk.mjs','build-browser-runtime.mjs','build-browser-registration-runtime.mjs'])run(process.execPath,['scripts/'+script],env);
 for(const name of ['remote-runtime','remote-registration-runtime'])verifyRuntime(path.join(root,'public',name),undefined,browserTarget);
 const site=releaseSite?path.join(root,'.audit','browser-release-site'):path.join(root,'dist');
 if(releaseSite){
  const parent=path.dirname(site);
  const stat=fs.lstatSync(parent);
  if(!stat.isDirectory()||stat.isSymbolicLink()||path.relative(parent,fs.realpathSync(parent))||fs.lstatSync(site,{throwIfNoEntry:false}))throw Error('Browser release stage must be new and confined');
 }
 run(process.execPath,['node_modules/vite/bin/vite.js','build','--mode','browser-product',...(releaseSite?['--outDir',site]:[])],env);
 for(const name of ['remote-runtime','remote-registration-runtime'])verifyRuntime(path.join(root,'public',name),path.join(site,name),browserTarget);
 if(releaseSite)excludeReleaseClientBootstrap(site);
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2);
 if(args.length>1||(args.length===1&&args[0]!=='--release-site'))throw Error('Unknown browser build argument');
 buildDesktopProduct({releaseSite:args.length===1});
}
