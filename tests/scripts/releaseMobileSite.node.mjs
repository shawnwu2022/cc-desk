import assert from 'node:assert/strict'
import test from 'node:test'
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync} from 'node:fs'
import {join} from 'node:path'
import {tmpdir} from 'node:os'
import {spawnSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import {packageSite,verifySite,siteFiles,mobileReleaseFamily,ZIP_NAME,MANIFEST_NAME} from '../../scripts/release-mobile-site.mjs'

const owner={sourceSha:'a'.repeat(40),version:'0.28.0-rc.3',runId:'7',attempt:'2'}
const names=['browser-remote.html','browser-registration.html','browser-profile.js','browser-registration.js','browser-registration.manifest.json',...['remote-runtime','remote-registration-runtime'].flatMap(d=>['manifest.json','fixture_bg.js','fixture_bg.wasm'].map(f=>d+'/'+f)),'assets/browser-test-token.js','assets/AppButton.vue_vue_type_script_setup_true_lang-CAaOXTt2.js','assets/browserEntry.worker-ByEPvlpO.js']
function fixture(fn){
 const dir=mkdtempSync(join(tmpdir(),'ccdesk-mobile-archive-')), dist=join(dir,'site'), output=join(dir,'candidate');
 try {
  mkdirSync(dist);mkdirSync(output);
  for(const name of names){
   if(name.includes('/'))mkdirSync(join(dist,name.split('/')[0]),{recursive:true});
   writeFileSync(join(dist,name),'bounded synthetic text');
  }
  writeFileSync(join(output,'normal-setup.exe'),'unrelated NSIS fixture');writeFileSync(join(output,'normal-setup.exe.sig'),'unrelated signature');
  fn(dist,output)
 } finally {rmSync(dir,{recursive:true,force:true})}
}
test('mobile release family is positive and does not change recovery',()=>{
 for(const version of ['0.28.0','0.28.0-rc.3','0.28.1'])assert.equal(mobileReleaseFamily(version),true);
 for(const version of ['0.18.1','0.28.0-beta.1','0.28.0-rc.03','0.28.0-preview.1','1.28.0','0.280.0','0.29.0'])assert.equal(mobileReleaseFamily(version),false);
});
test('exact ZIP and manifest required even with valid-path installer artifacts',()=>fixture((site,out)=>{
 assert.throws(()=>verifySite(out,owner));
 packageSite(site,out,owner);
 assert.equal(verifySite(out,owner).sourceSha,owner.sourceSha);
 assert.throws(()=>verifySite(out,{...owner,attempt:'3'}));
 const archive=readFileSync(join(out,ZIP_NAME)); const manifest=readFileSync(join(out,MANIFEST_NAME));
 rmSync(join(out,MANIFEST_NAME));assert.throws(()=>verifySite(out,owner));writeFileSync(join(out,MANIFEST_NAME),manifest);
 rmSync(join(out,ZIP_NAME));assert.throws(()=>verifySite(out,owner));archive[45]^=1;writeFileSync(join(out,ZIP_NAME),archive);
 assert.throws(()=>verifySite(out,owner));
}));
test('reject unexpected and local-path diagnostic payload',()=>fixture((site,out)=>{
 const extra=join(site,'assets','leak.js.map');writeFileSync(extra,'synthetic');assert.throws(()=>siteFiles(site));rmSync(extra);
 const ambiguous=join(site,'assets','chunk..worker-test.js');writeFileSync(ambiguous,'synthetic');assert.throws(()=>siteFiles(site));rmSync(ambiguous);
 writeFileSync(join(site,'browser-profile.js'),'D:\\Users\\example\\private-data');assert.throws(()=>siteFiles(site));
}));

test('CLI package and publish-side checks are bound to the run and fail closed',()=>fixture((site,out)=>{
 const entry=fileURLToPath(new URL('../../scripts/release-mobile-site.mjs',import.meta.url));
 const env={...process.env,GITHUB_SHA:owner.sourceSha,CC_DESK_RELEASE_VERSION:owner.version,GITHUB_RUN_ID:owner.runId,GITHUB_RUN_ATTEMPT:owner.attempt};
 const run=(args,overrides)=>spawnSync(process.execPath,[entry,...args],{env:{...env,...overrides},encoding:'utf8',windowsHide:true});
 assert.equal(run(['verify',out]).status,1);
 assert.equal(run(['package',out,site]).status,0);
 const good=run(['verify',out]);assert.equal(good.status,0);assert.match(good.stdout,/deployment not verified/);
 assert.equal(run(['verify',out],{GITHUB_SHA:'b'.repeat(40)}).status,1);
 rmSync(join(out,MANIFEST_NAME));
 const missing=run(['verify',out]);assert.equal(missing.status,1);assert.match(missing.stderr,/archive or manifest failed verification/);
}));
test('workflow gates mobile compile/upload/promote separately from old drafts and updater',()=>{
 const workflow=readFileSync(new URL('../../.github/workflows/release.yml',import.meta.url),'utf8');
 assert.match(workflow,/observer_features=\(--features remote-observer\)/);
 assert.doesNotMatch(workflow,/--features (?:\S*,)?remote-local-acceptance/);
 const windowsValidation=workflow.indexOf('Install and check signed Windows candidate before upload');
 const browserBuild=workflow.indexOf('Build and inspect the isolated same-source browser-product stage');
 const mobileUpload=workflow.indexOf('Upload exact Windows candidate and mobile site');
 assert.ok(windowsValidation>=0&&windowsValidation<browserBuild&&browserBuild<mobileUpload);
 assert.match(workflow,/Build and inspect the isolated same-source browser-product stage\s+if: matrix.name == 'windows' && needs.admission.outputs.mobile_release == 'true'/);
 assert.match(workflow,/Compose English release highlights with complete technical disclosure\s+if: steps.promotion.outputs.recovery_draft == ''[\s\S]+if \[ "\$CC_DESK_MOBILE_RELEASE" = "true" \]; then[^]*node scripts\/release-mobile-site.mjs verify/);
 assert.match(workflow,/name: cc-desk-candidate-\$\{\{ github.sha \}\}-windows[\s\S]+cc-desk-mobile-site\.zip/);
 assert.match(workflow,/Stage this run's nine new assets without overwriting originals/);
 assert.match(workflow,/node scripts\/generate-updater-manifest\.js artifacts latest\.json/);
 assert.ok(workflow.indexOf('node scripts/release-mobile-site.mjs verify')<workflow.indexOf('Publish ordinary unused-version GitHub Release'));
});
