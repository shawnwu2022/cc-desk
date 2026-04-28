# 版本发布流程

本文档描述 CC-Box 的自动化版本发布流程。

## 版本号规则

遵循 [语义化版本](https://semver.org/lang/zh-CN/)：`vMAJOR.MINOR.PATCH`

## 版本号更新位置

| 文件 | 字段 |
|------|------|
| `src-tauri/Cargo.toml` | `version` |
| `package.json` | `version` |
| `src-tauri/tauri.conf.json` | `version` |

三个文件的版本号必须保持一致。

## 自动化发布流程

执行一条命令完成全部发布流程：

```bash
npm run release
```

该命令自动完成以下步骤：

1. **检查前置条件** - 确保 git 状态干净、版本号一致
2. **更新 CHANGELOG.md** - 从模板生成版本变更记录
3. **提交更改** - 创建规范格式的 commit
4. **推送代码** - 推送到 GitHub main 分支
5. **创建标签** - 创建 annotated tag
6. **推送标签** - 触发 GitHub Actions 构建
7. **等待 CI** - 监控 Actions 构建进度
8. **发布 Release** - 自动发布草稿 Release

## 发布前检查清单

- [ ] 本地构建测试通过 (`npm run build && cargo check`)
- [ ] 版本号已更新（三个文件一致）
- [ ] CHANGELOG.md 已更新当前版本内容
- [ ] 无未提交的敏感信息

## 手动发布（备用）

如果自动化流程失败，可手动执行：

```bash
# 1. 提交更改
git add -A
git commit -m "Release v0.2.4

Bug fixes:
- Fix terminal copy

Features:
- Add Alt+N shortcut

Author <Chen Zihan>: orczh_hj@163.com"

# 2. 推送并创建标签
git push origin main
git tag -a v0.2.4 -m "Release v0.2.4"
git push origin v0.2.4

# 3. 等待 CI 并发布
gh run watch --exit-status
gh release edit v0.2.4 --draft=false
```

## GitHub Actions 构建产物

CI 自动构建三平台安装包：

| 平台 | 产物 |
|------|------|
| Windows (x64) | `.exe` (NSIS) + `.msi` |
| macOS (Universal) | `.dmg` + `.app` |
| Linux (x64) | `.deb` + `.AppImage` |

## 发布文档模板

Release Notes 和 CHANGELOG 使用统一模板，参见 [release-template.md](./release-template.md)。

## 体积优化

当前已启用：
- `strip = true` — 移除调试符号
- `lto = true` — 链接时优化

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
