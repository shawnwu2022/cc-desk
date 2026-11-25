import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const root=fileURLToPath(new URL('../../',import.meta.url));
const entry=new URL('../../scripts/build-browser-registration-runtime.mjs',import.meta.url);
const load=async()=>{assert.ok(fs.existsSync(entry),'production registration build entry must exist');return import(entry.href);};
const symbols=['browser_registration_start','browser_registration_poll','browser_registration_cancel','browser_registration_event'];
const validExports=()=>[{name:'memory',kind:'memory'},{name:'__wbindgen_start',kind:'function'},...symbols.map(name=>({name,kind:'function'}))];

test('RegistrationRuntime_ExportPolicy_001',async()=>{
 const {validateRegistrationExports}=await load();
 assert.doesNotThrow(()=>validateRegistrationExports(validExports()));
 for(const name of symbols){assert.throws(()=>validateRegistrationExports(validExports().filter(e=>e.name!==name)),/export policy/);}
 for(const name of ['TEST_ONLY_owner','browser_client_start','browser_tls_start']){
  assert.throws(()=>validateRegistrationExports([...validExports(),{name,kind:'function'}]),/export policy/);
 }
 assert.throws(()=>validateRegistrationExports(validExports().map(e=>e.name==='memory'?{...e,kind:'function'}:e)),/export policy/);
 assert.throws(()=>validateRegistrationExports(validExports().map(e=>e.name===symbols[0]?{...e,kind:'global'}:e)),/export policy/);
 assert.throws(()=>validateRegistrationExports(validExports().filter(e=>e.name!=='__wbindgen_start')),/export policy/);
});

test('RegistrationRuntime_ExactCargoArtifact_002',async()=>{
 const {selectRegistrationArtifacts}=await load();
 const target=path.join(root,'.audit/build-browser-registration-runtime/target');
 const row=name=>({reason:'compiler-artifact',target:{name,kind:['lib'],src_path:path.join(root,name==='cc_desk_remote_security'?'remote/crates/security/src/lib.rs':'dependency/lib.rs')},profile:{test:false},features:['experimental'],filenames:[path.join(target,name==='cc_desk_remote_security'?'wasm32-unknown-unknown/debug':'wasm32-unknown-unknown/debug/deps',`lib${name}-exact.rlib`)]});
 const rows=[row('cc_desk_remote_security'),row('rustls')];
 assert.equal(selectRegistrationArtifacts(rows,root,target).security,rows[0].filenames[0]);
 assert.throws(()=>selectRegistrationArtifacts(rows.slice(1),root,target),/exact.*artifact/);
 assert.throws(()=>selectRegistrationArtifacts([...rows,rows[0]],root,target),/exact.*artifact/);
 assert.throws(()=>selectRegistrationArtifacts([{...rows[0],target:{...rows[0].target,src_path:path.join(root,'tests/replacement.rs')}},rows[1]],root,target),/source lineage/);
 assert.throws(()=>selectRegistrationArtifacts([{...rows[0],features:[]},rows[1]],root,target),/feature/);
 assert.throws(()=>selectRegistrationArtifacts([{...rows[0],profile:{test:true}},rows[1]],root,target),/test artifact/);
 assert.throws(()=>selectRegistrationArtifacts([{...rows[0],filenames:[path.join(target,'debug/deps/native.rlib')]},rows[1]],root,target),/wasm artifact/);
});

test('RegistrationRuntime_LinkOnlySource_003',async()=>{
 await load();
 const source=fs.readFileSync(path.join(root,'remote/browser-registration-runtime.rs'),'utf8').replace(/\/\/[^\n]*\n/g,'').replace(/\s/g,'');
 const forwards=`#![no_main]
 #[unsafe(no_mangle)]pub extern "C" fn browser_registration_start(setup:u32,total:u32)->i32{cc_desk_remote_security::browser_registration_start(setup,total)}
 #[unsafe(no_mangle)]pub extern "C" fn browser_registration_poll(){cc_desk_remote_security::browser_registration_poll()}
 #[unsafe(no_mangle)]pub extern "C" fn browser_registration_cancel(){cc_desk_remote_security::browser_registration_cancel()}
 #[unsafe(no_mangle)]pub extern "C" fn browser_registration_event(kind:u32){cc_desk_remote_security::browser_registration_event(kind)}`.replace(/\s/g,'');
 assert.equal(source,forwards,'wrapper must only forward the original registration functions');
 const pkg=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8'));
 assert.equal(pkg.scripts['build:browser:registration-runtime'],'node scripts/build-browser-registration-runtime.mjs');
 assert.equal(pkg.scripts['build:browser'],'node scripts/build-browser-runtime.mjs && vite build --mode browser-product');
 assert.equal(pkg.scripts['build:browser:product'],'node scripts/build-browser-runtime.mjs && node scripts/build-browser-registration-runtime.mjs && vite build --mode browser-product');
});

test('RegistrationRuntime_ProjectLocalPaths_004',async()=>{
 const {projectPath}=await load();
 const audit=path.join(root,'.audit/registration-runtime-checks');
 fs.mkdirSync(audit,{recursive:true});
 const scratch=fs.mkdtempSync(path.join(audit,'paths-'));
 const project=path.join(scratch,'project'),outside=path.join(scratch,'outside');
 fs.mkdirSync(project);fs.mkdirSync(outside);
 assert.equal(projectPath(project,'.audit/target'),path.join(project,'.audit/target'));
 assert.throws(()=>projectPath(project,'../outside'),/inside the project/);
 assert.throws(()=>projectPath(project,project),/inside the project/);
 fs.symlinkSync(outside,path.join(project,'escape'),process.platform==='win32'?'junction':'dir');
 assert.throws(()=>projectPath(project,'escape/new-file'),/symlink/);
});

test('RegistrationRuntime_CompilerPathMapping_005',async()=>{
 const {browserRuntimeCompilerEnvironment}=await import('../../scripts/browser-runtime-remap.mjs');
 const os=await import('node:os'),path=await import('node:path');
 const root=path.join(os.tmpdir(),'browser-remap-fixture','source'),target=path.join(root,'.audit','target');
 const cargo=path.join(os.tmpdir(),'browser-remap-fixture','cargo');
 const input={CARGO_HOME:cargo,RUSTFLAGS:'-C opt-level=0'};
 const result=browserRuntimeCompilerEnvironment(root,target,input);
 for(const [from,to] of [[root,'/build/source'],[target,'/build/target'],[cargo,'/build/cargo']]){
  assert.ok(result.flags.includes(`--remap-path-prefix=${from}=${to}`));
  assert.ok(result.flags.includes(`--remap-path-prefix=${from.replaceAll('\\','/')}=${to}`));
 }
 assert.deepEqual(result.env.CARGO_ENCODED_RUSTFLAGS.split(String.fromCharCode(31)).slice(0,2),['-C','opt-level=0']);
 assert.equal(input.CARGO_ENCODED_RUSTFLAGS,undefined);
});
