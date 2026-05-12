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
| 用户指定级别 | `--bump patch/minor/major` |
| CI 已完成 | `--skip-ci` |
| 没有变更 | 提示用户，中止 |

## Notes 编写规范

### 格式

```markdown
### Fixed
- Fix <具体问题>

### Features
- Add <具体功能>

### Improvements
- Improve <具体改进>
```

### 规则

1. **英文**：全部用英文撰写
2. **动词开头**：Fix / Add / Improve / Update / Remove / Refactor
3. **具体描述**：写具体改动内容，不写抽象概括
4. **每条独立**：一个改动一条，不合并多条
5. **换行符**：`\n` 表示换行，类别之间空一行 `\n\n`

### 示例

正确：
```
### Fixed\n- Fix pending status not cleared on home page\n- Fix session badge showing for all projects\n\n### Features\n- Add working/pending indicators to home page session list
```

错误：
```
修复了一些bug，添加了新功能  ← 中文
Fix various issues  ← 抽象概括
Fix bug A and bug B  ← 多条合并
```

### 类别优先级

| 级别 | 首选类别 |
|------|----------|
| patch | Fixed |
| minor | Features / Improvements |
| major | Breaking Changes |

## 验收标准

| 项目 | 检查 | 预期 |
|------|------|------|
| GitHub Release | `gh release view v<x.y.z>` | 有 3 个资产 |
| Gitee Release | curl API | body 不为空，换行正确 |
| OSS latest.json | curl | version 正确 |
| OSS 安装包 | curl HEAD | 3 个 200 |
| 本地版本 | `git describe --tags` | 与发布一致 |