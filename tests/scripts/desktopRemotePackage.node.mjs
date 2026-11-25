import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
test('Windows standard package excludes the remote host and browser builds, but keeps ConPTY',()=>{
 const config=read('src-tauri/tauri.windows.conf.json');
 assert.ok(!config.build.features?.includes('remote-host'));
 assert.ok(!config.build.features?.includes('remote-client-ipc'));
 assert.equal(config.build.beforeBuildCommand,'node scripts/prepare-conpty.mjs && npm run build');
 assert.match(fs.readFileSync('src-tauri/Cargo.toml','utf8'),/default = \["sqlite"\]/);
 assert.equal(read('src-tauri/tauri.conf.json').build.features,undefined);
 assert.equal(read('src-tauri/tauri.remote-trial.conf.json').build.beforeBuildCommand,'node scripts/prepare-remote-trial.mjs');
});

test('Vite resolves a desktop-only production distribution, even with old public files',async()=>{
 const {resolveConfig}=await import('vite');
 const desktop=await resolveConfig({configFile:'vite.config.ts'},'build','production','production');
 assert.deepEqual(Object.keys(desktop.build.rollupOptions.input),['main','versionManager']);
 assert.equal(desktop.build.copyPublicDir,false);
 assert.ok(!desktop.plugins.some(p=>p.name==='registration-source'));
 const browser=await resolveConfig({configFile:'vite.config.ts',mode:'browser-product'},'build','production','production');
 assert.deepEqual(Object.keys(browser.build.rollupOptions.input),['main','versionManager','browserRemote','browserRegistration','browserProfile']);
 assert.equal(browser.build.copyPublicDir,true);
 assert.ok(browser.plugins.some(p=>p.name==='registration-source'));
 assert.equal(browser.isProduction,true);
 assert.equal(browser.build.minify,desktop.build.minify);
});

import {desktopEnvironment} from '../../scripts/build-desktop-product.mjs';
import path from 'node:path';
const existing=path.resolve('scripts/build-desktop-product.mjs');
const tools={WASM_BINDGEN:existing,CC_wasm32_unknown_unknown:existing,AR_wasm32_unknown_unknown:existing};
const versions=()=>{let call=0;return ()=>['rustc 1.98.1 (fixture)','wasm-bindgen 0.2.129','clang version 21.1.8','LLVM version 21.1.8'][call++];};
test('Standard build freezes tools in a child environment and rejects conflicts',()=>{
 const input={...tools,RUSTUP_TOOLCHAIN:'wrong'};
 const env=desktopEnvironment(input,versions());
 assert.equal(input.RUSTUP_TOOLCHAIN,'wrong');
 assert.equal(env.RUSTUP_TOOLCHAIN,'1.98.1');assert.equal(env.CARGO_NET_OFFLINE,'true');
 assert.equal(env['CC_wasm32-unknown-unknown'],existing);assert.equal(env['AR_wasm32-unknown-unknown'],existing);
 assert.throws(()=>desktopEnvironment({...tools,'CC_wasm32-unknown-unknown':'other'},versions()),/Conflicting/);
 assert.throws(()=>desktopEnvironment({},versions()),/Explicit existing/);
 for(let index=0;index<4;index++){let call=0;const good=versions();assert.throws(()=>desktopEnvironment(tools,()=>{const value=good();return call++===index?'wrong':value;}),/required/);}
});
test('Windows release builds the signed desktop candidate without browser tools',()=>{
 const workflow=fs.readFileSync('.github/workflows/release.yml','utf8');
 assert.match(workflow,/Install and check signed Windows candidate before upload[\s\S]+needs\.admission\.outputs\.mobile_release == 'true'\s+uses: \.\/\.github\/actions\/setup-windows-browser-tools/);
 const action=fs.readFileSync('.github/actions/setup-windows-browser-tools/action.yml','utf8');
 assert.match(action,/cargo fetch --locked --manifest-path remote\/Cargo.toml/);
 assert.match(action,/rustup target add wasm32-unknown-unknown --toolchain 1.98.1/);
 const supplier=fs.readFileSync('scripts/prepare-windows-browser-tools.ps1','utf8');
 assert.match(supplier,/Get-FileHash -LiteralPath \$file -Algorithm SHA256/);
 assert.match(supplier,/749d22f565fcd5718dbed06512572d0e5353b502c03fe1f7f17ee8b8aca21a47/);
 assert.match(workflow,/npm run tauri -- build --bundles nsis/);
});

import {root,validateInputInventory} from '../../scripts/remote-trial-build-policy.mjs';
test('Standard target inventory stays exact rather than accepting trial or foreign inputs',()=>{
 const target=path.join(root,'.audit','desktop-product-browser');
 const dir=path.join(target,'wasm32-unknown-unknown','debug');
 const entries=['libcc_desk_remote_client.rlib','deps/libcc_desk_remote_security-0123456789abcdef.rlib','deps/librustls-fedcba9876543210.rlib'].map(file=>({file:path.relative(root,path.join(dir,file))}));
 validateInputInventory(entries,false,target);
 assert.throws(()=>validateInputInventory(entries,false),/path mismatch/);
 assert.throws(()=>validateInputInventory([...entries.slice(0,2),entries[1]],false,target),/role mismatch/);
 assert.throws(()=>validateInputInventory(entries,false,path.join(root,'.audit','foreign')),/path mismatch/);
});

test('Every Windows standard Tauri build job supplies the same prerequisites',()=>{
 const required=[['ci.yml','roundtrip-release'],['ci.yml','roundtrip-debug'],['conpty-integration.yml','package'],['paste-runtime-trace.yml','diagnostic']];
 for(const [file,job] of required){const text=fs.readFileSync('.github/workflows/'+file,'utf8');const start=text.indexOf('  '+job+':');assert.ok(start>=0);const next=text.slice(start+1).search(/\n  [a-z][a-z0-9-]*:/);const body=next<0?text.slice(start):text.slice(start,start+1+next);const supply=body.indexOf('uses: ./.github/actions/setup-windows-browser-tools');assert.ok(supply>=0&&supply<body.indexOf('npm run tauri'),file+' '+job);}
});

test('Browser prefetch covers the complete locked resolution graph including host metadata',()=>{
 const action=fs.readFileSync('.github/actions/setup-windows-browser-tools/action.yml','utf8');
 const fetch=action.split(/\r?\n/).find(line=>line.trim().startsWith('cargo fetch '));
 assert.equal(fetch?.trim(),'cargo fetch --locked --manifest-path remote/Cargo.toml');
 for(const file of ['scripts/build-browser-runtime.mjs','scripts/build-browser-registration-runtime.mjs']){
  const source=fs.readFileSync(file,'utf8');assert.ok(source.includes("'--offline','--locked'"));assert.ok(source.includes("'--target','wasm32-unknown-unknown'"));
 }
});
