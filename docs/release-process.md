# 版本发布流程

## 版本号更新位置

三个文件的版本号必须保持一致：

| 文件 | 路径 |
|------|------|
| Cargo.toml | `src-tauri/Cargo.toml` → `version` |
| package.json | `package.json` → `version` |
| tauri.conf.json | `src-tauri/tauri.conf.json` → `version` |

## 发布命令

```bash
# 1. 更新版本号（编辑三个文件）

# 2. 提交并推送
git add -A && git commit -m "Release v0.2.5"
git push origin main

# 3. 创建并推送标签（触发 CI 构建）
git tag -a v0.2.5 -m "Release v0.2.5"
git push origin v0.2.5

# 4. 监控 CI 构建
gh run watch <run-id> --exit-status
# 或访问 https://github.com/orczh-hj/cc-box/actions

# 5. 发布 Release（必须附带 release notes）
gh release edit v0.2.5 --draft=false --notes "$(cat <<'EOF'
## What's Changed

### Bug Fixes
- ...

### Features
- ...
EOF
)"
```

## 构建产物

CI 自动构建并上传：

| 平台 | 产物 |
|------|------|
| Windows (x64) | `.exe` (NSIS) + `.msi` |
| macOS (Universal) | `.dmg` + `.app` |
| Linux (x64) | `.deb` + `.AppImage` |

## CHANGELOG 格式

`CHANGELOG.md` 用于记录版本变更，CI 从中提取 Release notes：

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
3. **推送到 GitHub 需要代理**（国内环境）

## 回滚流程

```bash
git push origin :refs/tags/v0.2.5
git tag -d v0.2.5
gh release delete v0.2.5 --yes
```
