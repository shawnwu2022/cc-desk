#!/usr/bin/env node

/**
 * release.js 双格式 JSON 生成逻辑测试
 * 运行: node scripts/release.test.js
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

// ============================================
// 构造测试数据
// ============================================

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
  if (!filePath) return ''
  if (filePath.endsWith('.sig')) return 'dW50cnVzdGVkLXNpZ25hdHVyZQ=='
  return ''
}

const fakeGetFileSize = (filePath) => {
  if (!filePath) return 0
  if (filePath.includes('.sig')) return 128
  if (filePath.includes('.exe')) return 50000000
  if (filePath.includes('.app.tar.gz')) return 60000000
  if (filePath.includes('.dmg')) return 70000000
  if (filePath.includes('.AppImage')) return 55000000
  return 0
}

const BASE_URL = 'https://cc-box.oss-cn-beijing.aliyuncs.com/cc-desk'

// ============================================
// 官方格式（platforms）测试
// ============================================

console.log('\nTauri 官方格式 (platforms):')

// version 字段去除 v 前缀
test('LatestJson_VersionNoVPrefix_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: 'test',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.strictEqual(json.version, '0.8.0', `expected "0.8.0", got "${json.version}"`)
})

// platforms 包含三个平台
test('LatestJson_PlatformsAllPresent_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.ok(json.platforms['windows-x86_64'], 'missing windows-x86_64')
  assert.ok(json.platforms['darwin-x86_64'], 'missing darwin-x86_64')
  assert.ok(json.platforms['linux-x86_64'], 'missing linux-x86_64')
})

// platforms 中每个平台包含 signature 和 url
test('LatestJson_PlatformFields_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  for (const [platform, data] of Object.entries(json.platforms)) {
    assert.ok(data.signature, `${platform} missing signature`)
    assert.ok(data.url, `${platform} missing url`)
    assert.ok(data.url.startsWith(BASE_URL), `${platform} url should start with ${BASE_URL}`)
  }
})

// Windows platform 使用 .exe 文件
test('LatestJson_WindowsUrl_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.ok(json.platforms['windows-x86_64'].url.endsWith('-setup.exe'), `expected url ending with -setup.exe, got ${json.platforms['windows-x86_64'].url}`)
})

// macOS platform 使用 .app.tar.gz 而非 .dmg
test('LatestJson_MacTarGz_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.ok(json.platforms['darwin-x86_64'].url.endsWith('.app.tar.gz'), `expected url ending with .app.tar.gz, got ${json.platforms['darwin-x86_64'].url}`)
})

// notes 字段正确传递
test('LatestJson_NotesField_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '### Features\n- New feature',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.strictEqual(json.notes, '### Features\n- New feature')
})

// pub_date 是 ISO 格式字符串
test('LatestJson_PubDate_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.ok(json.pub_date, 'pub_date should exist')
  assert.ok(!isNaN(Date.parse(json.pub_date)), `pub_date "${json.pub_date}" is not a valid date`)
})

// ============================================
// 旧版本兼容格式（assets）测试
// ============================================

console.log('\n旧版本兼容格式 (assets):')

// assets 包含 windows/macos/linux 三个平台
test('LatestJson_AssetsAllPresent_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.ok(json.assets.windows, 'missing assets.windows')
  assert.ok(json.assets.macos, 'missing assets.macos')
  assert.ok(json.assets.linux, 'missing assets.linux')
})

// 旧版本 macOS 使用 .dmg 而非 .app.tar.gz
test('LatestJson_MacDmg_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.ok(json.assets.macos.url.endsWith('.dmg'), `expected url ending with .dmg, got ${json.assets.macos.url}`)
})

// release_notes_url 指向 GitHub Release
test('LatestJson_ReleaseNotesUrl_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.strictEqual(json.release_notes_url, 'https://github.com/shawnwu2022/cc-desk/releases/tag/v0.8.0')
})

// release_date 格式为 YYYY-MM-DD
test('LatestJson_ReleaseDate_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.ok(/^\d{4}-\d{2}-\d{2}$/.test(json.release_date), `expected YYYY-MM-DD, got "${json.release_date}"`)
})

// assets 中每个平台包含 url 和 size
test('LatestJson_AssetFields_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  for (const [platform, data] of Object.entries(json.assets)) {
    assert.ok(data.url, `${platform} missing url`)
    assert.ok(typeof data.size === 'number', `${platform} size should be number`)
  }
})

// ============================================
// 边界条件测试
// ============================================

console.log('\n边界条件:')

// 签名文件不存在时 signature 为空字符串
test('LatestJson_MissingSig_001', () => {
  const filesNoSig = makeFiles().filter(f => !f.name.endsWith('.sig'))
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: filesNoSig,
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.strictEqual(json.platforms['windows-x86_64'].signature, '', 'expected empty signature when .sig file missing')
})

// releaseNotes 为空时 notes 为空字符串
test('LatestJson_EmptyNotes_001', () => {
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: makeFiles(),
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.strictEqual(json.notes, '')
  assert.strictEqual(json.release_notes, '')
})

// macOS 没有 .dmg 文件时 url 以空文件名结尾
test('LatestJson_MissingDmg_001', () => {
  const filesNoDmg = makeFiles().filter(f => !f.name.endsWith('.dmg'))
  const json = generateLatestJson({
    version: 'v0.8.0',
    releaseNotes: '',
    files: filesNoDmg,
    bucketName: 'cc-box',
    endpoint: 'oss-cn-beijing.aliyuncs.com',
    readFileContent: fakeReadFileContent,
    getFileSize: fakeGetFileSize,
  })
  assert.strictEqual(json.assets.macos.size, 0, 'expected size 0 when no .dmg file')
})

// ============================================
// 结果
// ============================================

console.log(`\n${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
