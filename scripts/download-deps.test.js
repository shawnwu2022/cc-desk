#!/usr/bin/env node

/**
 * download-deps.js 版本合并逻辑测试
 * 运行: node scripts/download-deps.test.js
 */

const {
  buildVersionEntryFromPlatformInfos,
  compareVersionsDesc,
  mergeVersionEntries,
} = require('./download-deps')
const assert = require('assert')

let passed = 0
let failed = 0

function test(name, fn) {
  try {
    fn()
    passed++
    console.log(`  \x1b[32m✓\x1b[0m ${name}`)
  } catch (err) {
    failed++
    console.log(`  \x1b[31m✗\x1b[0m ${name}`)
    console.log(`    ${err.message}`)
  }
}

// ============================================
// 测试数据
// ============================================

function makePlatformInfos(version) {
  return [
    { platform: 'win32-x64', filename: 'claude.exe', checksum: `sha-${version}-win`, size: 100 },
    { platform: 'darwin-arm64', filename: 'claude', checksum: `sha-${version}-mac`, size: 95 },
  ]
}

// ============================================
// buildVersionEntryFromPlatformInfos
// ============================================

console.log('\nbuildVersionEntryFromPlatformInfos:')

// 正常构建 entry
test('BuildEntry_Normal_001', () => {
  const entry = buildVersionEntryFromPlatformInfos('1.0.17', makePlatformInfos('1.0.17'))
  assert.strictEqual(entry.version, '1.0.17')
  assert.strictEqual(typeof entry.release_date, 'string')
  assert.match(entry.release_date, /^\d{4}-\d{2}-\d{2}$/)
  assert.deepStrictEqual(Object.keys(entry.platforms).sort(), ['darwin-arm64', 'win32-x64'])
  assert.strictEqual(entry.platforms['win32-x64'].url, 'deps/claude/1.0.17/win32-x64/claude.exe')
  assert.strictEqual(entry.platforms['win32-x64'].size, 100)
})

// 空 platformInfos
test('BuildEntry_Empty_001', () => {
  const entry = buildVersionEntryFromPlatformInfos('1.0.0', [])
  assert.strictEqual(entry.version, '1.0.0')
  assert.deepStrictEqual(entry.platforms, {})
})

// ============================================
// compareVersionsDesc
// ============================================

console.log('\ncompareVersionsDesc:')

// 1.0.10 > 1.0.2（数值比较，非字符串）
test('Compare_NumericSort_001', () => {
  const r = compareVersionsDesc({ version: '1.0.2' }, { version: '1.0.10' })
  assert.ok(r > 0, '1.0.10 应排在 1.0.2 前面（降序）')
})

// 1.0.17 > 1.0.16
test('Compare_Patch_001', () => {
  const r = compareVersionsDesc({ version: '1.0.16' }, { version: '1.0.17' })
  assert.ok(r > 0)
})

// 1.1.0 > 1.0.99
test('Compare_Minor_001', () => {
  const r = compareVersionsDesc({ version: '1.0.99' }, { version: '1.1.0' })
  assert.ok(r > 0)
})

// 2.0.0 > 1.9.9
test('Compare_Major_001', () => {
  const r = compareVersionsDesc({ version: '1.9.9' }, { version: '2.0.0' })
  assert.ok(r > 0)
})

// 相同版本返回 0
test('Compare_Same_001', () => {
  const r = compareVersionsDesc({ version: '1.0.17' }, { version: '1.0.17' })
  assert.strictEqual(r, 0)
})

// ============================================
// mergeVersionEntries
// ============================================

console.log('\nmergeVersionEntries:')

// 空现有 + 新 entry
test('Merge_IntoEmpty_001', () => {
  const newEntry = buildVersionEntryFromPlatformInfos('1.0.17', makePlatformInfos('1.0.17'))
  const merged = mergeVersionEntries([], newEntry)
  assert.strictEqual(merged.length, 1)
  assert.strictEqual(merged[0].version, '1.0.17')
})

// null 现有也能合并
test('Merge_IntoNull_001', () => {
  const newEntry = buildVersionEntryFromPlatformInfos('1.0.17', makePlatformInfos('1.0.17'))
  const merged = mergeVersionEntries(null, newEntry)
  assert.strictEqual(merged.length, 1)
})

// 旧列表 + 新版本：新版本应在最前
test('Merge_NewAtFront_001', () => {
  const existing = [
    buildVersionEntryFromPlatformInfos('1.0.10', makePlatformInfos('1.0.10')),
    buildVersionEntryFromPlatformInfos('1.0.2', makePlatformInfos('1.0.2')),
  ]
  const newEntry = buildVersionEntryFromPlatformInfos('1.0.17', makePlatformInfos('1.0.17'))
  const merged = mergeVersionEntries(existing, newEntry)
  assert.strictEqual(merged.length, 3)
  assert.deepStrictEqual(merged.map(e => e.version), ['1.0.17', '1.0.10', '1.0.2'])
})

// 同版本号去重：替换 platforms
test('Merge_Dedupe_001', () => {
  const existing = [
    buildVersionEntryFromPlatformInfos('1.0.17', [
      { platform: 'win32-x64', filename: 'claude.exe', checksum: 'old', size: 50 },
    ]),
  ]
  const newEntry = buildVersionEntryFromPlatformInfos('1.0.17', makePlatformInfos('1.0.17'))
  const merged = mergeVersionEntries(existing, newEntry)
  assert.strictEqual(merged.length, 1)
  assert.strictEqual(merged[0].platforms['win32-x64'].size, 100)
  assert.strictEqual(merged[0].platforms['win32-x64'].checksum, 'sha-1.0.17-win')
})

// 降级版本（旧版本运行，新版本比 latest 旧）
test('Merge_OlderVersion_001', () => {
  const existing = [
    buildVersionEntryFromPlatformInfos('1.0.20', makePlatformInfos('1.0.20')),
    buildVersionEntryFromPlatformInfos('1.0.18', makePlatformInfos('1.0.18')),
  ]
  const newEntry = buildVersionEntryFromPlatformInfos('1.0.19', makePlatformInfos('1.0.19'))
  const merged = mergeVersionEntries(existing, newEntry)
  assert.strictEqual(merged.length, 3)
  assert.deepStrictEqual(merged.map(e => e.version), ['1.0.20', '1.0.19', '1.0.18'])
})

// 跳过无效条目（无 version 字段）
test('Merge_SkipInvalid_001', () => {
  const existing = [
    { version: '1.0.2', release_date: '2026-05-01', platforms: {} },
    null,
    { release_date: '2026-05-02', platforms: {} }, // 无 version
  ]
  const newEntry = buildVersionEntryFromPlatformInfos('1.0.10', makePlatformInfos('1.0.10'))
  const merged = mergeVersionEntries(existing, newEntry)
  assert.strictEqual(merged.length, 2)
  assert.deepStrictEqual(merged.map(e => e.version), ['1.0.10', '1.0.2'])
})

// ============================================

console.log(`\n\x1b[36m==>\x1b[0m ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
