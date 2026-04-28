# 版本发布流程

本文档描述 CC-Box 的自动化版本发布流程。

## 版本号规则

遵循 [语义化版本](https://semver.org/lang/zh-CN/)：`vMAJOR.MINOR.PATCH`

## 版本号更新位置

| 文件 | 路径 |
|------|------|
| Cargo.toml | `src-tauri/Cargo.toml` → `version` |
| package.json | `package.json` → `version` |
| tauri.conf.json | `src-tauri/tauri.conf.json` → `version` |

三个文件的版本号必须保持一致。

## 自动化发布流程

推送版本标签后，CI 自动完成以下步骤：

```
git push origin v0.2.4
  ↓
GitHub Actions 触发构建
  ↓
构建三平台安装包（Windows/macOS/Linux）
  ↓
等待构建完成（gh run watch）
  ↓
自动生成 Release notes（从 CHANGELOG.md 提取）
  ↓
自动发布 Release（--draft=false）
```

## 发布命令

```bash
# 1. 更新版本号
#    编辑: src-tauri/Cargo.toml, package.json, src-tauri/tauri.conf.json

# 2. 更新 CHANGELOG.md（在对应版本区块添加变更记录）

# 3. 提交并推送
git add -A
git commit -m "Release v0.2.4"
git push origin main

# 4. 创建并推送标签（触发自动化发布）
git tag -a v0.2.4 -m "Release v0.2.4"
git push origin v0.2.4
```

## 构建产物

CI 自动构建并上传以下产物：

| 平台 | 产物 |
|------|------|
| Windows (x64) | `.exe` (NSIS) + `.msi` |
| macOS (Universal) | `.dmg` + `.app` |
| Linux (x64) | `.deb` + `.AppImage` |

## CHANGELOG 格式

`CHANGELOG.md` 用于记录版本变更，CI 从中提取 Release notes：

```markdown
## [v0.2.4] - 2026-04-29

### Bug Fixes
- Fix terminal copy (Ctrl+C/Ctrl+Shift+C)
- Fix console window flash on Windows (CREATE_NO_WINDOW)
- Fix new/restart session shortcuts (Alt+N/Alt+R)

### Features
- Add Alt+N/Alt+R shortcuts for new/restart session
- Add shortcut hints on session buttons
- Spawn new app instance instead of multi-window
- Preload sidebar data on startup

## [v0.2.3] - 2026-04-28
...
```

## 发布文档位置

| 文档 | 说明 |
|------|------|
| `CHANGELOG.md` | 版本变更记录 |
| `docs/release-process.md` | 发布流程说明 |
| GitHub Releases | 发布产物与说明 |

## 回滚流程

```bash
# 删除远程标签和 Release
git push origin :refs/tags/v0.2.4
git tag -d v0.2.4
gh release delete v0.2.4 --yes

# 修复问题后发布新版本
```