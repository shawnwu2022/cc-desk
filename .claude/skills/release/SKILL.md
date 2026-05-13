---
name: cc-box-release
description: CC-Box 版本发布。当用户说"发布"、"release"、"版本更新"、"准备发布"、"上传 OSS"时使用此 skill。
---

# CC-Box Release

## 标准流程

```bash
npm run release -- --bump <level> --notes "<notes>"
```

自动分析 diff → 决定级别 → 撰写 notes → 执行发布。

## 特殊情况

| 场景 | 命令 |
|------|------|
| 仅上传 OSS | `--oss-only v0.6.2` |
| 重新发布当前版本 | `--exact --skip-ci` |
| 用户指定级别 | `--bump patch/minor/major`（跳过自动判断） |
| CI 已完成 | `--skip-ci` |
| 没有变更 | 提示用户，中止 |

## Notes 格式

```markdown
### Fixed
- Fix <具体问题>

### Features
- Add <具体功能>
```

英文，`\n` 换行，动词开头。

## 验收标准

发布完成后，验证以下项：

| 项目 | 检查方式 | 预期 |
|------|----------|------|
| GitHub Release | `gh release view v<x.y.z>` | 存在，非 draft，有 3 个资产 |
| Gitee Release | curl API 检查 | 存在，body 不为空 |
| Release Notes | `gh release view` / Gitee 页面 | 格式正确，非字面 `\n`，动词开头，内容与 diff 匹配 |
| OSS latest.json | curl `https://<bucket>/cc-box/latest.json` | version 字段正确 |
| OSS 安装包 | curl HEAD 检查 | 3 个文件均返回 200 |
| 本地版本号 | `git describe --tags` | 与发布版本一致 |

全部通过 → 发布成功。

任一失败 → 排查并手动修复。