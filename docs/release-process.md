# 版本发布流程

## 首次独立发布前

1. 在 GitHub 将仓库创建或重命名为 `shawnwu2022/cc-desk`，并更新本地 `origin`。
2. 为 CC Desk 生成独立的 Tauri updater 签名密钥；私钥保存到仓库 Secret `TAURI_SIGNING_PRIVATE_KEY`，密码保存到 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
3. 用对应公钥替换 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。当前值继承自 CC-Box，只能验证原签名密钥，不能作为 CC Desk 的独立发布密钥继续使用。
4. 首次发布后确认 Release 附件包含 `latest.json` 以及三端安装包和 `.sig` 文件，再启用客户端自动更新。

> 私钥不得提交到仓库、文档、日志或聊天记录。

## 自动化发布（推荐）

稳定版由受保护的 `main` 分支驱动发布：

1. 按语义化版本确定新版本，并同步更新所有版本文件与 `CHANGELOG.md`。
2. 创建发布 PR，等待前端与 Rust CI 全部通过后合入 `main`。
3. `package.json` 的版本变更触发 `.github/workflows/release.yml`。
4. 工作流解析 `v<package.version>`，把该标签显式传给 `scripts/generate-updater-manifest.js`，构建三端签名产物并发布稳定 GitHub Release。
5. Release 明确设置为 Latest；工作流发布后立即校验 Latest manifest 的版本和各平台资产链接，任一不一致都会让发布任务失败。

Release notes 与 `CHANGELOG.md` 必须使用英文，并以动词开头描述用户可观察的变化。不要在新版本之后重新发布旧草稿，否则 GitHub Latest 可能被旧版本抢占。

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

以下版本号必须保持一致：

| 文件 | 路径 |
|------|------|
| Cargo.toml | `src-tauri/Cargo.toml` → `version` |
| Cargo.lock | `src-tauri/Cargo.lock` → `cc-desk` package `version` |
| package.json | `package.json` → `version` |
| package-lock.json | `package-lock.json` → 根包 `version` |
| tauri.conf.json | `src-tauri/tauri.conf.json` → `version` |

## 手动发布（备用）

```bash
# 1. 更新版本号（编辑三个文件）

# 2. 创建发布分支、提交并发起 PR
git switch -c codex/release-v0.2.5
git add <release-files>
git commit -m "Release v0.2.5"
git push origin codex/release-v0.2.5

# 3. PR 的前端与 Rust CI 通过后合入 main；main 自动触发发布工作流

# 4. 监控 Release workflow
gh run watch <run-id> --exit-status
# 或访问 https://github.com/shawnwu2022/cc-desk/actions

# 5. 可选：上传到自行配置的 OSS 镜像（无需代理）
node scripts/release.js --oss-only v0.2.5
```

## Updater manifest

`.github/workflows/release.yml` 在汇总三端产物后，将解析出的精确发布标签显式传给 `scripts/generate-updater-manifest.js`，校验标签和每个平台的签名文件并生成 `latest.json`。分支名或其他非版本标签会导致工作流失败。该文件会作为 GitHub Release 附件发布，对应应用配置中的：

```text
https://github.com/shawnwu2022/cc-desk/releases/latest/download/latest.json
```

manifest 缺少任一平台产物或 `.sig` 时 CI 直接失败，避免发布一个无法自动更新的版本。稳定 Release 使用 `make_latest: true`，确保 `/releases/latest/` 更新入口指向本次版本。

GitHub Release 对带空格的产物名按点号发布（例如 `CC Desk_0.15.0_x64-setup.exe` 发布为 `CC.Desk_0.15.0_x64-setup.exe`）；生成 manifest 时必须使用该已发布资产名。发布验收必须确认 GitHub Latest 的 `tag_name` 是本次标签、`/releases/latest/download/latest.json` 的 `version` 是本次版本，并请求其中每个平台的 `url`，确认均不返回 404。
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
