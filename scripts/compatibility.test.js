#!/usr/bin/env node

/**
 * 旧版本兼容性验证测试
 * 模拟旧版 updater.rs 的 OssLatestInfo 反序列化逻辑，
 * 验证 generateLatestJson 产生的 JSON 对旧版本完全兼容。
 *
 * 旧版 Rust 结构体定义：
 *   struct OssLatestInfo {
 *     version: String,
 *     release_date: String,
 *     release_notes: String,
 *     release_notes_url: String,
 *     assets: OssAssets { windows, macos, linux: OssAssetInfo { url, size } }
 *   }
 *
 * 旧版版本比较：is_newer_version("0.7.0", "0.8.0") == true
 *
 * 运行: node scripts/compatibility.test.js
 */

const { generateLatestJson } = require('./release')
const assert = require('assert')

let passed = 0
let failed = 0

function test(name, fn) {
  try {
    fn()
    passed++
    console.log(`  ✓ ${name}`)
  } catch (err) {
    failed++
    console.log(`  ✗ ${name}`)
    console.log(`    ${err.message}`)
  }
}

// 构造完整的测试文件列表
function makeFiles() {
  return [
    { name: 'CC Desk_0.8.0_x64-setup.exe', path: '/tmp/test/CC Desk_0.8.0_x64-setup.exe' },
    { name: 'CC Desk_0.8.0_x64-setup.exe.sig', path: '/tmp/test/CC Desk_0.8.0_x64-setup.exe.sig' },
    { name: 'CC Desk_0.8.0_x64.app.tar.gz', path: '/tmp/test/CC Desk_0.8.0_x64.app.tar.gz' },
    { name: 'CC Desk_0.8.0_x64.app.tar.gz.sig', path: '/tmp/test/CC Desk_0.8.0_x64.app.tar.gz.sig' },
    { name: 'CC Desk_0.8.0_x64.dmg', path: '/tmp/test/CC Desk_0.8.0_x64.dmg' },
    { name: 'CC Desk_0.8.0_amd64.AppImage', path: '/tmp/test/CC Desk_0.8.0_amd64.AppImage' },
    { name: 'CC Desk_0.8.0_amd64.AppImage.sig', path: '/tmp/test/CC Desk_0.8.0_amd64.AppImage.sig' },
  ]
}

const fakeReadFileContent = (filePath) => {
  if (!filePath || !filePath.endsWith('.sig')) return ''
  return 'dW50cnVzdGVkLXNpZ25hdHVyZQ=='
}

const fakeGetFileSize = (filePath) => {
  if (!filePath) return 0
  if (filePath.includes('.exe')) return 50000000
  if (filePath.includes('.app.tar.gz')) return 60000000
  if (filePath.includes('.dmg')) return 70000000
  if (filePath.includes('.AppImage')) return 55000000
  return 0
}

// 模拟旧版 Rust serde 反序列化：只提取旧版 OssLatestInfo 需要的字段
function deserializeAsOldVersion(json) {
  return {
    version: json.version,
    release_date: json.release_date,
    release_notes: json.release_notes,
    release_notes_url: json.release_notes_url,
    assets: {
      windows: { url: json.assets.windows.url, size: json.assets.windows.size },
      macos: { url: json.assets.macos.url, size: json.assets.macos.size },
      linux: { url: json.assets.linux.url, size: json.assets.linux.size },
    },
  }
}

// 模拟旧版 is_newer_version(current, remote)
function isOlderVersionCompare(current, remote) {
  const parse = (v) => v.trimStartMatches('v').split('.').filter(s => /^\d+$/.test(s)).map(Number)
  // Node.js doesn't have trimStartMatches, emulate
  const parseParts = (v) => v.replace(/^v/, '').split('.').filter(s => /^\d+$/.test(s)).map(Number)
  const cur = parseParts(current)
  const rem = parseParts(remote)
  for (let i = 0; i < Math.max(cur.length, rem.length); i++) {
    const c = cur[i] || 0
    const r = rem[i] || 0
    if (r > c) return true
    if (r < c) return false
  }
  return false
}

function generateJson() {
  return generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '### Features\n- Switch to official updater',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
}

// ============================================
// 测试：旧版 OssLatestInfo 反序列化
// ============================================

console.log('\n旧版反序列化兼容性:')

// 旧版能从新 JSON 中提取 version 字段
test('OldCompat_VersionPresent_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.version, 'version field missing')
  assert.strictEqual(typeof old.version, 'string')
})

// 旧版能从新 JSON 中提取 release_date 字段，格式为 YYYY-MM-DD
test('OldCompat_ReleaseDatePresent_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.release_date, 'release_date field missing')
  assert.ok(/^\d{4}-\d{2}-\d{2}$/.test(old.release_date), `expected YYYY-MM-DD, got "${old.release_date}"`)
})

// 旧版能从新 JSON 中提取 release_notes 字段
test('OldCompat_ReleaseNotesPresent_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.strictEqual(old.release_notes, '### Features\n- Switch to official updater')
})

// 旧版能从新 JSON 中提取 release_notes_url 字段
test('OldCompat_ReleaseNotesUrl_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.release_notes_url.startsWith('https://github.com/'), `expected GitHub URL, got "${old.release_notes_url}"`)
})

// 旧版能从新 JSON 中提取 assets.windows.url，指向 .exe 文件
test('OldCompat_WindowsAssetUrl_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.assets.windows.url.endsWith('-setup.exe'), `expected .exe url, got "${old.assets.windows.url}"`)
})

// 旧版能从新 JSON 中提取 assets.macos.url，指向 .dmg 文件
test('OldCompat_MacAssetUrl_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.assets.macos.url.endsWith('.dmg'), `expected .dmg url, got "${old.assets.macos.url}"`)
})

// 旧版能从新 JSON 中提取 assets.linux.url，指向 .AppImage 文件
test('OldCompat_LinuxAssetUrl_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.assets.linux.url.endsWith('.AppImage'), `expected .AppImage url, got "${old.assets.linux.url}"`)
})

// 旧版能从新 JSON 中提取每个平台的 size 字段（非零）
test('OldCompat_AssetSizeNonZero_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.assets.windows.size > 0, 'windows size should be > 0')
  assert.ok(old.assets.macos.size > 0, 'macos size should be > 0')
  assert.ok(old.assets.linux.size > 0, 'linux size should be > 0')
})

// ============================================
// 测试：旧版版本比较兼容性
// ============================================

console.log('\n旧版版本比较兼容性:')

// version 字段为 "0.8.0"（无 v 前缀），与旧版 current "0.7.0" 比较返回 true
test('OldCompat_VersionCompare_001', () => {
  const raw = generateJson()
  assert.ok(isOlderVersionCompare('0.7.0', raw.version), `0.7.0 < ${raw.version} should be true`)
})

// version 字段与旧版 JSON 格式（带 v 前缀）比较也正确
test('OldCompat_VersionCompareWithV_001', () => {
  assert.ok(isOlderVersionCompare('v0.7.0', '0.8.0'), 'v0.7.0 < 0.8.0 should be true')
  assert.ok(isOlderVersionCompare('0.7.0', 'v0.8.0'), '0.7.0 < v0.8.0 should be true')
})

// ============================================
// 测试：serde 反序列化容错（未知字段不报错）
// ============================================

console.log('\nserde 未知字段容错:')

// 新 JSON 包含旧版不认识的字段（platforms, notes, pub_date），不影响旧版解析
test('OldCompat_ExtraFieldsIgnored_001', () => {
  const raw = generateJson()
  // 旧版只需要 5 个顶级字段，新 JSON 有 8 个顶级字段
  const oldFields = ['version', 'release_date', 'release_notes', 'release_notes_url', 'assets']
  const allFields = Object.keys(raw)
  const extraFields = allFields.filter(f => !oldFields.includes(f))
  // 验证确实存在额外字段（platforms, notes, pub_date）
  assert.ok(extraFields.length >= 3, `expected >= 3 extra fields, got ${extraFields.length}: ${extraFields.join(', ')}`)
  // 旧版 serde 反序列化只提取需要的字段，忽略多余字段
  const old = deserializeAsOldVersion(raw)
  assert.ok(old.version, 'should still parse correctly despite extra fields')
})

// ============================================
// 测试：旧版完整更新流程模拟
// ============================================

console.log('\n旧版完整更新流程模拟:')

// 模拟旧版 Windows 用户检查更新并获取下载信息
test('OldCompat_WindowsFullFlow_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)

  // 1. 版本比较：0.7.0 → 0.8.0 有更新
  assert.ok(isOlderVersionCompare('0.7.0', old.version), 'should detect update')

  // 2. 获取 Windows 平台下载信息
  const winAsset = old.assets.windows
  assert.ok(winAsset.url.includes('cc-box.oss-cn-beijing.aliyuncs.com'), 'url should be OSS')
  assert.ok(winAsset.url.includes('v0.8.0'), 'url should contain version')
  assert.ok(winAsset.url.endsWith('-setup.exe'), 'url should point to exe')
  assert.ok(winAsset.size > 0, 'size should be positive')

  // 3. 提取文件名（旧版 extract_filename 逻辑）
  const filename = winAsset.url.split('/').pop()
  assert.ok(filename.endsWith('.exe'), `extracted filename "${filename}" should end with .exe`)
})

// 模拟旧版 macOS 用户检查更新并获取 .dmg 下载信息
test('OldCompat_MacFullFlow_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)

  assert.ok(isOlderVersionCompare('0.7.0', old.version), 'should detect update')

  const macAsset = old.assets.macos
  assert.ok(macAsset.url.endsWith('.dmg'), `expected .dmg, got "${macAsset.url}"`)
  assert.ok(macAsset.size > 0, 'size should be positive')
})

// 模拟旧版 Linux 用户检查更新并获取 .AppImage 下载信息
test('OldCompat_LinuxFullFlow_001', () => {
  const raw = generateJson()
  const old = deserializeAsOldVersion(raw)

  assert.ok(isOlderVersionCompare('0.7.0', old.version), 'should detect update')

  const linuxAsset = old.assets.linux
  assert.ok(linuxAsset.url.endsWith('.AppImage'), `expected .AppImage, got "${linuxAsset.url}"`)
  assert.ok(linuxAsset.size > 0, 'size should be positive')
})

// ============================================
// 结果
// ============================================

console.log(`\n${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
