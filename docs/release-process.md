# 版本发布流程

本文档描述 CC-Box 的版本号管理、本地打包、CI/CD 发布、签名与分发。

## 版本号规则

遵循 [语义化版本](https://semver.org/lang/zh-CN/)：`vMAJOR.MINOR.PATCH`

## 版本号更新位置

| 文件 | 路径 |
|------|------|
| Cargo.toml | `src-tauri/Cargo.toml` → `version` |
| package.json | `package.json` → `version` |
| tauri.conf.json | `src-tauri/tauri.conf.json` → `version` |

三个文件的版本号必须保持一致。

## 发布前检查清单

- [ ] 所有已计划的 feature 已完成
- [ ] 所有已知 bug 已修复
- [ ] 本地构建测试通过 (`npm run build`)
- [ ] Rust 编译通过 (`cargo check`)
- [ ] 检查敏感信息不会被提交

## 快速发布命令

```bash
# 1. 更新版本号（编辑三个文件）
#    - src-tauri/Cargo.toml
#    - package.json
#    - src-tauri/tauri.conf.json

# 2. 提交更改（使用规范格式）
git add -A
git commit -m "Release v0.2.4

Bug fixes:
- Fix terminal copy (Ctrl+C/Ctrl+Shift+C)
- Fix console window flash on Windows

Features:
- Add Alt+N/Alt+R shortcuts for new/restart session

Author <Chen Zihan>: orczh_hj@163.com"

# 3. 推送到 GitHub 并创建标签
git push origin main
git tag -a v0.2.4 -m "Release v0.2.4"
git push origin v0.2.4

# 4. 等待 CI 构建完成后发布
#    访问 https://github.com/orczh-hj/cc-box/actions 查看进度
#    构建完成后执行：
gh release edit v0.2.4 --draft=false
```

## 详细流程

### 步骤 1：更新版本号

手动编辑以下文件中的 `version` 字段：

```toml
# src-tauri/Cargo.toml
version = "0.2.4"
```

```json
// package.json
"version": "0.2.4"
```

```json
// src-tauri/tauri.conf.json
"version": "0.2.4"
```

### 步骤 2：提交更改

使用规范的 commit 格式，包含版本号、变更内容：

```bash
git add -A
git commit -m "Release v0.2.4

Bug fixes:
- Fix terminal copy (Ctrl+C/Ctrl+Shift+C)
- Fix console window flash on Windows (CREATE_NO_WINDOW)
- Fix new/restart session shortcuts (Alt+N/Alt+R)
- Fix window snap shortcuts (Ctrl+Shift+Arrow)
- Fix sidebar data not loading on startup

Features:
- Add Alt+N/Alt+R shortcuts for new/restart session
- Add shortcut hints on session buttons
- Spawn new app instance instead of multi-window
- Preload sidebar data (skills/agents/mcp/plugins)

Author <Chen Zihan>: orczh_hj@163.com"
```

### 步骤 3：推送并创建标签

```bash
# 推送 commit
git push origin main

# 创建 annotated tag
git tag -a v0.2.4 -m "Release v0.2.4"

# 推送 tag（触发 CI 构建）
git push origin v0.2.4
```

### 步骤 4：等待 CI 构建

推送 tag 后，GitHub Actions 自动触发构建：

1. 访问 https://github.com/orczh-hj/cc-box/actions
2. 等待 "Release" workflow 完成（约 10-15 分钟）
3. 构建产物：
   - Windows (x64): `.exe` + `.msi`
   - macOS (Universal): `.dmg` + `.app`
   - Linux (x64): `.deb` + `.AppImage`

### 步骤 5：发布 Release

CI 构建完成后，会创建一个草稿 Release。发布方式：

**方式 A：使用 CLI**
```bash
gh release edit v0.2.4 --draft=false
```

**方式 B：手动发布**
1. 访问 https://github.com/orczh-hj/cc-box/releases
2. 找到 v0.2.4 draft
3. 编辑 Release notes
4. 点击 "Publish release"

## 签名与公证（可选）

### Windows 代码签名
在 `src-tauri/tauri.conf.json` 中配置:
```json
"bundle": {
  "windows": {
    "certificateThumbprint": "证书指纹",
    "timestampUrl": "http://timestamp.digicert.com"
  }
}
```

### macOS 代码签名
```bash
security import certificate.p12 -k ~/Library/Keychains/login.keychain-db
```
在 `tauri.conf.json` 配置 `signingIdentity` 和 `hardenedRuntime`。

## 回滚流程

```bash
# 删除标签和 Release
git push origin :refs/tags/v1.2.3
git tag -d v1.2.3
gh release delete v1.2.3

# 修复后发布新版本
```

## 体积优化

当前已启用：
- `strip = true` — 移除调试符号
- `lto = true` — 链接时优化
