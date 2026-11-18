import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';

test('RemoteTrial_ExplicitBuildEntry_001',async()=>{
 const pkg=JSON.parse(await readFile('package.json','utf8'));
 assert.equal(pkg.scripts['build:win:remote-trial'],'node scripts/build-remote-trial.mjs');
 const config=JSON.parse(await readFile('src-tauri/tauri.remote-trial.conf.json','utf8'));
 assert.equal(config.bundle.createUpdaterArtifacts,false);
 assert.equal(config.build.beforeBuildCommand,'node scripts/prepare-remote-trial.mjs');
 assert.equal(config.build.frontendDist,'../.audit/remote-trial-dist');
 assert.equal(config.version,undefined);
 assert.equal(config.identifier,undefined);
});

import {mkdtemp,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {selectedTools,offlineEnvironment,verifyRuntime} from '../../scripts/remote-trial-build-policy.mjs';
import {buildArguments} from '../../scripts/build-remote-trial.mjs';

test('RemoteTrial_NoInstallerOrFeatureOverride_002',()=>{
 const args=buildArguments(['--debug','--no-bundle'],'selected-cargo');
 assert.ok(args.includes('--no-bundle'));
 assert.ok(args.includes('--no-sign'));
 assert.equal(args[args.indexOf('--features')+1],'remote-host');
 for(const extra of [['--bundles','nsis'],['--config','other.json'],['--features','other'],['--ignore-version-mismatches']])assert.throws(()=>buildArguments(extra,'selected-cargo'));
});
test('RemoteTrial_ExplicitToolsAndOffline_003',()=>{
 assert.throws(()=>selectedTools({}),/Explicit existing tool/);
 assert.throws(()=>selectedTools({CC_DESK_TRIAL_CARGO:'cargo.exe'}),/Explicit existing tool/);
 const env=offlineEnvironment({PATH:'original'},{CC_DESK_TRIAL_CARGO:'C:/chosen/cargo.exe',CC_DESK_TRIAL_RUSTC:'C:/chosen/rustc.exe',CC_DESK_TRIAL_CLANG:'chosen-clang',CC_DESK_TRIAL_AR:'chosen-ar',CC_DESK_TRIAL_BINDGEN:'chosen-bindgen'});
 assert.equal(env.CARGO_NET_OFFLINE,'true');assert.equal(env.npm_config_offline,'true');
 assert.equal(env.WASM_BINDGEN,'chosen-bindgen');assert.equal(env.RUSTC,'C:/chosen/rustc.exe');
 assert.equal(env.CC_wasm32_unknown_unknown,'chosen-clang');
});
test('RemoteTrial_IncompleteRuntimeRejected_004',async()=>{
 await mkdir('.audit',{recursive:true});const dir=await mkdtemp('.audit/trial-policy-');
 await writeFile(path.join(dir,'manifest.json'),JSON.stringify({kind:'OriginalBrowserRuntime',sources:[],inputs:[],files:[]}));
 assert.throws(()=>verifyRuntime(dir),/Missing sources/);
 await writeFile(path.join(dir,'manifest.json'),JSON.stringify({kind:'OriginalBrowserRuntime',sources:[{file:'remote/browser-runtime.rs',sha256:'0'.repeat(64)}],inputs:[{}],files:[{}]}));
 assert.throws(()=>verifyRuntime(dir),/source inventory mismatch/);
});
test('RemoteTrial_StandardDefaultsUnchanged_005',async()=>{
 const normal=JSON.parse(await readFile('src-tauri/tauri.conf.json','utf8'));
 assert.equal(normal.build.beforeBuildCommand,'npm run build');
 assert.equal(normal.bundle.createUpdaterArtifacts,true);
 const cargo=await readFile('src-tauri/Cargo.toml','utf8');assert.match(cargo,/default = \["sqlite"\]/);
 const release=await readFile('.github/workflows/release.yml','utf8');assert.equal(release.includes('remote-trial'),false);
});

test('RemoteTrial_NoInheritedCompilerReplacement_006',()=>{
 assert.throws(()=>offlineEnvironment({RUSTC_WRAPPER:'other'},{}),/Unselected/);
 const env=offlineEnvironment({'CC_wasm32-unknown-unknown':'other','AR_wasm32-unknown-unknown':'other'},{CC_DESK_TRIAL_CARGO:'C:/chosen/cargo.exe',CC_DESK_TRIAL_CLANG:'chosen-clang',CC_DESK_TRIAL_AR:'chosen-ar'});
 assert.equal(env['CC_wasm32-unknown-unknown'],'chosen-clang');
 assert.equal(env['AR_wasm32-unknown-unknown'],'chosen-ar');
});

import {readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
test('RemoteTrial_ChangedSourceHashRejected_007',async()=>{
 const walk=async dir=>(await Promise.all((await readdir(dir,{withFileTypes:true})).map(async entry=>entry.isDirectory()?walk(`${dir}/${entry.name}`):entry.name.endsWith('.rs')?[`${dir}/${entry.name}`]:[]))).flat();
 const files=['remote/browser-runtime.rs','scripts/build-browser-runtime.mjs','remote/Cargo.lock','remote/Cargo.toml','remote/crates/client/Cargo.toml','remote/crates/security/Cargo.toml',...await walk('remote/crates/client/src'),...await walk('remote/crates/security/src'),...await walk('protocol/remote/inner/v1/src')];
 const sources=await Promise.all(files.map(async file=>({file,sha256:createHash('sha256').update(await readFile(file)).digest('hex')})));
 sources[0].sha256='0'.repeat(64);
 const dir=await mkdtemp('.audit/trial-source-hash-');
 await writeFile(path.join(dir,'manifest.json'),JSON.stringify({kind:'OriginalBrowserRuntime',sources,inputs:sources.slice(1,4),files:[{}]}));
 assert.throws(()=>verifyRuntime(dir),/source\/input mismatch/);
});

import {validateInputInventory,trialTarget} from '../../scripts/remote-trial-build-policy.mjs';
test('RemoteTrial_InputRolesAndTargetAreExact_008',()=>{
 const file=name=>({file:path.join(trialTarget,'browser','wasm32-unknown-unknown','debug','deps',`lib${name}-0123456789abcdef.rlib`)});
 const primary=name=>({file:path.join(trialTarget,'browser','wasm32-unknown-unknown','debug',`lib${name}.rlib`)});
 const valid=[primary('cc_desk_remote_client'),file('cc_desk_remote_security'),file('rustls')];
 assert.doesNotThrow(()=>validateInputInventory(valid,false));
 assert.throws(()=>validateInputInventory([valid[0],valid[0],valid[2]],false),/role mismatch/);
 assert.throws(()=>validateInputInventory([valid[0],valid[1],{file:'other/librustls-0123456789abcdef.rlib'}],false),/path mismatch/);
 assert.throws(()=>validateInputInventory([valid[0],valid[1],file('unknown')],false),/role mismatch/);
 assert.doesNotThrow(()=>validateInputInventory([primary('cc_desk_remote_security'),file('rustls')],true));
});
