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
export function buildDesktopProduct(){
 // 标准 Windows 包包含能力，但不创建登记、授权或网络会话。
 if(process.platform!=='win32')throw Error('Desktop remote host packaging is Windows-only');
 const browserTarget=path.join(root,'.audit','desktop-product-browser');
 const env=desktopEnvironment(process.env);env.CARGO_TARGET_DIR=browserTarget;
 for(const script of ['prepare-session-sdk.mjs','prepare-opencode-archive-sdk.mjs','build-browser-runtime.mjs','build-browser-registration-runtime.mjs'])run(process.execPath,['scripts/'+script],env);
 for(const name of ['remote-runtime','remote-registration-runtime'])verifyRuntime(path.join(root,'public',name),undefined,browserTarget);
 run(process.execPath,['node_modules/vite/bin/vite.js','build','--mode','browser-product'],env);
 for(const name of ['remote-runtime','remote-registration-runtime'])verifyRuntime(path.join(root,'public',name),path.join(root,'dist',name),browserTarget);
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))buildDesktopProduct();
