# 启动先决条件检查

应用启动时自动检测 Claude CLI 和 Git Bash（仅 Windows）的可用性。检测结果自动保存到配置文件，后续启动直接从配置读取。

涉及文件：`src-tauri/src/checks.rs`、`src-tauri/src/pty.rs`、`src-tauri/src/store.rs`、`src/App.vue`

## 检查流程

```
应用启动
  │
  ├─ lib.rs: CHECK_RESULTS (LazyLock)
  │   → checks::run_checks()
  │     ├─ read_config_paths()          ← 从 ~/.cc-box/config.json 读取已保存路径
  │     ├─ check_claude_cli()           ← 检查 Claude CLI + 检测启动类型
  │     ├─ check_git_bash()             ← 检查 Git Bash (Windows only)
  │     └─ save_detected_paths()        ← 通过的路径和启动类型写回 config.json
  │
  ├─ 前端 App.vue onMounted()
  │   → appStore.runChecks()
  │   ├─ 全部通过 → initAfterChecks() 进入主界面
  │   └─ 有失败   → 显示检查失败面板
```

## Claude CLI 检查

优先级从高到低：

| 步骤 | 来源 | 说明 |
|------|------|------|
| 1 | config.claudePath | 配置文件保存的路径 |
| 2 | `where`/`which` 查找 | 系统路径搜索 |

找到后检测启动类型并保存到 `config.claudeLauncherType`：

| 检测方式 | 条件 | 结果 |
|---------|------|------|
| 扩展名 | `.js` 结尾 | node |
| shebang | `#!/usr/bin/env node` | node |
| 版权特征 | `// (c) Anthropic` + `Version:` | node |
| 符号链接 (Mac/Linux) | 真实路径 `.js` 结尾 | node |
| 其他 | - | direct |

启动类型为 `node` 时额外检查 `node` 命令是否可用。

## Git Bash 检查（仅 Windows）

| 步骤 | 来源 | 说明 |
|------|------|------|
| 1 | config.gitBashPath | 配置文件保存的路径 |
| 2 | 环境变量 `CLAUDE_CODE_GIT_BASH_PATH` | Claude Code 原生支持 |
| 3 | `where git.exe` → 推导安装目录 | 找到 git.exe 后拼接 `bin/bash.exe` |

## 路径自动保存

`save_detected_paths()` 在 `run_checks()` 结束时调用：
- 仅保存 passed=true 的检查项
- 写入 `~/.cc-box/config.json` 的 `claudePath` / `claudeLauncherType` / `gitBashPath` 字段
- 使用 `update_app_config()` 合并更新

## 前端交互

检查失败时 App.vue 显示全屏遮罩：
- 每个失败项显示输入框 + Browse 按钮 + 安装引导链接
- 用户填写路径后点击 Save & Retry → 写入 config → 重新检测
