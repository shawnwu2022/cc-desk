# 终端集成架构

## 架构概述

本项目使用 **Tauri 2 + xterm.js + portable-pty** 直接运行 Claude CLI，实现完全的原生终端体验。

### 核心依赖

| 包名 | 版本 | 用途 |
|------|------|------|
| @xterm/xterm | ^5.5.0 | 终端 UI 渲染 |
| @xterm/addon-fit | ^0.11.0 | 自动适应容器大小 |
| @xterm/addon-search | ^0.16.0 | 终端内搜索 |
| @xterm/addon-web-links | ^0.12.0 | 链接点击 |
| @xterm/addon-serialize | ^0.14.0 | 会话序列化 |
| portable-pty | ^0.8 | 伪终端进程管理（Rust） |
| tauri | ^2 | 应用框架 |

## Rust 后端 PTY 模块 (src-tauri/src/pty.rs)

### 核心功能

```rust
// 启动 Claude CLI 进程
pub fn spawn(&self, cwd: String, cols: u16, rows: u16, args: Option<Vec<String>>) -> Result<PtyInfo>

// 写入数据到 PTY
pub fn write(&self, id: &str, data: &str) -> Result<()>

// resize PTY
pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()>

// 终止 PTY
pub fn kill(&self, id: &str) -> Result<()>
pub fn kill_all(&self)
```

### Claude 进程启动

Claude CLI 有多种安装方式，启动命令构建需要智能检测启动类型：

#### 安装方式与文件特征

| 安装方式 | 命令 | 文件类型 | Windows路径示例 | Mac/Linux路径示例 |
|---------|------|---------|-----------------|-------------------|
| Native Install | `curl -fsSL https://claude.ai/install.sh | bash` | 编译后可执行文件 | `~/.local/bin/claude.exe` | `~/.local/bin/claude` |
| npm | `npm install -g @anthropic-ai/claude-code` | Node.js脚本 | `...\node_global\node_modules\@anthropic-ai\claude-code\cli.js` | `.../node_modules/@anthropic-ai/claude-code/cli.js` |
| Homebrew | `brew install --cask claude-code` | 编译后可执行文件 | - | `/usr/local/bin/claude` |
| WinGet | `winget install Anthropic.ClaudeCode` | 编译后可执行文件 | 系统PATH | - |

#### npm shim 脚本结构

npm安装会创建shim脚本（如`D:\...\node_global\claude`），内容为shell脚本：

```bash
#!/bin/sh
basedir=$(dirname "$(echo "$0" | sed -e 's,\\,/,g')")
if [ -x "$basedir/node" ]; then
  exec "$basedir/node" "$basedir/node_modules/@anthropic-ai/claude-code/cli.js" "$@"
else 
  exec node "$basedir/node_modules/@anthropic-ai/claude-code/cli.js" "$@"
fi
```

cli.js文件特征：
- 第一行：`#!/usr/bin/env node`
- 包含Anthropic版权：`// (c) Anthropic PBC. All rights reserved...`

#### 启动类型检测逻辑

检测优先级（检测结果保存到配置避免重复检测）：

```
1. 检查扩展名
   path.ends_with(".js") → node

2. 检查文件内容（前5行）
   "#!/usr/bin/env node" → node
   "// (c) Anthropic" + "Version:" → node (cli.js特征)

3. Mac/Linux: 解析符号链接
   canonicalize(path).ends_with(".js") → node
   真实文件内容检测 → node

否则 → direct
```

#### 命令构建逻辑

```rust
// Windows
if ends_with(".exe") {
    // 直接执行编译版
    CommandBuilder::new(&claude_path)
} else if needs_node_launcher() {
    // Node.js脚本：用node执行
    CommandBuilder::new("node").arg(&claude_path)
} else {
    // shim脚本：用cmd.exe包装
    CommandBuilder::new("cmd.exe").arg("/C").arg(&claude_path)
}

// Mac/Linux
if needs_node_launcher() {
    CommandBuilder::new("node").arg(&claude_path)
} else {
    CommandBuilder::new(&claude_path)  // 直接执行
}
```

#### 配置存储

检测结果保存到 `~/.cc-box/config.json`：

```jsonc
{
  "claudeLauncherType": "node"  // 或 "direct"
}
```

PTY启动时优先从配置读取，无值时检测并保存。

#### 环境变量

```rust
// Windows: Git Bash 路径
.env("CLAUDE_CODE_GIT_BASH_PATH", detected_path)

// 基础终端环境
.env("TERM", "xterm-256color")
.env("COLORTERM", "truecolor")
.env("PWD", cwd)
```
```

### 数据流

```
PTY reader → emit('pty-output', { id, data }) → frontend
PTY exit → emit('pty-exit', { id, exit_code }) → frontend
```

## IPC 通道 (Tauri Commands)

### PTY 操作命令

| 命令 | 方向 | 参数 | 返回 |
|------|------|------|------|
| pty_spawn | renderer→main | { cwd, cols, rows, type, args } | PtySpawnResult |
| pty_input | renderer→main | { id, data } | boolean |
| pty_resize | renderer→main | { id, cols, rows } | boolean |
| pty_kill | renderer→main | { id } | boolean |
| pty_kill_all | renderer→main | - | void |

### PTY 事件

| 事件 | 方向 | 数据 |
|------|------|------|
| pty:output | main→renderer | { id, data } |
| pty:exit | main→renderer | { id, exit_code, signal? } |

## 前端 API (src/api/tauri.ts)

### PTY 操作

```typescript
interface PtySpawnOptions {
  cwd: string
  cols: number
  rows: number
  type: 'claude' | 'shell'
  args?: string[]
}

ptySpawn(options: PtySpawnOptions): Promise<PtySpawnResult>
ptyInput(id: string, data: string): Promise<boolean>
ptyResize(id: string, cols: number, rows: number): Promise<boolean>
ptyKill(id: string): Promise<boolean>
ptyKillAll(): Promise<void>
```

### PTY 事件监听

```typescript
onPtyOutput((payload) => { ... }): Promise<UnlistenFn>
onPtyExit((payload) => { ... }): Promise<UnlistenFn>
```

## 渲染进程终端组件 (XTermTerminal.vue)

### xterm.js 配置

```typescript
const term = new Terminal({
  fontFamily: 'Cascadia Code, Fira Code, Consolas, monospace',
  fontSize: 12,
  lineHeight: 1.2,
  cursorBlink: true,
  cursorStyle: 'bar',
  theme: lightTheme,
  allowProposedApi: true,
  macOptionIsMeta: true, // macOS Option 作为 Meta
})
```

### 浅色终端主题

```typescript
const lightTheme = {
  background: '#f8f9fa',      // 浅灰背景
  foreground: '#1a1a2e',      // 深色文字
  cursor: '#6c5ce7',          // 紫色光标
  selectionBackground: '#ede9fe',
  // ANSI 16色...
}
```

### 数据绑定

```typescript
// 用户输入 → PTY
term.onData(data => {
  const instance = terminalInstances.get(tabId)
  if (instance) {
    ptyInput(instance.ptyId, data)
  }
})

// PTY 输出 → Terminal
onPtyOutput(({ id, data }) => {
  const instance = terminalInstances.get(tabId)
  if (instance && id === instance.ptyId) {
    instance.term.write(data)
  }
})

// resize 同步
term.onResize(({ cols, rows }) => {
  ptyResize(instance.ptyId, cols, rows)
})
```

### Ctrl+V 粘贴处理

```typescript
term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
  if (event.ctrlKey && event.key === 'v') {
    event.preventDefault()
    readText().then(text => {
      if (text) term.paste(text)
    })
    return false
  }
  return true
})
```

## 快捷键处理机制

### 原则

**应用级快捷键由 Rust 端 `tauri-plugin-global-shortcut` 在 OS 层拦截，通过事件传递到前端处理；终端快捷键由 xterm.js + PTY 原生处理。**

- 应用快捷键：Rust 端 `lib.rs` 注册 `RegisterHotKey` → `app.emit("shortcut:*")` → 前端 `listen()` → `useAppShortcuts.ts`
- 终端快捷键：xterm.js 通过 `onData` 发送到 PTY，由 Claude CLI 处理
- 窗口焦点检查：Windows 使用 `GetForegroundWindow()` FFI 直接检查前台窗口，其他平台使用 `on_window_event(Focused)`

### 应用级快捷键

| 快捷键 | 功能 | 处理方 |
|--------|------|--------|
| Ctrl+Shift+N | 新建窗口 | Rust global-shortcut → emit |
| Ctrl+Shift+←/→ | 窗口左移/右移半屏 | Rust global-shortcut → emit |
| Ctrl+Shift+R | 重启应用 | Rust global-shortcut → emit |
| Ctrl+, | 打开设置 | Rust global-shortcut → emit |
| Ctrl+Plus/Minus | 增大/减小字体 | Rust global-shortcut → emit |
| Ctrl+0 | 重置字体 | Rust global-shortcut → emit |
| Alt+N/R | 新建/重启会话 | Rust global-shortcut → emit |
| Alt+↑/↓ | 切换标签 | Rust global-shortcut → emit |
| Ctrl+Shift+H | 回到项目列表 | Rust global-shortcut → emit |

### 终端快捷键（由 Claude CLI 处理）

| 快捷键 | 功能 | 处理方 |
|--------|------|--------|
| Ctrl+C | 取消当前输入/生成 | xterm.js → PTY → Claude CLI |
| Ctrl+D | 退出 Claude Code | xterm.js → PTY → Claude CLI |
| Ctrl+L | 清屏 | xterm.js → PTY → Claude CLI |
| Ctrl+R | 反向搜索历史 | xterm.js → PTY → Claude CLI |
| Ctrl+W | 删除前一个单词 | xterm.js 发送 `\x17` → PTY |
| Ctrl+B | 后台运行任务 | xterm.js → PTY → Claude CLI |
| Alt+P | 切换模型 | xterm.js → PTY → Claude CLI |
| Alt+T | 切换扩展思考 | xterm.js → PTY → Claude CLI |

### 窗口前台检查

```rust
// src-tauri/src/lib.rs
// Windows: 使用 GetForegroundWindow() 直接检查前台窗口
// 其他平台: 使用 on_window_event(Focused) 追踪焦点状态
#[cfg(target_os = "windows")]
fn is_window_foreground() -> bool {
    let hwnd = OUR_HWND.load(Ordering::SeqCst);
    if hwnd.is_null() { return true; }
    unsafe { GetForegroundWindow() == hwnd }
}
```

详细快捷键架构文档见 [docs/interaction.md](interaction.md)。

## 环境检查 (src-tauri/src/checks.rs)

### 检查项

```rust
pub fn run_checks() -> CheckResults {
    vec![
        check_claude_cli(),    // claude 命令可用
        check_git_bash(),      // Git Bash 路径（Windows）
    ]
}
```

### 启动流程

```rust
// src-tauri/src/lib.rs
.on_window_event(|_window, event| {
    if let tauri::WindowEvent::CloseRequested { .. } = event {
        if let Some(manager) = pty::get_pty_manager() {
            manager.kill_all();
        }
    }
})
```

## 进程生命周期

```
启动 → 环境检查 → PTY spawn → Claude CLI 运行
                              ↓
                       用户交互（双向数据流）
                              ↓
关闭窗口 → kill_all() → PTY 进程清理 → Claude CLI 退出
```

## 与 Electron 架构的差异

| 特性 | Electron | Tauri |
|------|----------|-------|
| PTY 实现 | node-pty (Node) | portable-pty (Rust) |
| 快捷键拦截 | `before-input-event` | DOM capturing phase |
| Ctrl+W 处理 | 需手动拦截发送 | xterm.js 原生处理 |
| IPC | ipcMain/ipcRenderer | invoke/listen |
| 进程通信 | webContents.send | emit/listen |
