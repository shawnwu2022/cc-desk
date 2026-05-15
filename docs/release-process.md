# 版本发布流程

## 自动化发布（推荐）

使用 `scripts/release.js` 脚本全自动发布：

```bash
# 设置代理（GitHub 访问需要）
export HTTP_PROXY=http://127.0.0.1:33210
export HTTPS_PROXY=http://127.0.0.1:33210

# 使用 npm 命令
npm run release -- --bump patch --notes "### Fixed\n- Fix terminal copy issue"

# 或直接运行脚本
node scripts/release.js --bump minor --notes "### Features\n- Add sidebar panel\n\n### Fixed\n- Memory leak"
```

### 参数说明

| 参数 | 必填 | 说明 |
|------|------|------|
| `--bump` | ✓* | 版本更新类型：`major` / `minor` / `patch`（与 `--exact` 二选一） |
| `--exact` | ✓* | 使用当前版本发布，不 bump 版本号（用于重新发布） |
| `--notes` | ✓ | Release notes 内容，用 `\n` 表示换行 |
| `--skip-ci` |  | 跳过 CI 监控（用于已构建的标签） |
| `--oss-only` |  | 仅下载指定版本并上传 OSS（如 `--oss-only v0.5.1`） |

### 执行流程

脚本自动执行以下步骤：
1. 更新版本号（Cargo.toml, package.json, tauri.conf.json）
2. 更新 CHANGELOG.md
3. Git 提交并推送（自动使用代理）
4. 创建并推送标签
5. 监控 CI 构建
6. 发布 GitHub Release
7. 下载 Release 产物并上传到阿里云 OSS（国内更新渠道）

### Release Notes 格式

```bash
# 多行格式示例（用 \n 表示换行）
--notes "### Bug Fixes\n- Fix issue A\n- Fix issue B\n\n### Features\n- Add feature X"
```

### 常用示例

```bash
# 新版本发布
npm run release -- --bump patch --notes "### Fixed\n- Fix copy issue"
npm run release -- --bump minor --notes "### Features\n- Add sidebar panel"

# 重新发布当前版本（CI 已构建）
npm run release -- --exact --notes "### Fixed\n- Fix issue" --skip-ci

# 仅上传 OSS（补传某个版本）
npm run release -- --oss-only v0.5.1
```

## 仅上传到 OSS

已有 Release 产物，只需上传到 OSS：

```bash
# 无需代理（OSS 国内直连）
npm run release:oss -- v0.5.1

# 或直接运行
node scripts/release.js --oss-only v0.5.1
```

## OSS 配置

OSS 配置文件：`scripts/oss-config.json`

```json
{
  "bucketName": "cc-box",
  "region": "oss-cn-beijing",
  "accessKeyId": "YOUR_ACCESS_KEY_ID",
  "accessKeySecret": "YOUR_ACCESS_KEY_SECRET"
}
```

**注意**：此文件已加入 `.gitignore`，不会提交到仓库。首次使用需复制示例文件：

```bash
cp scripts/oss-config.example.json scripts/oss-config.json
# 编辑填入阿里云 AccessKey
```

## 版本号更新位置

三个文件的版本号必须保持一致：

| 文件 | 路径 |
|------|------|
| Cargo.toml | `src-tauri/Cargo.toml` → `version` |
| package.json | `package.json` → `version` |
| tauri.conf.json | `src-tauri/tauri.conf.json` → `version` |

## 手动发布（备用）

```bash
# 1. 更新版本号（编辑三个文件）

# 2. 提交并推送（需要代理）
export HTTP_PROXY=http://127.0.0.1:33210
git add -A && git commit -m "Release v0.2.5"
git push origin main

# 3. 创建并推送标签（触发 CI 构建）
git tag -a v0.2.5 -m "Release v0.2.5"
git push origin v0.2.5

# 4. 监控 CI 构建
gh run watch <run-id> --exit-status
# 或访问 https://github.com/orczh-hj/cc-box/actions

# 5. 发布 Release
gh release edit v0.2.5 --draft=false --notes "## What's Changed\n\n..."

# 6. 上传到 OSS（无需代理）
node scripts/release.js --oss-only v0.2.5
```

## 构建产物

CI 自动构建并上传：

| 平台 | 产物 |
|------|------|
| Windows (x64) | `.exe` (NSIS) + `.exe.sig` |
| macOS (ARM) | `.dmg` + `.app.tar.gz` + `.app.tar.gz.sig` |
| Linux (x64) | `.AppImage` + `.AppImage.sig` |

## CHANGELOG 格式

`CHANGELOG.md` 用于记录版本变更：

```markdown
## [v0.2.5] - 2026-05-05

### Bug Fixes
- Fix terminal copy (Ctrl+C)

### Features
- Add Alt+N/Alt+R shortcuts
```

## 重要提醒

1. **每次发布必须编写 release notes**，说明变更内容
2. **CHANGELOG.md 用英文编写**，便于国际用户阅读
3. **GitHub 操作需要代理**（国内环境），OSS 操作无需代理
4. **OSS 配置文件包含敏感信息**，已在 `.gitignore` 中排除

## 回滚流程

```bash
git push origin :refs/tags/v0.2.5
git tag -d v0.2.5
gh release delete v0.2.5 --yes
```