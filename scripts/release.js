#!/usr/bin/env node

/**
 * CC-Box 自动化发布脚本
 *
 * 用法：
 *   node scripts/release.js --bump patch --notes "### Fixed\n- Fix issue"
 *   node scripts/release.js --bump minor --notes "### Features\n- Add feature" --skip-ci
 *
 * 环境变量：
 *   HTTP_PROXY / HTTPS_PROXY — 推送 GitHub 时使用的代理
 */

const fs = require('fs')
const path = require('path')
const { execSync } = require('child_process')

// ============================================
// 配置
// ============================================

const PROJECT_ROOT = path.resolve(__dirname, '..')
const VERSION_FILES = {
  cargoToml: path.join(PROJECT_ROOT, 'src-tauri/Cargo.toml'),
  packageJson: path.join(PROJECT_ROOT, 'package.json'),
  tauriConf: path.join(PROJECT_ROOT, 'src-tauri/tauri.conf.json'),
  changelog: path.join(PROJECT_ROOT, 'CHANGELOG.md'),
}
const OSS_CONFIG_PATH = path.join(PROJECT_ROOT, 'scripts/oss-config.json')
const REMOTE_NAME = 'origin'
const MAIN_BRANCH = 'main'

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

function exec(cmd, options = {}) {
  const env = { ...process.env }
  // git push / gh 命令需要代理访问 GitHub
  const needsProxy = /^(git push|git tag.*push|gh )/.test(cmd)
  if (needsProxy && process.env.HTTP_PROXY) {
    env.HTTP_PROXY = process.env.HTTP_PROXY
    env.HTTPS_PROXY = process.env.HTTPS_PROXY || process.env.HTTP_PROXY
  } else {
    delete env.HTTP_PROXY
    delete env.HTTPS_PROXY
    delete env.http_proxy
    delete env.https_proxy
  }
  try {
    return execSync(cmd, {
      encoding: 'utf-8',
      stdio: options.silent ? 'pipe' : 'inherit',
      env,
      ...options,
    })
  } catch (e) {
    if (options.allowFail) return null
    logError(`命令执行失败: ${cmd}`)
    if (!options.silent) process.exit(1)
  }
}

// 代理自动重试：先代理，失败后非代理，都失败则中止
function execWithProxyRetry(cmd, options = {}) {
  const PROXY = 'http://127.0.0.1:33210'

  // 第一次尝试：使用代理
  logInfo('尝试使用代理...')
  const envWithProxy = { ...process.env, HTTP_PROXY: PROXY, HTTPS_PROXY: PROXY }
  try {
    return execSync(cmd, {
      encoding: 'utf-8',
      stdio: options.silent ? 'pipe' : 'inherit',
      env: envWithProxy,
      ...options,
    })
  } catch (e1) {
    logInfo('代理失败，尝试直连...')
    // 第二次尝试：不使用代理
    const envNoProxy = { ...process.env }
    delete envNoProxy.HTTP_PROXY
    delete envNoProxy.HTTPS_PROXY
    delete envNoProxy.http_proxy
    delete envNoProxy.https_proxy
    try {
      return execSync(cmd, {
        encoding: 'utf-8',
        stdio: options.silent ? 'pipe' : 'inherit',
        env: envNoProxy,
        ...options,
      })
    } catch (e2) {
      logError(`命令执行失败（代理和直连都失败）: ${cmd}`)
      if (!options.allowFail) process.exit(1)
      return null
    }
  }
}

// 清理所有代理配置
function clearAllProxyConfig() {
  const configs = [
    ['--global', 'http.proxy'],
    ['--global', 'https.proxy'],
    ['--system', 'http.proxy'],
    ['--system', 'https.proxy'],
    ['--local', 'http.proxy'],
    ['--local', 'https.proxy'],
  ]
  for (const [level, key] of configs) {
    try {
      execSync(`git config ${level} --unset ${key}`, { stdio: 'pipe' })
    } catch {}
  }
}

function execQuiet(cmd) {
  return exec(cmd, { silent: true, allowFail: true })
}

function askConfirm(question) {
  const readline = require('readline')
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout })
  return new Promise(resolve => {
    rl.question(question, answer => {
      rl.close()
      resolve(answer.toLowerCase() === 'y')
    })
  })
}

function getCurrentVersion() {
  const pkg = JSON.parse(fs.readFileSync(VERSION_FILES.packageJson, 'utf-8'))
  return pkg.version
}

function bumpVersion(version, bumpType) {
  const parts = version.split('.').map(Number)
  if (bumpType === 'major') { parts[0]++; parts[1] = 0; parts[2] = 0 }
  else if (bumpType === 'minor') { parts[1]++; parts[2] = 0 }
  else { parts[2]++ }
  return parts.join('.')
}

// ============================================
// 版本号更新
// ============================================

function updateVersionFiles(newVersion) {
  logStep('更新版本号文件...')

  // Cargo.toml
  let cargo = fs.readFileSync(VERSION_FILES.cargoToml, 'utf-8')
  cargo = cargo.replace(/^version\s*=\s*"[^"]*"/m, `version = "${newVersion}"`)
  fs.writeFileSync(VERSION_FILES.cargoToml, cargo)
  logSuccess(`Cargo.toml -> v${newVersion}`)

  // package.json
  const pkg = JSON.parse(fs.readFileSync(VERSION_FILES.packageJson, 'utf-8'))
  pkg.version = newVersion
  fs.writeFileSync(VERSION_FILES.packageJson, JSON.stringify(pkg, null, 2) + '\n')
  logSuccess(`package.json -> v${newVersion}`)

  // tauri.conf.json
  const conf = JSON.parse(fs.readFileSync(VERSION_FILES.tauriConf, 'utf-8'))
  conf.version = newVersion
  fs.writeFileSync(VERSION_FILES.tauriConf, JSON.stringify(conf, null, 2) + '\n')
  logSuccess(`tauri.conf.json -> v${newVersion}`)
}

// ============================================
// CHANGELOG 更新
// ============================================

function updateChangelog(newVersion, releaseNotes) {
  logStep('更新 CHANGELOG.md...')

  const today = new Date().toISOString().split('T')[0]
  const entry = `\n## [${newVersion}] - ${today}\n\n${releaseNotes}\n`

  const content = fs.readFileSync(VERSION_FILES.changelog, 'utf-8')
  const match = content.match(/(.*?# Changelog.*?\n)/s)
  if (!match) {
    logError('无法解析 CHANGELOG.md 格式')
    process.exit(1)
  }

  const newContent = match[1] + entry + content.slice(match[1].length)
  fs.writeFileSync(VERSION_FILES.changelog, newContent)
  logSuccess('CHANGELOG.md 已更新')
}

// ============================================
// Git 操作
// ============================================

function gitCommit(version) {
  logStep('Git 提交...')
  const status = execQuiet('git status --porcelain')
  if (!status) {
    logError('没有待提交的更改')
    process.exit(1)
  }
  exec('git add -A')
  exec(`git commit -m "Release v${version}"`)
  logSuccess(`提交完成: Release v${version}`)
}

function gitPush(version) {
  logStep('推送到远程仓库...')
  clearAllProxyConfig() // 先清理残留代理配置
  execWithProxyRetry(`git push ${REMOTE_NAME} ${MAIN_BRANCH}`)
  logSuccess('main 分支已推送')

  execWithProxyRetry(`git tag -a v${version} -m "Release v${version}"`)
  execWithProxyRetry(`git push ${REMOTE_NAME} v${version}`)
  logSuccess(`标签 v${version} 已推送`)
}

// ============================================
// CI 监控
// ============================================

async function watchCIBuild() {
  logStep('监控 CI 构建...')
  logInfo('等待 CI workflow 启动...')

  // 等待 10 秒让 workflow 启动
  await new Promise(r => setTimeout(r, 10000))

  const runsJson = execWithProxyRetry('gh run list --limit 5 --json databaseId,displayTitle,conclusion,status', { silent: true })
  if (!runsJson) {
    logError('无法获取 GitHub Actions 状态，请手动检查：https://github.com/orczh-hj/cc-box/actions')
    return
  }

  const runs = JSON.parse(runsJson)
  if (!runs.length) {
    logError('未找到 workflow run')
    return
  }

  const run = runs[0]
  logInfo(`找到 workflow run: ${run.displayTitle} (ID: ${run.databaseId})`)

  try {
    execWithProxyRetry(`gh run watch ${run.databaseId} --exit-status`)
    logSuccess('CI 构建成功')
  } catch {
    logError('CI 构建失败，请检查：https://github.com/orczh-hj/cc-box/actions')
    // 全自动模式继续执行，不中断流程
    logInfo('继续执行发布流程...')
  }
}

// ============================================
// 发布 Release
// ============================================

function publishRelease(version, releaseNotes) {
  logStep('发布 GitHub Release...')

  const fullNotes = `## What's Changed\n\n${releaseNotes}`
  // 用临时文件传递多行 notes，避免 shell 转义问题
  const notesFile = path.join(require('os').tmpdir(), `cc-box-release-notes-${version}.txt`)
  fs.writeFileSync(notesFile, fullNotes)

  try {
    execWithProxyRetry(`gh release edit v${version} --draft=false --notes-file "${notesFile}"`)
    logSuccess(`GitHub Release v${version} 已发布！`)
    logInfo(`查看: https://github.com/orczh-hj/cc-box/releases/tag/v${version}`)
  } catch {
    logError('GitHub Release 发布失败，请手动发布：https://github.com/orczh-hj/cc-box/releases')
    process.exit(1)
  } finally {
    try { fs.unlinkSync(notesFile) } catch {}
  }
}

// ============================================
// Gitee Release
// ============================================

function publishGiteeRelease(version, releaseNotes) {
  logStep('发布 Gitee Release...')

  // 获取 Gitee token
  let giteeToken = process.env.GITEE_TOKEN
  if (!giteeToken) {
    try {
      giteeToken = execSync('git config --local gitee.token', { encoding: 'utf-8', stdio: 'pipe' }).trim()
    } catch {}
  }

  if (!giteeToken) {
    logError('未找到 Gitee token，跳过 Gitee Release')
    logInfo('请设置: git config --local gitee.token <your-token>')
    return
  }

  const https = require('https')
  const tagName = `v${version}`
  const body = JSON.stringify({
    access_token: giteeToken,
    tag_name: tagName,
    name: tagName,
    target_commitish: 'main',
    body: releaseNotes
  })

  const options = {
    hostname: 'gitee.com',
    path: '/api/v5/repos/orczh/cc-box/releases',
    method: 'POST',
    headers: { 'Content-Type': 'application/json' }
  }

  const req = https.request(options, res => {
    let data = ''
    res.on('data', chunk => data += chunk)
    res.on('end', () => {
      if (res.statusCode >= 200 && res.statusCode < 300) {
        logSuccess(`Gitee Release ${tagName} 已发布！`)
        logInfo(`查看: https://gitee.com/orczh/cc-box/releases/${tagName}`)
      } else {
        logError(`Gitee Release 发布失败: HTTP ${res.statusCode}`)
        logInfo('请手动发布: https://gitee.com/orczh/cc-box/releases')
      }
    })
  })

  req.on('error', e => {
    logError(`Gitee Release 发布失败: ${e.message}`)
  })

  req.write(body)
  req.end()
}

// ============================================
// OSS 上传
// ============================================

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

function downloadGitHubRelease(version) {
  logStep('下载 GitHub Release 产物...')

  // 保存到项目目录 releases/v0.5.1/...（统一管理，不删除）
  const releasesDir = path.join(PROJECT_ROOT, 'releases')
  const versionDir = path.join(releasesDir, version)

  // 如果已存在，直接返回（避免重复下载）
  if (fs.existsSync(versionDir)) {
    const files = fs.readdirSync(versionDir)
    if (files.length > 0) {
      logSuccess(`产物已存在: ${versionDir}`)
      return { versionDir }
    }
  }

  fs.mkdirSync(versionDir, { recursive: true })

  execWithProxyRetry(`gh release download ${version} --dir "${versionDir}" --pattern "*.exe" --pattern "*.dmg" --pattern "*.AppImage" --clobber`)
  logSuccess(`产物下载完成: ${versionDir}`)

  return { versionDir }
}

function uploadToOSS(version, versionDir, releaseNotes) {
  logStep('上传到阿里云 OSS...')

  const config = loadOssConfig()
  if (!config) return

  const { bucketName, region, accessKeyId, accessKeySecret } = config
  const endpoint = `${region}.aliyuncs.com`

  // 检查 ossutil
  const ossUtilPath = path.join(PROJECT_ROOT, 'ossutil64.exe')
  if (!fs.existsSync(ossUtilPath)) {
    logInfo('下载 ossutil...')
    const https = require('https')
    const writeStream = fs.createWriteStream(ossUtilPath)
    https.get('https://gosspublic.alicdn.com/ossutil/1.7.14/ossutil64.exe', res => {
      res.pipe(writeStream)
    })
    // 同步等待下载完成
    execSync('node -e "const https=require(\'https\'),fs=require(\'fs\'),ws=fs.createWriteStream(process.argv[1]);https.get(process.argv[2],r=>r.pipe(ws))" "' + ossUtilPath + '" "https://gosspublic.alicdn.com/ossutil/1.7.14/ossutil64.exe"', { stdio: 'ignore' })
  }

  // 配置 ossutil
  exec(`"${ossUtilPath}" config -e ${endpoint} -i ${accessKeyId} -k ${accessKeySecret} -L CH`)

  // 上传安装包
  const files = fs.readdirSync(versionDir).map(f => ({
    name: f,
    path: path.join(versionDir, f),
  }))

  for (const file of files) {
    exec(`"${ossUtilPath}" cp "${file.path}" "oss://${bucketName}/cc-box/${version}/" -f`)
    logSuccess(`已上传: ${file.name}`)
  }

  // 生成 latest.json
  const winFile = files.find(f => /-setup\.exe$/i.test(f.name))
  const macFile = files.find(f => /\.dmg$/i.test(f.name))
  const linuxFile = files.find(f => /\.AppImage$/i.test(f.name))

  // 获取文件大小
  const getFileSize = (filePath) => {
    if (!filePath) return 0
    try {
      return fs.statSync(filePath).size
    } catch {
      return 0
    }
  }

  const latestJson = {
    version,
    release_date: new Date().toISOString().split('T')[0],
    release_notes: releaseNotes || '',
    release_notes_url: `https://github.com/orczh-hj/cc-box/releases/tag/${version}`,
    assets: {
      windows: {
        url: `https://${bucketName}.${endpoint}/cc-box/${version}/${winFile?.name || ''}`,
        size: getFileSize(winFile?.path),
      },
      macos: {
        url: `https://${bucketName}.${endpoint}/cc-box/${version}/${macFile?.name || ''}`,
        size: getFileSize(macFile?.path),
      },
      linux: {
        url: `https://${bucketName}.${endpoint}/cc-box/${version}/${linuxFile?.name || ''}`,
        size: getFileSize(linuxFile?.path),
      },
    },
  }

  // latest.json 保存到项目 releases 目录
  const jsonPath = path.join(versionDir, 'latest.json')
  fs.writeFileSync(jsonPath, JSON.stringify(latestJson, null, 2))
  exec(`"${ossUtilPath}" cp "${jsonPath}" "oss://${bucketName}/cc-box/latest.json" -f`)
  logSuccess('latest.json 已上传')
}

// ============================================
// 独立 OSS 上传命令
// ============================================

function ossUploadOnly(version) {
  const { versionDir } = downloadGitHubRelease(version)

  // 从 GitHub API 获取 release notes（需要代理）
  const releaseNotes = execWithProxyRetry(`gh release view ${version} --json body --jq .body`, { silent: true, encoding: 'utf-8' }).trim()

  uploadToOSS(version, versionDir, releaseNotes)
  logSuccess(`OSS 上传完成: ${version}`)
}

// ============================================
// 参数解析
// ============================================

function parseArgs() {
  const args = process.argv.slice(2)
  const parsed = { bumpType: null, releaseNotes: null, skipCI: false, ossOnly: null, yes: true, exact: false }

  for (let i = 0; i < args.length; i++) {
    switch (args[i]) {
      case '--bump':
        parsed.bumpType = args[++i]
        break
      case '--notes':
        parsed.releaseNotes = args[++i]?.replace(/\\n/g, '\n')
        break
      case '--skip-ci':
        parsed.skipCI = true
        break
      case '--oss-only':
        parsed.ossOnly = args[++i]
        break
      case '--yes':
        parsed.yes = true
        break
      case '--no':
        parsed.yes = false
        break
      case '--exact':
        parsed.exact = true
        break
      case '--help':
      case '-h':
        console.log(`
CC-Box 自动化发布脚本（全自动，无需交互）

用法:
  npm run release -- --bump <type> --notes "<notes>"    发布新版本
  npm run release -- --exact --notes "<notes>"          发布当前版本（不 bump）
  npm run release -- --oss-only <version>               仅上传 OSS

参数:
  --bump <type>      版本类型: major / minor / patch（与 --exact 二选一）
  --exact            使用当前版本发布，不 bump 版本号
  --notes "<notes>"  Release notes，用 \\n 表示换行（必填）
  --skip-ci          跳过 CI 监控（标签已构建时使用）
  --oss-only <ver>   仅下载指定版本并上传 OSS（如 --oss-only v0.5.1）
  --yes              自动确认（默认，Claude 自动化时使用）
  --no               显示确认提示（人工执行时使用）

示例:
  npm run release -- --bump patch --notes "### Fixed\\n- Fix copy issue"
  npm run release -- --exact --notes "### Features\\n- Add feature"
  npm run release -- --oss-only v0.5.1
`)
        process.exit(0)
    }
  }
  return parsed
}

// ============================================
// 主流程
// ============================================

async function main() {
  const args = parseArgs()

  // 独立 OSS 上传模式
  if (args.ossOnly) {
    ossUploadOnly(args.ossOnly)
    return
  }

  // 参数检查：--bump 或 --exact 二选一
  if (!args.exact && !args.bumpType) {
    logError('需要指定 --bump <type> 或 --exact')
    logInfo('使用 --help 查看帮助')
    process.exit(1)
  }

  if (args.bumpType && !['major', 'minor', 'patch'].includes(args.bumpType)) {
    logError(`无效的 bump 类型: ${args.bumpType}`)
    process.exit(1)
  }

  if (!args.releaseNotes) {
    logError('缺少必填参数 --notes')
    logInfo('使用 --help 查看帮助')
    process.exit(1)
  }

  console.log('\x1b[35m======================================')
  console.log('     CC-Box 自动化发布脚本')
  console.log('======================================\x1b[0m')

  const currentVersion = getCurrentVersion()
  const newVersion = args.exact ? currentVersion : bumpVersion(currentVersion, args.bumpType)

  if (args.exact) {
    console.log(`\n\x1b[33m发布当前版本: v${newVersion}\x1b[0m`)
  } else {
    console.log(`\n\x1b[33m版本更新: v${currentVersion} -> v${newVersion}\x1b[0m`)
    console.log(`\x1b[33m更新类型: ${args.bumpType}\x1b[0m`)
  }

  console.log('\n即将执行以下操作：')
  console.log('  1. 更新版本号文件')
  console.log('  2. 更新 CHANGELOG.md')
  console.log('  3. Git 提交并推送')
  console.log(`  4. 创建并推送标签 v${newVersion}`)
  if (!args.skipCI) console.log('  5. 监控 CI 构建')
  console.log('  6. 发布 GitHub Release')
  console.log('  7. 发布 Gitee Release')
  console.log('  8. 上传到阿里云 OSS（国内更新渠道）')

  console.log('\nRelease Notes 预览：')
  console.log(args.releaseNotes)

  // 全自动模式无需确认
  if (!args.yes) {
    const confirmed = await askConfirm('\n是否继续？(y/n) ')
    if (!confirmed) {
      logInfo('已取消发布')
      process.exit(0)
    }
  }

  // 执行发布流程
  if (!args.exact) {
    updateVersionFiles(newVersion)
  }
  updateChangelog(newVersion, args.releaseNotes)
  gitCommit(newVersion)
  gitPush(newVersion)

  if (!args.skipCI) {
    await watchCIBuild()
  }

  publishRelease(newVersion, args.releaseNotes)
  publishGiteeRelease(newVersion, args.releaseNotes)

  // 下载并上传到 OSS
  const { versionDir } = downloadGitHubRelease(`v${newVersion}`)
  uploadToOSS(`v${newVersion}`, versionDir, args.releaseNotes)

  // 清理代理配置
  clearAllProxyConfig()

  console.log(`\n\x1b[32m======================================`)
  console.log(`     发布完成！v${newVersion}`)
  console.log('======================================\x1b[0m')
}

main().catch(e => {
  logError(e.message)
  process.exit(1)
})