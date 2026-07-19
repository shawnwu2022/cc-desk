# 版本发布流程

## 首次独立发布前

1. 在 GitHub 将仓库创建或重命名为 `shawnwu2022/cc-desk`，并更新本地 `origin`。
2. 为 CC Desk 生成独立的 Tauri updater 签名密钥；私钥保存到仓库 Secret `TAURI_SIGNING_PRIVATE_KEY`，密码保存到 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
3. 用对应公钥替换 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。当前值继承自 CC-Box，只能验证原签名密钥，不能作为 CC Desk 的独立发布密钥继续使用。
4. 首次发布后确认 Release 附件包含 `latest.json` 以及三端安装包和 `.sig` 文件，再启用客户端自动更新。

> 私钥不得提交到仓库、文档、日志或聊天记录。

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
| `--oss-only` |  | 可选镜像：仅上传到自行配置的 OSS，不属于默认发布链 |

### 执行流程

脚本自动执行以下步骤：
1. 更新版本号（Cargo.toml, package.json, tauri.conf.json）
2. 更新 CHANGELOG.md
3. Git 提交并推送（自动使用代理）
4. 创建并推送标签
5. 监控 CI 构建
6. 发布 GitHub Release
7. CI 生成 `latest.json` updater manifest，并与安装包一起上传到 GitHub Release

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

# 可选：上传到自行配置的 OSS 镜像
npm run release -- --oss-only v0.5.1
```

## 可选 OSS 镜像

默认更新通道是 CC Desk GitHub Releases。只有维护者自行配置镜像时，才使用以下兼容命令上传已有 Release 产物：

```bash
# 无需代理（OSS 国内直连）
npm run release:oss -- v0.5.1

# 或直接运行
node scripts/release.js --oss-only v0.5.1
```

## 可选 OSS 配置

OSS 配置文件：`scripts/oss-config.json`

```json
{
  "bucketName": "cc-desk",
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
# 或访问 https://github.com/shawnwu2022/cc-desk/actions

# 5. 发布 Release
gh release edit v0.2.5 --draft=false --notes "## What's Changed\n\n..."

# 6. 可选：上传到自行配置的 OSS 镜像（无需代理）
node scripts/release.js --oss-only v0.2.5
```

## Updater manifest

`.github/workflows/release.yml` 在汇总三端产物后运行 `scripts/generate-updater-manifest.js`，校验每个平台的签名文件并生成 `latest.json`。该文件会作为 GitHub Release 附件发布，对应应用配置中的：

```text
https://github.com/shawnwu2022/cc-desk/releases/latest/download/latest.json
```

manifest 缺少任一平台产物或 `.sig` 时 CI 直接失败，避免发布一个无法自动更新的版本。
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