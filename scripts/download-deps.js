#!/usr/bin/env node

/**
 * CC Desk 依赖下载脚本
 *
 * 从 Claude 官方和 GitHub 下载最新版本，上传到阿里云 OSS
 *
 * 用法：
 *   set HTTP_PROXY=http://127.0.0.1:33210
 *   npm run download-deps
 *
 * 下载内容：
 *   - Claude CLI (各平台版本)
 *   - Git for Windows (便携版)
 *
 * 功能：
 *   - 显示下载/上传进度
 *   - 跳过已下载且 checksum 验证通过的文件
 *   - 跳过 OSS 已存在的文件
 */

const fs = require('fs')
const path = require('path')
const crypto = require('crypto')
const { execSync, spawn } = require('child_process')

// ============================================
// 配置
// ============================================

const PROJECT_ROOT = path.resolve(__dirname, '..')
const OSS_CONFIG_PATH = path.join(PROJECT_ROOT, 'scripts/oss-config.json')
const RELEASES_DIR = path.join(PROJECT_ROOT, 'releases/deps')

const CLAUDE_DOWNLOAD_BASE = 'https://downloads.claude.ai/claude-code-releases'
const GIT_RELEASES_API = 'https://api.github.com/repos/git-for-windows/git/releases/latest'

// Claude 平台列表（主要支持 Windows）
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

function logStep(msg) {
  console.log(`\n\x1b[36m==> ${msg}\x1b[0m`)
}

function logSuccess(msg) {
  console.log(`\x1b[32m✓ ${msg}\x1b[0m`)
}

function logError(msg) {
  console.log(`\x1b[31m✗ ${msg}\x1b[0m`)
}

function logInfo(msg) {
  console.log(msg)
}

function logSkip(msg) {
  console.log(`\x1b[33m⊙ ${msg}\x1b[0m`)
}

// 格式化文件大小
function formatSize(bytes) {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)}KB`
  return `${Math.round(bytes / 1024 / 1024)}MB`
}

// 格式化进度条
function formatProgress(percent, downloaded, total) {
  const barWidth = 40
  const filled = Math.round(barWidth * percent / 100)
  const empty = barWidth - filled
  const bar = '\x1b[36m' + '█'.repeat(filled) + '\x1b[0m' + '░'.repeat(empty)
  return `${bar} ${percent.toFixed(1)}% (${formatSize(downloaded)}/${formatSize(total)})`
}

// 使用 curl 下载（支持进度显示）
async function curlDownload(url, outputPath, expectedChecksum = null) {
  const proxy = process.env.HTTP_PROXY || process.env.HTTPS_PROXY

  // 检查本地文件是否已存在且 checksum 匹配
  if (fs.existsSync(outputPath)) {
    if (expectedChecksum) {
      const localChecksum = await calculateChecksum(outputPath)
      if (localChecksum === expectedChecksum.toLowerCase()) {
        logSkip(`已存在且校验通过: ${path.basename(outputPath)} (${formatSize(fs.statSync(outputPath).size)})`)
        return { success: true, skipped: true }
      } else {
        logInfo(`本地文件 checksum 不匹配，重新下载...`)
        fs.unlinkSync(outputPath)
      }
    } else {
      logSkip(`已存在: ${path.basename(outputPath)} (${formatSize(fs.statSync(outputPath).size)})`)
      return { success: true, skipped: true }
    }
  }

  logInfo(`下载: ${url}`)
  if (proxy) logInfo(`使用代理: ${proxy}`)

  // 创建临时文件用于下载
  const tempPath = outputPath + '.tmp'
  // 清理可能存在的临时文件
  if (fs.existsSync(tempPath)) fs.unlinkSync(tempPath)

  try {
    // 构建 curl 参数
    const curlArgs = [
      '-fsSL',
      '--ssl-no-revoke',
      '-#', // 简短进度条（类似 wget）
      '-o', tempPath,
    ]
    if (proxy) {
      curlArgs.push('--proxy', proxy)
    }
    curlArgs.push(url)

    // 使用 spawn 显示 curl 自带的进度条
    const curl = spawn('curl', curlArgs, {
      stdio: ['inherit', 'inherit', 'inherit']
    })

    // 等待下载完成
    await new Promise((resolve, reject) => {
      curl.on('close', (code) => {
        if (code === 0) resolve()
        else reject(new Error(`curl exited with code ${code}`))
      })
      curl.on('error', reject)
    })

    // 移动临时文件到目标路径
    fs.renameSync(tempPath, outputPath)

    logSuccess(`下载完成: ${path.basename(outputPath)} (${formatSize(fs.statSync(outputPath).size)})`)
    return { success: true, skipped: false }
  } catch (e) {
    // 清理临时文件
    if (fs.existsSync(tempPath)) fs.unlinkSync(tempPath)
    logError(`下载失败: ${e.message}`)
    return { success: false, skipped: false }
  }
}

// 使用 curl 获取内容（自动支持 HTTP_PROXY 环境变量）
function curlFetch(url) {
  const proxy = process.env.HTTP_PROXY || process.env.HTTPS_PROXY
  const proxyArg = proxy ? `--proxy "${proxy}"` : ''

  if (proxy) logInfo(`使用代理: ${proxy}`)

  try {
    return execSync(`curl -fsSL ${proxyArg} --ssl-no-revoke "${url}"`, {
      encoding: 'utf-8'
    })
  } catch (e) {
    throw new Error(`请求失败: ${e.message}`)
  }
}

// 计算文件 SHA256
function calculateChecksum(filePath) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash('sha256')
    const stream = fs.createReadStream(filePath)
    stream.on('data', chunk => hash.update(chunk))
    stream.on('end', () => resolve(hash.digest('hex')))
    stream.on('error', reject)
  })
}

// 加载 OSS 配置
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

// 检查 OSS 文件是否存在且完整（大小匹配）
function checkOssFileValid(ossPath, expectedSize) {
  const config = loadOssConfig()
  if (!config) return false

  const { bucketName, region, accessKeyId, accessKeySecret } = config
  const endpoint = `${region}.aliyuncs.com`
  const ossUtilPath = path.join(PROJECT_ROOT, 'ossutil64.exe')

  if (!fs.existsSync(ossUtilPath)) return false

  try {
    const result = execSync(`"${ossUtilPath}" stat "oss://${bucketName}/${ossPath}" -e ${endpoint} -i ${accessKeyId} -k ${accessKeySecret}`, {
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe']
    })

    // 提取 Content-Length
    const sizeMatch = result.match(/Content-Length\s*:\s*(\d+)/)
    if (!sizeMatch) return false

    const ossSize = parseInt(sizeMatch[1])
    // 大小必须匹配（允许 0 字节的 latest.json）
    return ossSize === expectedSize || expectedSize === 0
  } catch {
    return false
  }
}

// OSS 上传（带进度显示）
async function uploadToOSS(localPath, ossPath, skipIfExists = true) {
  const config = loadOssConfig()
  if (!config) return { success: false, skipped: false }

  const fileSize = fs.statSync(localPath).size

  // 检查 OSS 文件是否已存在且完整
  if (skipIfExists && checkOssFileValid(ossPath, fileSize)) {
    logSkip(`OSS 已存在且完整: ${ossPath} (${formatSize(fileSize)})`)
    return { success: true, skipped: true }
  }

  const { bucketName, region, accessKeyId, accessKeySecret } = config
  const endpoint = `${region}.aliyuncs.com`
  const ossUtilPath = path.join(PROJECT_ROOT, 'ossutil64.exe')

  // 检查 ossutil
  if (!fs.existsSync(ossUtilPath)) {
    logInfo('下载 ossutil...')
    await downloadOssUtil(ossUtilPath)
  }

  logInfo(`上传: ${path.basename(localPath)} (${formatSize(fileSize)}) → oss://${bucketName}/${ossPath}`)

  try {
    // 使用 spawn 显示 ossutil 自带的进度输出
    const ossutil = spawn(ossUtilPath, [
      'cp',
      localPath,
      `oss://${bucketName}/${ossPath}`,
      '-f',
      '-e', endpoint,
      '-i', accessKeyId,
      '-k', accessKeySecret
    ], {
      stdio: ['inherit', 'inherit', 'inherit']
    })

    // 等待上传完成
    await new Promise((resolve, reject) => {
      ossutil.on('close', (code) => {
        if (code === 0) resolve()
        else reject(new Error(`ossutil exited with code ${code}`))
      })
      ossutil.on('error', reject)
    })

    logSuccess(`已上传: ${ossPath}`)
    return { success: true, skipped: false }
  } catch (e) {
    logError(`上传失败: ${e.message}`)
    return { success: false, skipped: false }
  }
}

// 下载 ossutil
async function downloadOssUtil(targetPath) {
  const url = 'https://gosspublic.alicdn.com/ossutil/1.7.14/ossutil64.exe'
  const result = await curlDownload(url, targetPath)
  if (!result.success) {
    throw new Error('下载 ossutil 失败')
  }
}

// ============================================
// Claude CLI 下载
// ============================================

async function downloadClaude() {
  logStep('获取 Claude CLI 最新版本...')

  // 获取最新版本号（使用 curl）
  const versionText = curlFetch(`${CLAUDE_DOWNLOAD_BASE}/latest`)
  const version = versionText.trim()
  logSuccess(`最新版本: ${version}`)

  // 获取 manifest
  const manifestText = curlFetch(`${CLAUDE_DOWNLOAD_BASE}/${version}/manifest.json`)
  let manifest
  try {
    manifest = JSON.parse(manifestText)
  } catch (e) {
    logError('解析 manifest.json 失败')
    throw e
  }

  // 创建版本目录
  const versionDir = path.join(RELEASES_DIR, 'claude', version)
  fs.mkdirSync(versionDir, { recursive: true })

  // 下载各平台版本
  const platformInfos = []
  const downloadResults = { downloaded: 0, skipped: 0, failed: 0 }

  for (const platform of CLAUDE_PLATFORMS) {
    const platformData = manifest.platforms?.[platform]
    if (!platformData) {
      logInfo(`跳过 ${platform}（manifest 中不存在）`)
      continue
    }

    const expectedChecksum = platformData.checksum
    if (!expectedChecksum) {
      logInfo(`跳过 ${platform}（无 checksum）`)
      continue
    }

    // Windows 使用 .exe 扩展名
    const ext = platform.startsWith('win32') ? '.exe' : ''
    const filename = `claude${ext}`
    const platformDir = path.join(versionDir, platform)
    fs.mkdirSync(platformDir, { recursive: true })
    const outputPath = path.join(platformDir, filename)

    logInfo(`\n--- ${platform}/${filename} ---`)
    const downloadUrl = `${CLAUDE_DOWNLOAD_BASE}/${version}/${platform}/${filename}`

    const result = await curlDownload(downloadUrl, outputPath, expectedChecksum)

    if (result.success) {
      if (result.skipped) {
        downloadResults.skipped++
      } else {
        downloadResults.downloaded++
      }

      // 验证 checksum（已下载的文件）
      const actualChecksum = await calculateChecksum(outputPath)
      if (actualChecksum !== expectedChecksum.toLowerCase()) {
        logError(`${platform} checksum 不匹配`)
        fs.unlinkSync(outputPath)
        downloadResults.failed++
        continue
      }

      logSuccess(`${platform} checksum 验证通过`)
      platformInfos.push({
        platform,
        filename,
        checksum: actualChecksum,
        size: fs.statSync(outputPath).size,
      })
    } else {
      downloadResults.failed++
    }
  }

  // 显示下载统计
  logInfo(`\n下载统计: 新下载 ${downloadResults.downloaded}, 跳过 ${downloadResults.skipped}, 失败 ${downloadResults.failed}`)

  if (platformInfos.length === 0) {
    throw new Error('没有成功下载任何平台版本')
  }

  // 生成 latest.json
  const latestJson = {
    version,
    release_date: new Date().toISOString().split('T')[0],
    platforms: {},
  }

  for (const info of platformInfos) {
    latestJson.platforms[info.platform] = {
      url: `deps/claude/${version}/${info.platform}/${info.filename}`,
      checksum: info.checksum,
      size: info.size,
    }
  }

  const latestJsonPath = path.join(RELEASES_DIR, 'claude', 'latest.json')
  fs.writeFileSync(latestJsonPath, JSON.stringify(latestJson, null, 2) + '\n')
  logSuccess(`latest.json 已生成: ${latestJsonPath}`)

  return { version, versionDir, platformInfos, downloadResults }
}

// ============================================
// Git 便携版下载
// ============================================

async function downloadGitPortable() {
  logStep('获取 Git for Windows 最新版本...')

  // GitHub API 获取最新 release（使用 curl）
  const releaseText = curlFetch(GIT_RELEASES_API)
  let release
  try {
    release = JSON.parse(releaseText)
  } catch (e) {
    logError('解析 GitHub release 失败')
    throw e
  }

  const version = release.tag_name
  logSuccess(`最新版本: ${version}`)

  // 找到 PortableGit 文件
  const portableAsset = release.assets.find(a =>
    a.name.match(/PortableGit-[\d.]+-64-bit\.7z\.exe/i)
  )

  if (!portableAsset) {
    logError('未找到 PortableGit-64-bit.7z.exe')
    throw new Error('PortableGit asset not found')
  }

  logInfo(`找到: ${portableAsset.name}`)
  logInfo(`下载 URL: ${portableAsset.browser_download_url}`)

  // 创建 Git 目录
  const gitDir = path.join(RELEASES_DIR, 'git')
  fs.mkdirSync(gitDir, { recursive: true })
  const outputPath = path.join(gitDir, portableAsset.name)

  logInfo(`\n--- ${portableAsset.name} ---`)
  const result = await curlDownload(portableAsset.browser_download_url, outputPath)

  if (!result.success) {
    throw new Error('下载 Git 便携版失败')
  }

  const fileSize = fs.statSync(outputPath).size
  const downloadResult = result.skipped ? '跳过' : '新下载'
  logSuccess(`下载完成 (${downloadResult}): ${formatSize(fileSize)}`)

  // 生成 latest.json
  const latestJson = {
    version,
    release_date: new Date().toISOString().split('T')[0],
    file: portableAsset.name,
    url: `deps/git/${portableAsset.name}`,
    size: fileSize,
  }

  const latestJsonPath = path.join(gitDir, 'latest.json')
  fs.writeFileSync(latestJsonPath, JSON.stringify(latestJson, null, 2) + '\n')
  logSuccess(`latest.json 已生成: ${latestJsonPath}`)

  return { version, outputPath, portableAsset, skipped: result.skipped }
}

// ============================================
// 上传到 OSS
// ============================================

// 构建 versions.json 中的单条 entry（纯函数）
function buildVersionEntryFromPlatformInfos(version, platformInfos) {
  const platforms = {}
  for (const info of platformInfos) {
    platforms[info.platform] = {
      url: `deps/claude/${version}/${info.platform}/${info.filename}`,
      checksum: info.checksum,
      size: info.size,
    }
  }
  return {
    version,
    release_date: new Date().toISOString().split('T')[0],
    platforms,
  }
}

// 比较两个版本号，降序（纯函数）
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

// 合并现有版本列表与新 entry：去重 + 降序排序（纯函数）
function mergeVersionEntries(existing, newEntry) {
  const map = new Map()
  for (const e of existing || []) {
    if (e && e.version) map.set(e.version, e)
  }
  if (newEntry && newEntry.version) {
    map.set(newEntry.version, newEntry)
  }
  return Array.from(map.values()).sort(compareVersionsDesc)
}

// 拉取 OSS 上的 versions.json（不存在时返回 null）
function fetchExistingVersionsJson() {
  const config = loadOssConfig()
  if (!config) return null
  const { bucketName, region } = config
  const endpoint = `${region}.aliyuncs.com`
  const url = `https://${bucketName}.${endpoint}/deps/claude/versions.json`
  try {
    const text = curlFetch(url)
    return JSON.parse(text)
  } catch {
    return null
  }
}

// 维护 deps/claude/versions.json：拉取现有 → 合并当前版本 → 上传
async function mergeAndUploadVersionsJson(version, platformInfos) {
  logInfo('\n--- 维护 versions.json ---')
  try {
    const existing = fetchExistingVersionsJson() || { latest: '', updated_at: '', versions: [] }
    const newEntry = buildVersionEntryFromPlatformInfos(version, platformInfos)
    const mergedVersions = mergeVersionEntries(existing.versions, newEntry)
    const merged = {
      latest: version,
      updated_at: new Date().toISOString(),
      versions: mergedVersions,
    }

    const versionsJsonPath = path.join(RELEASES_DIR, 'claude', 'versions.json')
    fs.mkdirSync(path.dirname(versionsJsonPath), { recursive: true })
    fs.writeFileSync(versionsJsonPath, JSON.stringify(merged, null, 2) + '\n')

    const result = await uploadToOSS(versionsJsonPath, 'deps/claude/versions.json', false)
    if (result.success) {
      logSuccess(`versions.json 已更新（共 ${mergedVersions.length} 个版本，最新 v${version}）`)
    } else {
      throw new Error('上传失败')
    }
  } catch (e) {
    logError(`versions.json 维护失败（不影响主流程）: ${e.message}`)
  }
}

async function uploadClaudeToOSS(version, versionDir, platformInfos) {
  logStep('上传 Claude CLI 到 OSS...')

  const uploadResults = { uploaded: 0, skipped: 0, failed: 0 }

  // 上传各平台文件
  const platforms = fs.readdirSync(versionDir)
  for (const platform of platforms) {
    const platformDir = path.join(versionDir, platform)
    if (!fs.statSync(platformDir).isDirectory()) continue

    const files = fs.readdirSync(platformDir)
    for (const file of files) {
      const localPath = path.join(platformDir, file)
      const ossPath = `deps/claude/${version}/${platform}/${file}`

      logInfo(`\n--- 上传 ${platform}/${file} ---`)
      const result = await uploadToOSS(localPath, ossPath)

      if (result.success) {
        if (result.skipped) uploadResults.skipped++
        else uploadResults.uploaded++
      } else {
        uploadResults.failed++
      }
    }
  }

  // 上传 latest.json（强制更新）
  logInfo(`\n--- 上传 latest.json ---`)
  const latestJsonPath = path.join(RELEASES_DIR, 'claude', 'latest.json')
  const result = await uploadToOSS(latestJsonPath, 'deps/claude/latest.json', false)
  if (result.success) {
    uploadResults.uploaded++
  } else {
    uploadResults.failed++
  }

  // 维护 versions.json（失败不影响主流程）
  if (platformInfos && platformInfos.length > 0) {
    await mergeAndUploadVersionsJson(version, platformInfos)
  }

  logInfo(`\n上传统计: 新上传 ${uploadResults.uploaded}, 跳过 ${uploadResults.skipped}, 失败 ${uploadResults.failed}`)
}

module.exports = {
  buildVersionEntryFromPlatformInfos,
  compareVersionsDesc,
  mergeVersionEntries,
}

async function uploadGitToOSS(outputPath, portableAsset, skipped) {
  logStep('上传 Git 便携版到 OSS...')

  // uploadToOSS 会自动检查 OSS 文件完整性
  const uploadResults = { uploaded: 0, skipped: 0, failed: 0 }

  logInfo(`\n--- 上传 ${portableAsset.name} ---`)
  const result = await uploadToOSS(outputPath, `deps/git/${portableAsset.name}`)

  if (result.success) {
    if (result.skipped) uploadResults.skipped++
    else uploadResults.uploaded++
  } else {
    uploadResults.failed++
  }

  // 上传 latest.json（强制更新）
  logInfo(`\n--- 上传 latest.json ---`)
  const latestJsonPath = path.join(RELEASES_DIR, 'git', 'latest.json')
  const jsonResult = await uploadToOSS(latestJsonPath, 'deps/git/latest.json', false)
  if (jsonResult.success) {
    uploadResults.uploaded++
  } else {
    uploadResults.failed++
  }

  logInfo(`\n上传统计: 新上传 ${uploadResults.uploaded}, 跳过 ${uploadResults.skipped}, 失败 ${uploadResults.failed}`)
}

// ============================================
// 主流程
// ============================================

async function main() {
  console.log('\x1b[35m======================================')
  console.log('     CC Desk 依赖下载脚本')
  console.log('======================================\x1b[0m')

  // 检查代理
  if (!process.env.HTTP_PROXY && !process.env.HTTPS_PROXY) {
    logInfo('\n提示: 未配置代理，国内可能无法访问')
    logInfo('建议设置: set HTTP_PROXY=http://127.0.0.1:33210')
  }

  try {
    // 下载 Claude
    const claudeResult = await downloadClaude()
    await uploadClaudeToOSS(claudeResult.version, claudeResult.versionDir, claudeResult.platformInfos)

    // 下载 Git（仅 Windows）
    const gitResult = await downloadGitPortable()
    await uploadGitToOSS(gitResult.outputPath, gitResult.portableAsset, gitResult.skipped)

    console.log('\n\x1b[32m======================================')
    console.log('     全部完成！')
    console.log('======================================\x1b[0m')

    logInfo('\nOSS 文件结构:')
    logInfo('  deps/claude/latest.json')
    logInfo('  deps/claude/versions.json')
    logInfo(`  deps/claude/${claudeResult.version}/win32-x64/claude.exe`)
    logInfo('  deps/git/latest.json')
    logInfo(`  deps/git/${gitResult.portableAsset.name}`)

  } catch (e) {
    logError(`\n执行失败: ${e.message}`)
    process.exit(1)
  }
}

// 仅在直接执行时运行 main（允许测试时 require 纯函数）
if (require.main === module) {
  main()
}