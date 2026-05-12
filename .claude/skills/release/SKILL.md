---
name: cc-box-release
description: CC-Box 版本发布。当用户说"发布"、"release"、"版本更新"、"patch/minor/major"时使用此 skill。
---

# CC-Box Release

执行 `npm run release`，自动从 git diff 生成 release notes。

## 基本命令

```bash
npm run release -- --bump <type>
```

## 参数

| 参数 | 说明 |
|------|------|
| `--bump patch/minor/major` | 版本类型 |
| `--exact` | 发布当前版本（不 bump） |
| `--skip-ci` | CI 已构建时使用 |
| `--oss-only v0.6.2` | 仅上传 OSS |

**Release Notes 自动生成**：对比上一版本标签的 git diff，提取 commit 分类。

## 示例

```bash
# 发布 patch（自动生成 notes）
npm run release -- --bump patch

# 发布 minor（手动指定 notes）
npm run release -- --bump minor --notes "### Features\n- Add feature"

# 重新发布当前版本
npm run release -- --exact --skip-ci
```