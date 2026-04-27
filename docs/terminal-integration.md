# 终端集成架构

## 架构概述

本项目使用 **xterm.js + node-pty** 直接运行 Claude CLI，实现完全的原生终端体验。

### 核心依赖

| 包名 | 版本 | 用途 |
|------|------|------|
| @xterm/xterm | ^5.5.0 | 终端 UI 渲染 |
| @xterm/addon-fit | ^0.11.0 | 自动适应容器大小 |
| @xterm/addon-search | ^0.16.0 | 终端内搜索 |
| @xterm/addon-web-links | ^0.12.0 | 链接点击 |
| @xterm/addon-serialize | ^0.14.0 | 会话序列化 |
| node-pty | ^1.1.0 | 伪终端进程管理 |

## 主进程 PTY 模块 (main/pty.ts)

### 核心功能

```typescript
// 检测 Git Bash 路径（Windows 必需）
export function detectGitBash(): string | null

// 启动 Claude CLI 进程
export function spawnClaude(
  cwd: string,
  cols: number,
  rows: number,
  args: string[]
): PtyInstance | null

// 启动普通 Shell
export function spawnTerminal(cwd, cols, rows): PtyInstance | null

// 写入数据到 PTY
export function writeToPty(id: string, data: string): boolean

// resize PTY
export function resizePty(id: string, cols: number, rows: number): boolean

// 终止 PTY
export function killPty(id: string): boolean
export function killAllPty(): void
```

### Git Bash 检测逻辑

Windows 上 Claude CLI 需要 Git Bash。检测顺序：

1. 环境变量 `CLAUDE_CODE_GIT_BASH_PATH`
2. 常见安装路径：
   - `D:\Program Files\Git\bin\bash.exe`
   - `C:\Program Files\Git\bin\bash.exe`
   - `C:\Program Files (x86)\Git\bin\bash.exe`
3. PATH 环境变量中的 Git bin 目录

检测到的路径会自动设置为 `CLAUDE_CODE_GIT_BASH_PATH` 环境变量，传递给 PTY 进程。

### Claude 进程启动

```typescript
// Windows
shell: 'cmd.exe'
shellArgs: ['/c', 'claude', ...args]
env: {
  ...process.env,
  CLAUDE_CODE_GIT_BASH_PATH: detectedPath, // 关键！
  TERM: 'xterm-256color'
}

// Unix
shell: '/bin/bash'
shellArgs: ['-l', '-c', `claude ${args.join(' ')}`]
```

### 数据流

```
PTY onData → mainWindow.webContents.send('pty:output', { id, data })
PTY onExit → mainWindow.webContents.send('pty:exit', { id, exitCode })
```

## IPC 通道 (main/ipc.ts)

### PTY 操作通道

| 通道 | 方向 | 参数 | 返回 |
|------|------|------|------|
| pty:spawn | renderer→main | { cwd, cols, rows, type, args } | { id, type, cwd } |
| pty:input | renderer→main | { id, data } | boolean |
| pty:resize | renderer→main | { id, cols, rows } | boolean |
| pty:kill | renderer→main | { id } | boolean |
| pty:list | renderer→main | - | string[] |
| pty:info | renderer→main | { id } | PtyInfo |
| pty:killAll | renderer→main | - | void |

### PTY 事件通道

| 通道 | 方向 | 数据 |
|------|------|------|
| pty:output | main→renderer | { id, data } |
| pty:exit | main→renderer | { id, exitCode, signal? } |
| terminal:keydown | main→renderer | { key, modifiers } |

### 其他通道

| 通道 | 用途 |
|------|------|
| dialog:selectDirectory | 目录选择对话框 |
| store:getFavorites | 获取收藏项目 |
| store:addFavorite | 添加收藏 |
| store:removeFavorite | 移除收藏 |
| shell:exec | 执行 shell 命令 |
| shell:listDir | 列出目录内容 |

## Preload API (preload/index.ts)

### PTY 操作

```typescript
interface PtySpawnOptions {
  cwd: string
  cols?: number
  rows?: number
  type: 'claude' | 'shell'
  args?: string[]
}

window.api.ptySpawn(options): Promise<PtyInfo | null>
window.api.ptyInput(id, data): Promise<boolean>
window.api.ptyResize(id, cols, rows): Promise<boolean>
window.api.ptyKill(id): Promise<boolean>
window.api.ptyList(): Promise<string[]>
window.api.ptyInfo(id): Promise<PtyInfo | null>
window.api.ptyKillAll(): Promise<void>
```

### PTY 事件监听

```typescript
window.api.onPtyOutput((event, { id, data }) => { ... })
window.api.onPtyExit((event, { id, exitCode }) => { ... })
window.api.onTerminalKeydown((event, { key, modifiers }) => { ... })
window.api.removeListener(channel)
```

## 渲染进程终端组件 (XTermTerminal.vue)

### xterm.js 配置

```typescript
const term = new Terminal({
  fontFamily: 'Cascadia Code, Fira Code, Consolas, monospace',
  fontSize: 14,
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
  if (ptyId.value) {
    window.api.ptyInput(ptyId.value, data)
  }
})

// PTY 输出 → Terminal
window.api.onPtyOutput((event, { id, data }) => {
  if (id === ptyId.value && term) {
    term.write(data)
  }
})

// resize 同步
term.onResize(({ cols, rows }) => {
  window.api.ptyResize(ptyId.value, cols, rows)
})
```

### 特殊快捷键处理

```typescript
// Ctrl+W 被 Electron 拦截，需要手动发送
window.api.onTerminalKeydown((event, { key, modifiers }) => {
  if (modifiers.includes('control') && key === 'w') {
    window.api.ptyInput(ptyId.value, '\x17') // Ctrl+W ASCII
  }
})
```

## 快捷键处理机制

### 原则

**所有 Claude Code 快捷键直接发送到 PTY，由 Claude CLI 处理。**

- xterm.js 的 `onData` 会发送所有按键（包括 Ctrl/Alt 组合）
- Claude CLI 有完整的 readline/keybindings 处理机制
- GUI 层不做任何快捷键映射或拦截

### Claude Code 常用快捷键

| 快捷键 | 功能 | 处理方 |
|--------|------|--------|
| Ctrl+C | 取消当前输入/生成 | Claude CLI |
| Ctrl+D | 退出 Claude Code | Claude CLI |
| Ctrl+L | 清屏 | Claude CLI |
| Ctrl+R | 反向搜索历史 | Claude CLI |
| Ctrl+B | 后台运行任务 | Claude CLI |
| Alt+P | 切换模型 | Claude CLI |
| Alt+T | 切换扩展思考 | Claude CLI |
| Ctrl+W | 后台运行（被拦截，手动发送） | PTY |

### Electron 快捷键处理

```typescript
// main/index.ts
mainWindow.webContents.on('before-input-event', (event, input) => {
  // Ctrl+W: 阻止关闭窗口，发送到终端
  if (hasControl && key === 'w') {
    event.preventDefault()
    mainWindow.webContents.send('terminal:keydown', { key, modifiers })
  }
  // 其他 Ctrl/Alt 组合：不拦截，让 xterm.js 处理
})
```

## 环境检查 (main/checks.ts)

### 检查项

```typescript
export function runAllChecks(): CheckResult[] {
  return [
    checkNodeVersion(),      // Node.js >= 18
    checkNodePty(),          // node-pty 模块可用
    checkClaudeCli(),        // claude 命令可用
    checkGitBash(),          // Git Bash 路径（Windows）
    checkShell(),            // 系统 Shell 可用
    checkClaudeConfigDir(),  // ~/.claude 目录
  ]
}
```

### 启动流程

```typescript
// main/index.ts
async function createWindow() {
  const canStart = await performStartupChecks()
  if (!canStart) {
    app.quit()
    return
  }
  // 创建窗口...
}
```

## 进程生命周期

```
启动 → 环境检查 → 窗口创建 → PTY spawn → Claude CLI 运行
                                              ↓
                                       用户交互（双向数据流）
                                              ↓
关闭窗口 → killAllPty() → PTY 进程清理 → Claude CLI 退出
```