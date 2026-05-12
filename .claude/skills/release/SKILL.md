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