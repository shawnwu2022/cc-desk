#!/usr/bin/env node

/**
 * 重建 deps/claude/versions.json
 *
 * 扫描本地 releases/deps/claude/ 下所有版本目录，对每个版本计算所有平台的
 * sha256 checksum 与 size，构建完整的 versions.json 并上传到 OSS。
 *
 * 用法：
 *   node scripts/rebuild-claude-versions.js              # 默认扫描全部本地版本
 *   node scripts/rebuild-claude-versions.js --only-oss   # 仅保留 OSS 上已存在的版本
 *
 * 配合 --only-oss 时，会用 ossutil ls 拉取 OSS 上现有的版本目录，
 * 排除本地有但 OSS 上没有的版本（避免出现"列表里有但下载 404"）。
 */

const fs = require('fs')
const path = require('path')
const crypto = require('crypto')
const { execSync, spawn } = require('child_process')

const PROJECT_ROOT = path.resolve(__dirname, '..')
const OSS_CONFIG_PATH = path.join(PROJECT_ROOT, 'scripts/oss-config.json')
const RELEASES_DIR = path.join(PROJECT_ROOT, 'releases/deps/claude')
const OSS_VERSIONS_PATH = 'deps/claude/versions.json'

const CLAUDE_PLATFORMS = [
  'win32-x64',
  'win32-arm64',
  'darwin-x64',
  'darwin-arm64',
  'linux-x64',
  'linux-x64-musl',
  'linux-arm64',
  'linux-arm64-musl',
]

// ============================================
// 工具函数
// ============================================

function logStep(msg) { console.log(`\n\x1b[36m==> ${msg}\x1b[0m`) }
function logSuccess(msg) { console.log(`\x1b[32m✓ ${msg}\x1b[0m`) }
function logError(msg) { console.log(`\x1b[31m✗ ${msg}\x1b[0m`) }
function logInfo(msg) { console.log(msg) }

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)}KB`
  return `${Math.round(bytes / 1024 / 1024)}MB`
}

function calculateChecksum(filePath) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash('sha256')
    const stream = fs.createReadStream(filePath)
    stream.on('data', chunk => hash.update(chunk))
    stream.on('end', () => resolve(hash.digest('hex')))
    stream.on('error', reject)
  })
}

function compareVersionsDesc(a, b) {
  const pa = String(a.version).split('.').map(n => parseInt(n, 10) || 0)
  const pb = String(b.version).split('.').map(n => parseInt(n, 10) || 0)
  const len = Math.max(pa.length, pb.length)
  for (let i = 0; i < len; i++) {
    const va = pa[i] || 0
    const vb = pb[i] || 0
    if (va !== vb) return vb - va
  }
  return 0
}

function loadOssConfig() {
  if (!fs.existsSync(OSS_CONFIG_PATH)) {
    logError(`OSS 配置文件不存在: ${OSS_CONFIG_PATH}`)
    return null
  }
  const config = JSON.parse(fs.readFileSync(OSS_CONFIG_PATH, 'utf-8'))
  if (!config.bucketName || !config.region || !config.accessKeyId || !config.accessKeySecret) {
    logError('OSS 配置缺失，请检查 scripts/oss-config.json')
    return null
  }
  return config
}

// 通过 ossutil 列出 OSS 上 deps/claude/ 下的版本目录
function listOssClaudeVersions() {
  const config = loadOssConfig()
  if (!config) return null
  const { bucketName, region, accessKeyId, accessKeySecret } = config
  const endpoint = `${region}.aliyuncs.com`
  const ossUtilPath = path.join(PROJECT_ROOT, 'ossutil64.exe')
  if (!fs.existsSync(ossUtilPath)) {
    logError('ossutil64.exe 不存在')
    return null
  }
  try {
    const result = execSync(
      `"${ossUtilPath}" ls "oss://${bucketName}/deps/claude/" -e ${endpoint} -i ${accessKeyId} -k ${accessKeySecret}`,
      { encoding: 'utf-8', stdio: ['pipe', 'pipe', 'pipe'] }
    )
    // 输出每行包含 "oss://bucket/deps/claude/<version>/<platform>/<file>"
    const versions = new Set()
    for (const line of result.split(/\r?\n/)) {
      const m = line.match(/deps\/claude\/([^/\s]+)\//)
      if (m && m[1] !== 'latest.json' && !m[1].endsWith('.json')) {
        versions.add(m[1])
      }
    }
    return Array.from(versions)
  } catch (e) {
    logError(`ossutil ls 失败: ${e.message}`)
    return null
  }
}

// 上传到 OSS（强制覆盖）
async function uploadToOSS(localPath, ossPath) {
  const config = loadOssConfig()
  if (!config) return false
  const { bucketName, region, accessKeyId, accessKeySecret } = config
  const endpoint = `${region}.aliyuncs.com`
  const ossUtilPath = path.join(PROJECT_ROOT, 'ossutil64.exe')
  if (!fs.existsSync(ossUtilPath)) {
    logError('ossutil64.exe 不存在')
    return false
  }
  const fileSize = fs.statSync(localPath).size
  logInfo(`上传: ${path.basename(localPath)} (${formatSize(fileSize)}) → oss://${bucketName}/${ossPath}`)
  try {
    const ossutil = spawn(ossUtilPath, [
      'cp', localPath, `oss://${bucketName}/${ossPath}`,
      '-f',
      '-e', endpoint,
      '-i', accessKeyId,
      '-k', accessKeySecret,
    ], { stdio: ['inherit', 'inherit', 'inherit'] })
    await new Promise((resolve, reject) => {
      ossutil.on('close', code => code === 0 ? resolve() : reject(new Error(`ossutil exited with code ${code}`)))
      ossutil.on('error', reject)
    })
    logSuccess(`已上传: ${ossPath}`)
    return true
  } catch (e) {
    logError(`上传失败: ${e.message}`)
    return false
  }
}

// ============================================
// 核心逻辑
// ============================================

// 扫描一个版本目录，返回 entry；返回 null 表示该版本不完整
async function buildEntryForVersion(version) {
  const versionDir = path.join(RELEASES_DIR, version)
  if (!fs.existsSync(versionDir) || !fs.statSync(versionDir).isDirectory()) {
    logError(`版本目录不存在: ${versionDir}`)
    return null
  }

  // 推断 release_date：取该版本下所有文件最早修改日期
  let earliest = null
  function walk(dir) {
    for (const name of fs.readdirSync(dir)) {
      const p = path.join(dir, name)
      const stat = fs.statSync(p)
      if (stat.isDirectory()) {
        walk(p)
      } else {
        const mtime = stat.mtime
        if (!earliest || mtime < earliest) earliest = mtime
      }
    }
  }
  walk(versionDir)
  const releaseDate = (earliest || new Date()).toISOString().split('T')[0]

  const platforms = {}
  for (const platform of CLAUDE_PLATFORMS) {
    const ext = platform.startsWith('win32') ? '.exe' : ''
    const filename = `claude${ext}`
    const filePath = path.join(versionDir, platform, filename)
    if (!fs.existsSync(filePath)) {
      logInfo(`  跳过 ${version}/${platform}（文件不存在）`)
      continue
    }
    const size = fs.statSync(filePath).size
    const checksum = await calculateChecksum(filePath)
    platforms[platform] = {
      url: `deps/claude/${version}/${platform}/${filename}`,
      checksum,
      size,
    }
    logInfo(`  ✓ ${version}/${platform}/${filename} (${formatSize(size)})`)
  }

  if (Object.keys(platforms).length === 0) {
    logError(`版本 ${version} 没有任何可用平台文件，跳过`)
    return null
  }

  return {
    version,
    release_date: releaseDate,
    platforms,
  }
}

async function rebuild(options = {}) {
  console.log('\x1b[35m======================================')
  console.log('  重建 deps/claude/versions.json')
  console.log('======================================\x1b[0m')

  // 收集要处理的版本列表
  let versions = fs.readdirSync(RELEASES_DIR)
    .filter(name => {
      const full = path.join(RELEASES_DIR, name)
      return fs.statSync(full).isDirectory() && /^\d+\.\d+\.\d+$/.test(name)
    })

  if (options.onlyOss) {
    logStep('拉取 OSS 上的版本目录列表...')
    const ossVersions = listOssClaudeVersions()
    if (!ossVersions) {
      logError('无法获取 OSS 版本列表，终止（避免误删 OSS 实际有的版本）')
      process.exit(1)
    }
    logInfo(`OSS 上现有版本: ${ossVersions.join(', ')}`)
    const ossSet = new Set(ossVersions)
    const before = versions.length
    versions = versions.filter(v => ossSet.has(v))
    logInfo(`本地版本过滤：${before} → ${versions.length}（仅保留 OSS 上已存在的）`)
  }

  // 排序：降序（最新在前）
  versions.sort((a, b) => compareVersionsDesc({ version: a }, { version: b }))

  logStep(`扫描 ${versions.length} 个版本...`)
  const entries = []
  for (const v of versions) {
    logInfo(`\n--- ${v} ---`)
    const entry = await buildEntryForVersion(v)
    if (entry) entries.push(entry)
  }

  if (entries.length === 0) {
    logError('没有任何有效版本，终止')
    process.exit(1)
  }

  const latest = entries[0].version
  const merged = {
    latest,
    updated_at: new Date().toISOString(),
    versions: entries,
  }

  const versionsJsonPath = path.join(RELEASES_DIR, 'versions.json')
  fs.writeFileSync(versionsJsonPath, JSON.stringify(merged, null, 2) + '\n')
  logSuccess(`versions.json 已生成: ${versionsJsonPath}`)
  logInfo(`共 ${entries.length} 个版本，最新 v${latest}`)

  // 上传
  logStep('上传 versions.json 到 OSS...')
  const ok = await uploadToOSS(versionsJsonPath, OSS_VERSIONS_PATH)
  if (!ok) {
    process.exit(1)
  }

  console.log('\n\x1b[32m======================================')
  console.log('  重建完成！')
  console.log('======================================\x1b[0m')
  logInfo(`OSS URL: https://cc-box.oss-cn-beijing.aliyuncs.com/${OSS_VERSIONS_PATH}`)
}

// ============================================
// 入口
// ============================================

const args = process.argv.slice(2)
const onlyOss = args.includes('--only-oss')

rebuild({ onlyOss }).catch(e => {
  logError(`执行失败: ${e.message}`)
  console.error(e.stack)
  process.exit(1)
})
