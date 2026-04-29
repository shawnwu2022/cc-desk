# 启动先决条件检查

## 概述

应用启动时自动检测 Claude CLI 和 Git Bash（仅 Windows）的可用性。检测成功的路径和启动类型自动保存到配置文件，后续启动直接从配置读取。

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
  │   → appStore.runChecks()            ← 拉取后端缓存的检查结果
  │   ├─ 全部通过 → initAfterChecks() 进入主界面
  │   └─ 有失败   → 显示检查失败面板，用户手动填写或安装
```

## Claude CLI 检查

优先级从高到低，找到即返回：

| 步骤 | 来源 | 说明 |
|------|------|------|
| 1 | config.claudePath | 配置文件保存的路径，路径存在即通过 |
| 2 | `where`/`which` 查找 | 系统路径搜索 `claude`（Windows 为 `claude.exe`） |

全部未找到 → 返回失败结果，附带安装引导链接。

### 启动类型检测

找到路径后，检测启动类型并保存：

| 检测方式 | 条件 | 结果 |
|---------|------|------|
| 扩展名检测 | `path.ends_with(".js")` | node |
| 内容shebang检测 | 前5行包含 `#!/usr/bin/env node` | node |
| Anthropic版权检测 | 前5行包含 `// (c) Anthropic` + `Version:` | node (cli.js特征) |
| 符号链接解析(Mac/Linux) | 真实路径以`.js`结尾 | node |
| 其他情况 | - | direct |

检测结果保存到 `config.claudeLauncherType`。

### Node.js可用性检查

如果启动类型为 `node`，额外检查 `node` 命令是否可用：

```
where node (Windows) / which node (Mac/Linux)
```

不可用时返回失败，提示安装 Node.js。

## Git Bash 检查（仅 Windows）

优先级从高到低，找到即返回：

| 步骤 | 来源 | 说明 |
|------|------|------|
| 1 | config.gitBashPath | 配置文件保存的路径，路径存在即通过 |
| 2 | 环境变量 `CLAUDE_CODE_GIT_BASH_PATH` | Claude Code 原生支持的环境变量 |
| 3 | `where git.exe` → 推导安装目录 | 找到 git.exe 后，向上取安装目录，拼接 `bin/bash.exe` |

第 3 步推导逻辑：

```
where git.exe → C:\Program Files\Git\cmd\git.exe
                    └── parent ──┘ └─ file
parent = "cmd" → git_install = parent.parent = C:\Program Files\Git
bash_path = C:\Program Files\Git\bin\bash.exe
```

处理 `cmd` 和 `bin` 两种常见子目录结构。

## 路径自动保存

`save_detected_paths()` 在 `run_checks()` 结束时调用：

- 仅保存 **passed=true** 的检查项的 `detected_path` 和启动类型
- 写入 `~/.cc-box/config.json` 的 `claudePath` / `claudeLauncherType` / `gitBashPath` 字段
- 使用 `update_app_config()` 合并更新，不影响其他配置项
- 调用时机：首次启动（自动检测）和用户点击 Retry（重新检测）

## 前端交互

检查失败时，App.vue 显示全屏遮罩：

- 每个失败的检查项显示输入框 + Browse 按钮 + 安装引导链接
- 用户手动填写路径后，点击 **Save & Retry**：
  1. 将用户填写的路径写入 `config.json`
  2. 重新调用 `run_checks()`（force=true）
  3. 全部通过则进入主界面

## 配置文件结构

`~/.cc-box/config.json` 中相关字段：

```jsonc
{
  "claudePath": "C:\\Users\\xxx\\.local\\bin\\claude.exe",  // Claude CLI 路径
  "claudeLauncherType": "direct",                          // 启动类型：direct 或 node
  "gitBashPath": "C:\\Program Files\\Git\\bin\\bash.exe"    // Git Bash 路径
}
```

字段为可选。首次启动时由检查逻辑自动填充，用户也可在设置中手动修改。

## PTY 启动时的路径和类型使用

PTY 层（`pty.rs`）在 spawn Claude CLI 进程时：

```
detect_claude_path()
  1. config.claudePath 存在 → 直接使用
  2. ~/.local/bin/claude.exe (Windows)
  3. PATH 环境变量搜索

should_use_node_launcher()
  1. config.claudeLauncherType 存在 → 使用配置值
  2. 配置无值 → 检测并保存

命令构建：
  Windows + .exe → 直接执行
  Windows + 需node → node cli.js
  Windows + shim → cmd.exe /C shim
  Mac/Linux + 需node → node cli.js
  Mac/Linux + 编译版 → 直接执行
```
