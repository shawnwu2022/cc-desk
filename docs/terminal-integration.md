# 终端集成架构

## 架构概述

使用 **Tauri 2 + xterm.js + portable-pty** 直接运行 Claude CLI，实现完全的原生终端体验。

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
pub fn spawn(&self, cwd: String, cols: u16, rows: u16, args: Option<Vec<String>>) -> Result<PtyInfo>
pub fn write(&self, id: &str, data: &str) -> Result<()>
pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()>
pub fn kill(&self, id: &str) -> Result<()>
pub fn kill_all(&self)
```

### Claude 进程启动

Claude CLI 有多种安装方式，启动命令构建需要智能检测启动类型：

#### 安装方式与文件特征

| 安装方式 | 命令 | 文件类型 | Windows路径示例 | Mac/Linux路径示例 |
|---------|------|---------|-----------------|-------------------|
| Native Install | `curl -fsSL https://claude.ai/install.sh \| bash` | 编译后可执行文件 | `~/.local/bin/claude.exe` | `~/.local/bin/claude` |
| npm | `npm install -g @anthropic-ai/claude-code` | Node.js脚本 | `...\node_global\node_modules\@anthropic-ai\claude-code\cli.js` | `.../node_modules/@anthropic-ai/claude-code/cli.js` |
| Homebrew | `brew install --cask claude-code` | 编译后可执行文件 | - | `/usr/local/bin/claude` |
| WinGet | `winget install Anthropic.ClaudeCode` | 编译后可执行文件 | 系统PATH | - |

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
if ends_with(".exe") { CommandBuilder::new(&claude_path) }
else if needs_node_launcher() { CommandBuilder::new("node").arg(&claude_path) }
else { CommandBuilder::new("cmd.exe").arg("/C").arg(&claude_path) }

// Mac/Linux
if needs_node_launcher() { CommandBuilder::new("node").arg(&claude_path) }
else { CommandBuilder::new(&claude_path) }
```

#### 配置存储

检测结果保存到 `~/.cc-box/config.json`：`{ "claudeLauncherType": "node" | "direct" }`

PTY 启动时优先从配置读取，无值时检测并保存。

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
  macOptionIsMeta: true,
})
```

### 数据绑定

```typescript
// 用户输入 → PTY
term.onData(data => { ptyInput(instance.ptyId, data) })

// PTY 输出 → Terminal
onPtyOutput(({ id, data }) => { instance.term.write(data) })

// resize 同步
term.onResize(({ cols, rows }) => { ptyResize(instance.ptyId, cols, rows) })
```

### Ctrl+V 粘贴处理

```typescript
term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
  if (event.ctrlKey && event.key === 'v') {
    event.preventDefault()
    readText().then(text => { if (text) term.paste(text) })
    return false
  }
  return true
})
```

## 快捷键处理机制

**应用级快捷键由 `useAppShortcuts.ts` 通过 DOM `keydown` capturing phase 统一处理；终端快捷键由 xterm.js + PTY 原生处理。**

- 应用快捷键：`window.addEventListener('keydown', handler, true)` → capturing phase 拦截 → 匹配后 `preventDefault` + `stopPropagation`
- 终端快捷键：xterm.js 通过 `onData` 发送到 PTY，由 Claude CLI 处理
- 终端视图可见性检查：`document.querySelector('[data-terminal-view]').checkVisibility()`

### 应用级快捷键

| 快捷键 | 功能 |
|--------|------|
| Ctrl+, | 打开设置 |
| Ctrl+Shift+N | 新建应用实例 |
| Ctrl+Shift+←/→ | 窗口左移/右移半屏 |
| Ctrl+Shift+R | 重启应用 |
| Ctrl+Shift+H | 回到项目列表 |
| Ctrl+=/- | 增大/减小字体 |
| Ctrl+0 | 重置字体 |
| Alt+N | 新建会话（终端可见时） |
| Alt+R | 重启会话（终端可见时） |
| Alt+↑/↓ | 切换标签（终端可见时） |

详细快捷键架构 → [docs/interaction.md](interaction.md)

## 进程生命周期

```
启动 → 环境检查 → PTY spawn → Claude CLI 运行
                              ↓
                       用户交互（双向数据流）
                              ↓
关闭窗口 → kill_all() → PTY 进程清理 → Claude CLI 退出
```
