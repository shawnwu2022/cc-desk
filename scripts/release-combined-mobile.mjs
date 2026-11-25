
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { createReadStream, lstatSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const APK_PROOF = 'android-release-build-proof.json'
const PROGRAM = /^[a-f0-9]{40}$/
const SHA256 = /^[a-f0-9]{64}$/
const COUNT = 13
const CONTROLLER_ABI = ['nativeViewerRole','nativeTakeControllerFrame','nativeConfirmControllerFrame','nativeQueueControllerText','nativeQueueControllerKey','nativeControllerInputOutcome']
const MOBILE_RC = /^0\.28\.(?:0|[1-9]\d*)-rc\.[1-9]\d*$/
const INTEGER = /^[1-9]\d{0,9}$/
const assert = (ok, msg) => { if (!ok) throw new Error('combined mobile release denied: ' + msg) }
const plain = file => { const s = lstatSync(file); assert(s.isFile() && !s.isSymbolicLink() && s.size > 0, 'expected bounded regular file'); return s.size }
const boundedJson = (file, max = 64 * 1024) => { assert(plain(file) <= max, 'JSON file is too large'); return JSON.parse(readFileSync(file, 'utf8')) }
const digestFile = file => new Promise((resolve, reject) => {
  const hash = createHash('sha256')
  const stream = createReadStream(file)
  stream.on('error', reject)
  stream.on('data', data => hash.update(data))
  stream.on('end', () => resolve(hash.digest('hex')))
})
export function admitMobileDispatch(version, event, versionCode) {
  assert(MOBILE_RC.test(version ?? '') && event === 'workflow_dispatch', 'mobile only supports a manually selected numbered 0.28 RC')
  assert(INTEGER.test(versionCode ?? '') && Number(versionCode) <= 2100000000, 'explicit monotonic Android versionCode is required')
}
export function verifyProof(proof, expected) {
  assert(PROGRAM.test(expected.sha ?? '') && SHA256.test(expected.signer ?? '') && !/^0+$/.test(expected.signer), 'trusted source/signer identities missing')
  assert(proof.sourceSha === expected.sha && proof.version === expected.version && proof.versionCode === Number(expected.versionCode) &&
    String(proof.workflowRunId) === expected.run && String(proof.workflowRunAttempt) === expected.attempt &&
    String(proof.ordinaryCiRunId) === expected.ciRun && String(proof.ordinaryCiRunAttempt) === expected.ciAttempt, 'APK proof does not bind release/ordinary CI execution')
  assert(proof.abi === 'arm64-v8a' && proof.rustToolchain === '1.100.0' && proof.ndkRevision === '27.3.13750724' &&
    proof.elf16KiB === true && proof.apk16KiB === true && proof.signerCertSha256 === expected.signer &&
    SHA256.test(proof.signedSha256 ?? '') && SHA256.test(proof.elfSha256 ?? '') &&
    SHA256.test(proof.lockSha256 ?? '') && proof.deviceAcceptance === 'unverified' &&
    proof.publication === 'not-published' && proof.nativeSecurityFixtures === 'passed' && Array.isArray(proof.jniExports) && CONTROLLER_ABI.every(name => proof.jniExports.includes(name)), 'APK proof gates or test disclosure invalid')
}
export function verifyAndroidSignature(stdout, expectedSigner) {
  const signers = [...stdout.matchAll(/^Signer #(\d+) certificate SHA-256 digest: ([a-f0-9]{64})\s*$/igm)]
  assert(signers.length === 1 && signers[0][1] === '1' && signers[0][2].toLowerCase() === expectedSigner &&
    !/certificate DN:.*(?:Android Debug|CN=debug)/i.test(stdout) &&
    /^Verified using v2 scheme .*: true\s*$/m.test(stdout) && /^Verified using v3 scheme .*: true\s*$/m.test(stdout), 'APK actual signer or signature scheme mismatch')
}
function assetsAt(root) {
  const files = []
  function walk(dir) {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      assert(!e.isSymbolicLink(), 'candidate contains symlink')
      const file = join(dir, e.name)
      if (e.isDirectory()) { walk(file); continue }
      if (/\.(?:exe|exe\.sig|dmg|app\.tar\.gz|app\.tar\.gz\.sig|AppImage|AppImage\.sig)$/.test(e.name)) files.push(file)
    }
  }
  walk(root)
  return files
}
function tagBindings(release, gate, draft) {
  assert(release.tag_name === 'v' + gate.version && release.target_commitish === gate.sha &&
    release.draft === draft && release.prerelease === true, 'remote release identity, kind or visibility mismatch')
}
export function verifyHostedAssets(release, expected, draft = true) {
  tagBindings(release, expected, draft)
  const remote = release.assets
  assert(Array.isArray(remote) && remote.length === COUNT && expected.assets.length === COUNT, 'release assets incomplete')
  for (const item of expected.assets) {
    const found = remote.filter(a => a.name === item.name)
    assert(found.length === 1 && found[0].state === 'uploaded' && found[0].size === item.size &&
      found[0].digest === 'sha256:' + item.sha256, 'hosted bytes differ for ' + item.name)
  }
}
function getExpected() {
  const expected = {
    sha: process.env.GITHUB_SHA, version: process.env.CC_DESK_RELEASE_VERSION,
    versionCode: process.env.ANDROID_VERSION_CODE, run: process.env.GITHUB_RUN_ID,
    attempt: process.env.GITHUB_RUN_ATTEMPT, ciRun: process.env.ANDROID_ORDINARY_CI_RUN,
    ciAttempt: process.env.ANDROID_ORDINARY_CI_ATTEMPT, signer: process.env.ANDROID_TRUSTED_SIGNER
  }
  admitMobileDispatch(expected.version, process.env.GITHUB_EVENT_NAME, expected.versionCode)
  return expected
}
async function prepare() {
  const expected = getExpected()
  const apkName = 'cc-desk-' + expected.version + '-android-arm64-v8a.apk'
  const apk = join('android-candidate', apkName)
  const proofFile = join('android-candidate', APK_PROOF)
  assert(readdirSync('android-candidate').sort().join('\n') === [apkName, APK_PROOF].sort().join('\n'), 'unexpected Android artifact contents')
  const proof = boundedJson(proofFile)
  verifyProof(proof, expected)
  assert(await digestFile(join('android-client','native','Cargo.lock')) === proof.lockSha256, 'APK proof uses a different source lockfile')
  assert(plain(apk) <= 96 * 1024 * 1024 && await digestFile(apk) === proof.signedSha256, 'candidate APK does not match signing proof bytes')
  const bt = join(process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT || '', 'build-tools', '35.0.0')
  assert(resolve(bt) !== resolve('build-tools', '35.0.0'), 'Android SDK not configured')
  const apkOutput = execFileSync(join(bt, 'apksigner'), ['verify','--verbose','--print-certs','--min-sdk-version','31',apk], { encoding:'utf8' })
  verifyAndroidSignature(apkOutput, expected.signer)
  execFileSync(join(bt, 'zipalign'), ['-c','-P','16','4',apk], { stdio:'inherit' })
  const windows = join('artifacts', 'cc-desk-candidate-' + expected.sha + '-windows')
  const desktop = assetsAt('artifacts')
  const types = ['.exe','.exe.sig','.dmg','.app.tar.gz','.app.tar.gz.sig','.AppImage','.AppImage.sig']
  assert(desktop.length === types.length && types.every(ext => desktop.filter(f => f.endsWith(ext)).length === 1), 'desktop asset set must include exactly one signed output per expected platform')
  const all = [...desktop, join(windows,'cc-desk-mobile-site.zip'), join(windows,'cc-desk-mobile-site.manifest.json'),
    'latest.json', join('coverage','windows-native-coverage.json'), apk, proofFile]
  assert(all.length === COUNT, 'bad local release asset count')
  const assets = await Promise.all(all.map(async file => ({
    file, name: file.split(/[\\/]/).at(-1).replace(/ /g, '.'),
    size: plain(file), sha256: await digestFile(file)
  })))
  assert(new Set(assets.map(x => x.name)).size === COUNT, 'asset names collide')
  writeFileSync('combined-mobile-assets.json', JSON.stringify({ ...expected, assets }), { flag:'wx' })
}
async function main() {
  const [mode, arg, extra] = process.argv.slice(2)
  assert(!extra, 'invalid command arguments')
  if (mode === 'admit') { const {CC_DESK_RELEASE_VERSION: version, GITHUB_EVENT_NAME: event, ANDROID_VERSION_CODE: code} = process.env; if (version?.startsWith('0.28.')) admitMobileDispatch(version,event,code); return }
  if (mode === 'artifact-id') { assert(/^[1-9][0-9]{0,19}$/.test(process.env.ANDROID_ARTIFACT_ID ?? ''), 'missing same-run Android artifact ID'); return }
  if (mode === 'prepare') { await prepare(); return }
  if (mode === 'verify-draft' || mode === 'verify-published') {
    assert(arg, 'missing hosted-release receipt')
    const expected = boundedJson('combined-mobile-assets.json'), record = boundedJson(arg, 1024 * 1024)
    verifyHostedAssets(record, expected, mode === 'verify-draft')
    return
  }
  assert(false, 'unknown command')
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main()
