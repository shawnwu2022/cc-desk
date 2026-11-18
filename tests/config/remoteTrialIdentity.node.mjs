import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {captureIdentity,verifyIdentity,identityEnvironment} from '../../scripts/remote-trial-identity.mjs';
function fixture(t){
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'cc-trial-identity-'));
 t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
 const git=(...args)=>execFileSync('git',['-C',root,...args],{stdio:'pipe',windowsHide:true});
 git('init');git('config','user.name','Isolated fixture');git('config','user.email','fixture@example.invalid');
 fs.writeFileSync(path.join(root,'source.txt'),'original\n');fs.writeFileSync(path.join(root,'.gitignore'),'generated/\n');
 git('add','.');git('-c','core.hooksPath=/dev/null','commit','-m','fixture');return {root,git};
}
test('RemoteTrialIdentity_CleanAndChildEnvironment_001',t=>{
 const {root}=fixture(t),identity=captureIdentity(root),env={CC_DESK_BUILD_SHA:'spoof',GITHUB_SHA:'spoof2'};
 const child=identityEnvironment(env,identity);assert.equal(child.CC_DESK_BUILD_SHA,identity.head);assert.equal(child.GITHUB_SHA,identity.head);assert.equal(env.CC_DESK_BUILD_SHA,'spoof');verifyIdentity(identity);
 fs.mkdirSync(path.join(root,'generated'));fs.writeFileSync(path.join(root,'generated','artifact'),'ignored');verifyIdentity(identity);
});
test('RemoteTrialIdentity_TrackedDirtyRejected_002',t=>{
 const {root}=fixture(t);fs.writeFileSync(path.join(root,'source.txt'),'changed');assert.throws(()=>captureIdentity(root),/clean/);
});
test('RemoteTrialIdentity_UntrackedRejected_003',t=>{
 const {root}=fixture(t);fs.writeFileSync(path.join(root,'new-source.txt'),'new');assert.throws(()=>captureIdentity(root),/clean/);
});
test('RemoteTrialIdentity_HeadChangeRejected_004',t=>{
 const {root,git}=fixture(t),identity=captureIdentity(root);git('-c','core.hooksPath=/dev/null','commit','--allow-empty','-m','new head');assert.throws(()=>verifyIdentity(identity),/changed/);
});
test('RemoteTrialIdentity_SourceChangeRejected_005',t=>{
 const {root}=fixture(t),identity=captureIdentity(root);fs.writeFileSync(path.join(root,'source.txt'),'changed');assert.throws(()=>verifyIdentity(identity));
});
test('RemoteTrialIdentity_NotGitRejected_006',t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'cc-trial-not-git-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));assert.throws(()=>captureIdentity(root),/Git/);
});

test('RemoteTrialIdentity_SubdirectoryRejected_007',t=>{
 const {root}=fixture(t);fs.mkdirSync(path.join(root,'generated'));assert.throws(()=>captureIdentity(path.join(root,'generated')),/exact Git root/);
});
test('RemoteTrialIdentity_IndirectIndexRejected_008',t=>{
 for(const mode of ['120000','160000']){
  const {root,git}=fixture(t);const object=git('rev-parse',mode==='160000'?'HEAD':'HEAD:source.txt').toString().trim();
  git('update-index','--add','--cacheinfo',mode,object,'indirect');git('-c','core.hooksPath=/dev/null','commit','-m','indirect fixture');
  // Worktree absence must fail closed before any indirect file is read.
  assert.throws(()=>captureIdentity(root));
 }
});
test('RemoteTrialIdentity_HiddenIndexRejected_009',t=>{
 const {root,git}=fixture(t);git('update-index','--assume-unchanged','source.txt');assert.throws(()=>captureIdentity(root),/hidden index/);
});

test('RemoteTrialIdentity_ChildGitCannotSelectForeignRepository_010',t=>{
 const own=fixture(t),foreign=fixture(t);foreign.git('-c','core.hooksPath=/dev/null','commit','--allow-empty','-m','foreign');
 const identity=captureIdentity(own.root);
 const env={...process.env,GIT_DIR:path.join(foreign.root,'.git'),GIT_WORK_TREE:foreign.root,GIT_CONFIG_COUNT:'1',GIT_CONFIG_KEY_0:'core.bare',GIT_CONFIG_VALUE_0:'false',GITHUB_SHA:'spoof'};
 const child=identityEnvironment(env,identity);
 const observed=execFileSync('git',['rev-parse','HEAD'],{cwd:own.root,env:child,encoding:'utf8',windowsHide:true}).trim();
 assert.equal(observed,identity.head);assert.equal(child.GITHUB_SHA,identity.head);
 assert.equal(Object.keys(child).some(key=>key.toUpperCase().startsWith('GIT_')),false);
 assert.equal(env.GIT_DIR,path.join(foreign.root,'.git'));assert.equal(env.GITHUB_SHA,'spoof');
});

const generatedPaths=['src-tauri/Cargo.toml','src-tauri/gen/schemas/desktop-schema.json','src-tauri/gen/schemas/windows-schema.json'];
function generatedFixture(t){
 const f=fixture(t);f.git('config','core.autocrlf','true');
 for(const file of generatedPaths){fs.mkdirSync(path.dirname(path.join(f.root,file)),{recursive:true});fs.writeFileSync(path.join(f.root,file),'value = 1\n');}
 f.git('add','.');f.git('-c','core.hooksPath=/dev/null','commit','-m','generated fixture');return f;
}
test('RemoteTrialIdentity_ThreeGeneratedEolEquivalent_011',t=>{
 const {root}=generatedFixture(t),identity=captureIdentity(root);
 for(const file of generatedPaths)fs.writeFileSync(path.join(root,file),'value = 1\r\n');
 verifyIdentity(identity);
 for(const file of generatedPaths)fs.writeFileSync(path.join(root,file),'value = 1\n');
 verifyIdentity(identity);
});
test('RemoteTrialIdentity_GeneratedContentNeverIgnored_012',t=>{
 for(const value of ['value = 2\n','value  = 1\n','{"changed":true}\n','value = 1\r']){
  for(const file of generatedPaths){const {root}=generatedFixture(t);fs.writeFileSync(path.join(root,file),value);assert.throws(()=>captureIdentity(root));}
 }
});
test('RemoteTrialIdentity_OtherSourceEolChangesTail_013',t=>{
 const {root}=generatedFixture(t),identity=captureIdentity(root);fs.writeFileSync(path.join(root,'source.txt'),'original\r\n');assert.throws(()=>verifyIdentity(identity),/changed/);
});
test('RemoteTrialIdentity_StagedChangeRejected_014',t=>{
 const {root,git}=fixture(t);fs.writeFileSync(path.join(root,'source.txt'),'staged');git('add','source.txt');assert.throws(()=>captureIdentity(root),/clean/);
});
