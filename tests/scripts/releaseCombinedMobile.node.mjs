import test from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { fileURLToPath } from 'node:url'
import { verifyAndroidSignature, verifyHostedAssets, verifyProof } from '../../scripts/release-combined-mobile.mjs'

const cli = fileURLToPath(new URL('../../scripts/release-combined-mobile.mjs', import.meta.url))
const sha = 'a'.repeat(40), signer = 'b'.repeat(64), version = '0.28.0-rc.4'
const expected = { sha, signer, version, versionCode:'4', run:'10', attempt:'2', ciRun:'5', ciAttempt:'1' }
const proof = { sourceSha:sha, version, versionCode:4, workflowRunId:'10', workflowRunAttempt:'2',
  ordinaryCiRunId:'5', ordinaryCiRunAttempt:'1', abi:'arm64-v8a', rustToolchain:'1.100.0',
  ndkRevision:'27.3.13750724', elf16KiB:true, apk16KiB:true, signerCertSha256:signer,
  signedSha256:'c'.repeat(64), elfSha256:'d'.repeat(64), lockSha256:'e'.repeat(64),
  deviceAcceptance:'unverified', publication:'not-published', nativeSecurityFixtures:'passed', jniExports:['nativeViewerRole','nativeTakeControllerFrame','nativeConfirmControllerFrame','nativeQueueControllerText','nativeQueueControllerKey','nativeControllerInputOutcome'] }
const row = (n)=>({name:n, size:12, sha256:'f'.repeat(64)})
const assets = [...Array.from({length:12},(_,i)=>row('synthetic-' + i)),row('cc-desk-0.28.0-rc.4-android-arm64-v8a.apk')]
const gate = {...expected, assets}
const draft = {tag_name:'v'+version, target_commitish:sha, draft:true, prerelease:true,
  assets:assets.map(a=>({name:a.name, size:a.size, digest:'sha256:'+a.sha256, state:'uploaded'}))}

test('read-only CLI admits manual numbered RC with explicit build code and refuses automatic and stable release', () => {
  const invoke = (v,e,c)=>execFileSync(process.execPath,[cli,'admit'], {stdio:'pipe', env:{...process.env,
    CC_DESK_RELEASE_VERSION:v, GITHUB_EVENT_NAME:e, ANDROID_VERSION_CODE:c}})
  assert.doesNotThrow(()=>invoke(version,'workflow_dispatch','28004'))
  for (const args of [[version,'push','4'],['0.28.0','workflow_dispatch','4'],[version,'workflow_dispatch',''],['0.28.0-beta.1','workflow_dispatch','4'],[version,'workflow_dispatch','2100000001'],[version,'workflow_dispatch','028004'],[version,'workflow_dispatch','2.8e4']]) {
    assert.throws(()=>invoke(...args), /combined mobile release denied/)
  }
})
test('exact proof fails closed on unsigned/missing security evidence, wrong signer, old SHA or a rerun attempt', () => {
  assert.doesNotThrow(()=>verifyProof(proof,expected))
  for (const bad of [{ nativeSecurityFixtures:null }, { signerCertSha256:'f'.repeat(64) },
    { sourceSha:'f'.repeat(40) }, { workflowRunAttempt:1 }, { ordinaryCiRunAttempt:'2' }, { apk16KiB:false }, { jniExports:[] }]) {
    assert.throws(()=>verifyProof({...proof,...bad},expected), /denied/)
  }
})
test('apksigner receipts require the actual pinned signer, exact v2/v3 and no debug certificate', () => {
  const out = 'Verified using v2 scheme (APK Signature Scheme v2): true\nVerified using v3 scheme (APK Signature Scheme v3): true\nSigner #1 certificate SHA-256 digest: '+signer+'\n'
  assert.doesNotThrow(()=>verifyAndroidSignature(out,signer))
  assert.throws(()=>verifyAndroidSignature(out,'f'.repeat(64)), /denied/)
  assert.throws(()=>verifyAndroidSignature(out+'Signer #1 certificate DN: CN=Android Debug\n',signer), /denied/)
  assert.throws(()=>verifyAndroidSignature(out+'\nSigner #2 certificate SHA-256 digest: '+signer+'\n',signer), /denied/)
})
test('draft CLI refuses a partial-asset upload, substituted APK bytes, and stable promotion',() => {
  const d = mkdtempSync(join(tmpdir(),'combined-release-fixture-'))
  try {
    writeFileSync(join(d,'combined-mobile-assets.json'),JSON.stringify(gate))
    const verify = (record,mode='verify-draft')=>{
      writeFileSync(join(d,'host.json'),JSON.stringify(record))
      return execFileSync(process.execPath,[cli,mode,'host.json'],{cwd:d,stdio:'pipe'})
    }
    assert.doesNotThrow(()=>verify(draft))
    assert.doesNotThrow(()=>verify({...draft,draft:false},'verify-published'))
    assert.throws(()=>verify({...draft,assets:draft.assets.slice(0,-1)}), /assets incomplete/)
    assert.throws(()=>verify({...draft,prerelease:false}), /identity, kind or visibility/)
    assert.throws(()=>verify({...draft,assets:draft.assets.map((x,i)=>i===12?{...x,digest:'sha256:'+'0'.repeat(64)}:x)}), /hosted bytes differ/)
  } finally { rmSync(d,{recursive:true,force:true}) }
})
test('release route cannot use the ordinary publication path for a mobile release',()=>{
  const workflow = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url),'utf8').replace(/\r/g,'')
  assert.match(workflow,/needs\.android\.result == 'success'/)
  assert.match(workflow,/Publish ordinary unused-version GitHub Release\n\s+if: .*mobile_release == 'false'/)
  assert.match(workflow,/Stage complete desktop and Android test release as an unpublished draft[\s\S]+?draft: true\n\s+prerelease: true\n\s+make_latest: false/)
  assert.match(workflow,/verify-draft combined-mobile-draft.json[\s\S]+?-F draft=false -F prerelease=true -f make_latest=false/)
  const android=readFileSync(new URL('../../.github/workflows/android-production.yml', import.meta.url),'utf8')
  assert.match(android,/workflow_call:/)
  assert.match(android,/environment: android-production/)
  assert.match(android,/test --locked --manifest-path android-client\/native\/Cargo.toml/)
})

test('mobile release transports versionCode as text across the manual and reusable workflow boundary', () => {
  const release = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8').replace(/\r/g, '')
  const android = readFileSync(new URL('../../.github/workflows/android-production.yml', import.meta.url), 'utf8').replace(/\r/g, '')
  assert.match(release, /^  workflow_dispatch:\n    inputs:\n      android_version_code:\n(?:        .*\n)*?        type: string$/m)
  for (const trigger of ['workflow_dispatch', 'workflow_call']) {
    const input = android.split('  ' + trigger + ':\n')[1].split(/\n  \w|\npermissions:/)[0]
    assert.match(input, /^      version_code:\n(?:        .*\n)*?        type: string$/m)
  }
  assert.match(release, /^      version_code: \$\{\{ inputs\.android_version_code \}\}$/m)
})
