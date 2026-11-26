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
const expected = { sha, signer, version, versionCode:'28004', run:'10', attempt:'2', ciRun:'5', ciAttempt:'1' }
const proof = { sourceSha:sha, version, versionCode:28004, workflowRunId:'10', workflowRunAttempt:'2',
  ordinaryCiRunId:'5', ordinaryCiRunAttempt:'1', abi:'arm64-v8a', rustToolchain:'1.100.0',
  ndkRevision:'27.3.13750724', elf16KiB:true, apk16KiB:true, signerCertSha256:signer,
  signedSha256:'c'.repeat(64), elfSha256:'d'.repeat(64), lockSha256:'e'.repeat(64),
  deviceAcceptance:'unverified', publication:'not-published', nativeSecurityFixtures:'passed', jniExports:['nativeViewerRole','nativeTakeControllerFrame','nativeConfirmControllerFrame','nativeQueueControllerText','nativeQueueControllerKey','nativeControllerInputOutcome'] }
const row = (n)=>({name:n, size:12, sha256:'f'.repeat(64)})
const assets = [...Array.from({length:12},(_,i)=>row('synthetic-' + i)),row('cc-desk-0.28.0-rc.4-android-arm64-v8a.apk')]
const gate = {...expected, assets}
const draft = {tag_name:'v'+version, target_commitish:sha, draft:true, prerelease:true,
  assets:assets.map(a=>({name:a.name, size:a.size, digest:'sha256:'+a.sha256, state:'uploaded'}))}

test('manual numbered RC dispatch derives its build code without a form value', () => {
  const invoke = (v,e)=>execFileSync(process.execPath,[cli,'admit'], {stdio:'pipe', env:{...process.env,
    CC_DESK_RELEASE_VERSION:v, GITHUB_EVENT_NAME:e, ANDROID_VERSION_CODE:''}}).toString().trim()
  assert.equal(invoke(version,'workflow_dispatch'),'28004')
  for (const bad of ['0.28.0','0.28.0-beta.1','0.28.0-rc.3','0.28.00-rc.4','0.28.0-rc.100']) {
    assert.throws(()=>invoke(bad,'workflow_dispatch'), /combined mobile release denied/)
  }
  assert.throws(()=>invoke(version,'push'), /combined mobile release denied/)
  assert.equal(invoke('0.27.3','push'),'')
})
test('Android candidate derives a stable and ordered code from checked-out package version', () => {
  const d = mkdtempSync(join(tmpdir(),'android-code-fixture-'))
  try {
    const invoke = (version, attempt) => {
      writeFileSync(join(d,'package.json'), JSON.stringify({version}))
      return execFileSync(process.execPath,[cli,'android-version-code'], {
        cwd:d, stdio:'pipe', env:{...process.env,GITHUB_RUN_ATTEMPT:attempt,ANDROID_VERSION_CODE:'99999'}
      }).toString().trim()
    }
    assert.equal(invoke(version,'1'),'28004')
    assert.equal(invoke(version,'2'),'28004')
    assert.equal(invoke('0.28.0-rc.5','1'),'28005')
    assert.equal(invoke('0.28.0-rc.99','1'),'28099')
    assert.equal(invoke('0.28.1-rc.1','1'),'28101')
    assert.throws(()=>invoke('0.28.99999999999999999999-rc.1','1'), /combined mobile release denied/)
    assert.throws(()=>invoke('0.29.0-rc.1','1'), /combined mobile release denied/)
  } finally { rmSync(d,{recursive:true,force:true}) }
})
test('exact proof fails closed on unsigned/missing security evidence, wrong signer, old SHA or a rerun attempt', () => {
  assert.doesNotThrow(()=>verifyProof(proof,expected))
  for (const bad of [{ nativeSecurityFixtures:null }, { signerCertSha256:'f'.repeat(64) },
    { sourceSha:'f'.repeat(40) }, { versionCode:28005 }, { workflowRunAttempt:1 }, { ordinaryCiRunAttempt:'2' }, { apk16KiB:false }, { jniExports:[] }]) {
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

test('manual release and candidate workflows require no typed versionCode and preserve the signing gate', () => {
  const release = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8').replace(/\r/g, '')
  const android = readFileSync(new URL('../../.github/workflows/android-production.yml', import.meta.url), 'utf8').replace(/\r/g, '')
  assert.doesNotMatch(release, /inputs\.android_version_code|ANDROID_VERSION_CODE:/)
  assert.doesNotMatch(android, /inputs\.version_code|^    inputs:/m)
  assert.match(release, /^  workflow_dispatch:\s*$/m)
  assert.match(android, /^  workflow_dispatch:\s*$/m)
  assert.match(android, /actions\/checkout@v4[\s\S]+?node scripts\/release-combined-mobile\.mjs android-version-code[\s\S]+?VERSION_CODE=.*GITHUB_ENV/)
  assert.match(android, /environment: android-production/)
})

test('release forwards only the three protected Android signer secrets to the reusable workflow', () => {
  const load = name => readFileSync(new URL('../../.github/workflows/' + name, import.meta.url), 'utf8').replaceAll('\r', '')
  const release = load('release.yml').split('\n  android:\n')[1].split('\n  release:')[0]
  const signer = load('android-production.yml')
  const names = ['ANDROID_RELEASE_KEYSTORE_B64', 'ANDROID_RELEASE_KEYSTORE_PASSWORD', 'ANDROID_RELEASE_KEY_PASSWORD']
  const expected = names.map(name => '      ' + name + ': '+ '$' + '{{ secrets.' + name + ' }}')
  assert.deepEqual(release.split('\n').filter(line => line.startsWith('      ANDROID_RELEASE_')), expected)
  assert.ok(!release.includes('secrets: inherit'))
  const call = signer.split('  workflow_call:\n')[1].split('    outputs:')[0]
  assert.deepEqual(call.split('\n').slice(0, 7), ['    secrets:', ...names.flatMap(name => ['      ' + name + ':', '        required: true'])])
  assert.ok(signer.includes('    environment: android-production'))
})


test('both release stages request an actual v2 and v3 verification without lowering app minimum', () => {
  const build = readFileSync(new URL('../../android-client/build-production.ps1', import.meta.url), 'utf8')
  const publish = readFileSync(new URL('../../scripts/release-combined-mobile.mjs', import.meta.url), 'utf8')
  assert.match(build, /'sign','--ks'[\s\S]*?'--min-sdk-version','31','--v2-signing-enabled','true','--v3-signing-enabled','true'/)
  assert.match(build, /'verify','--verbose','--print-certs','--min-sdk-version','27'/)
  assert.match(publish, /'verify','--verbose','--print-certs','--min-sdk-version','27'/)
  const v2 = 'Verified using v2 scheme (APK Signature Scheme v2): true\n'
  const v3 = 'Verified using v3 scheme (APK Signature Scheme v3): true\n'
  const cert = 'Signer #1 certificate SHA-256 digest: ' + 'a'.repeat(64) + '\n'
  assert.doesNotThrow(() => verifyAndroidSignature(v2 + v3 + cert, 'a'.repeat(64)))
  assert.throws(() => verifyAndroidSignature(v2 + cert, 'a'.repeat(64)), /denied/)
  assert.throws(() => verifyAndroidSignature(v3 + cert, 'a'.repeat(64)), /denied/)
})
