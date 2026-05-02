# 交互规则

## 核心原则

**终端输入直接发送到 PTY，由 Claude CLI 处理；应用级快捷键由 Rust 端 `tauri-plugin-global-shortcut` 在 OS 层面拦截，通过事件传递到前端处理。**

```
用户按键 → OS 层 RegisterHotKey 拦截
                ↓
       匹配应用快捷键？
       ├─ 是 → app.emit("shortcut:*") → 前端 listen → 执行应用操作
       └─ 否 → 正常传递到 webview → xterm.js.onData → PTY → Claude CLI
```

## 快捷键处理架构

### 架构：Rust global-shortcut → emit → 前端 listen

快捷键在 OS 层面通过 `tauri-plugin-global-shortcut`（底层使用 Windows `RegisterHotKey` / macOS Carbon / Linux X11）注册，不依赖 webview 焦点。

```
Rust 端（lib.rs setup）：
  1. Builder::new()
     .with_shortcuts(["CmdOrCtrl+Comma", ...])
     .with_handler(|app, shortcut, event| {
         // 窗口焦点检查（AtomicBool，由 on_window_event 追踪）
         // match (mods, key) → app.emit("shortcut:xxx", ())
     })
     .build()

前端（useAppShortcuts.ts）：
  setupShortcutListeners() → listen("shortcut:xxx", callback)
```

### 窗口焦点追踪

`RegisterHotKey` 在 OS 层拦截按键，即使应用不在前台也会消费按键。通过追踪原生窗口焦点状态来限制：

```rust
// lib.rs
static WINDOW_FOCUSED: AtomicBool = AtomicBool::new(true);

.on_window_event(|_window, event| {
    if let tauri::WindowEvent::Focused(focused) = event {
        WINDOW_FOCUSED.store(*focused, Ordering::SeqCst);
    }
})

// handler 中
if !WINDOW_FOCUSED.load(Ordering::SeqCst) { return; }
```

**注意**：`WebviewWindow::is_focused()` 检查的是 webview 内部焦点，不适用于此场景（点击标题栏时返回 false）。必须使用 `on_window_event(Focused)` 追踪原生窗口焦点。

### 跨平台修饰键

使用 `CmdOrCtrl` 修饰键实现跨平台适配：
- macOS → `SUPER`（⌘ Command）
- Windows/Linux → `CONTROL`（Ctrl）

### 三种输入场景

| 场景 | 焦点位置 | 应用快捷键 | 终端输入 |
|------|---------|-----------|----------|
| **1. 终端聚焦** | xterm.js | OS 层拦截 → emit | xterm.js → PTY |
| **2. 标题栏点击** | 窗口框架 | OS 层拦截 → emit（正常工作） | 无法输入 |
| **3. 窗口不在前台** | 其他应用 | handler 检查焦点，跳过 | 正常 |

## 应用级快捷键

所有应用级快捷键在 Rust 端注册（`src-tauri/src/lib.rs`），前端通过事件监听处理（`src/composables/useAppShortcuts.ts`）：

| 快捷键 | 事件名 | 功能 | 跨平台 |
|--------|--------|------|--------|
| Ctrl+, (⌘,) | `shortcut:toggle-settings` | 打开设置 | CmdOrCtrl |
| Ctrl+Shift+N (⌘⇧N) | `shortcut:new-instance` | 新建应用实例 | CmdOrCtrl |
| Ctrl+Shift+← (⌘⇧←) | `shortcut:snap-left` | 窗口左移半屏 | CmdOrCtrl |
| Ctrl+Shift+→ (⌘⇧→) | `shortcut:snap-right` | 窗口右移半屏 | CmdOrCtrl |
| Ctrl+Shift+R (⌘⇧R) | `shortcut:restart-app` | 重启应用 | CmdOrCtrl |
| Ctrl+Shift+H (⌘⇧H) | `shortcut:back-to-projects` | 回到项目列表 | CmdOrCtrl |
| Ctrl+= (⌘=) | `shortcut:font-increase` | 增大字体 | CmdOrCtrl |
| Ctrl++ (⌘⇧=) | `shortcut:font-increase` | 增大字体 | CmdOrCtrl+Shift |
| Ctrl+- (⌘-) | `shortcut:font-decrease` | 缩小字体 | CmdOrCtrl |
| Ctrl+0 (⌘0) | `shortcut:font-reset` | 重置字体 | CmdOrCtrl |
| Alt+N | `shortcut:new-session` | 新建会话（终端可见时） | 各平台一致 |
| Alt+R | `shortcut:restart-session` | 重启会话（终端可见时） | 各平台一致 |
| Alt+↑ | `shortcut:tab-prev` | 上一个标签（终端可见时） | 各平台一致 |
| Alt+↓ | `shortcut:tab-next` | 下一个标签（终端可见时） | 各平台一致 |

**终端视图可见性检查**：
部分快捷键（Alt+N/R、Alt+↑↓、Ctrl+Shift+H）仅在终端视图可见时生效，避免在 Welcome/Projects 视图中误触发。

**Ctrl++ 特殊处理**：
`Ctrl++` 在物理键盘上是 `Ctrl+Shift+=`，所以需要同时注册 `CmdOrCtrl+Equal` 和 `CmdOrCtrl+Shift+Equal`，两者触发相同事件。

## 终端快捷键（Claude CLI 处理）

终端内的快捷键由 xterm.js 原生处理，通过 `onData` 发送到 PTY：

### xterm.js 数据流

```typescript
// src/components/XTermTerminal.vue
term.onData(data => {
  const instance = terminalInstances.get(tabId)
  if (instance) {
    ptyInput(instance.ptyId, data)  // 发送到 PTY
  }
})
```

### Claude CLI 常用快捷键

| 快捷键 | 功能 | 由谁处理 |
|--------|------|----------|
| Ctrl+C | 取消输入/生成 | xterm.js → PTY → Claude CLI |
| Ctrl+D | 退出 Claude Code | xterm.js → PTY → Claude CLI |
| Ctrl+L | 清屏 | xterm.js → PTY → Claude CLI |
| Ctrl+R | 反向搜索历史 | xterm.js → PTY → Claude CLI |
| Ctrl+B | 后台运行任务 | xterm.js → PTY → Claude CLI |
| Ctrl+W | 删除前一个单词 | xterm.js → `\x17` → PTY |
| Alt+P | 切换模型 | xterm.js → PTY → Claude CLI |
| Alt+T | 扩展思考 | xterm.js → PTY → Claude CLI |
| Ctrl+A/E | 行首/行尾 | xterm.js → PTY → Claude CLI |
| Ctrl+K/U | 删除到行尾/行首 | xterm.js → PTY → Claude CLI |

### Ctrl+W 特殊处理

**Tauri vs Electron**：

| 框架 | Ctrl+W 行为 | 处理方式 |
|------|-------------|----------|
| Electron | 浏览器内核会拦截关闭标签页 | 需要 `before-input-event` 手动拦截 |
| Tauri | 无特殊绑定，正常传递到 webview | xterm.js 原生处理即可 |

**Tauri 中的处理流程**：
```
用户按 Ctrl+W
  ↓
xterm.js 内部处理
  ↓
term.onData('\x17')  // 自动转换为 ASCII 0x17
  ↓
ptyInput('\x17')
  ↓
PTY → Claude CLI
  ↓
readline 删除前一个单词
```

**无需额外代码** — xterm.js 会自动将 Ctrl+W 转换为 `\x17` 字符。

### Ctrl+V 粘贴处理

```typescript
// src/components/XTermTerminal.vue
term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
  if (event.ctrlKey && event.key === 'v') {
    event.preventDefault()
    readText().then(text => {
      if (text) term.paste(text)
    })
    return false  // 阻止 xterm.js 处理
  }
  return true  // 其他按键交给 xterm.js
})
```

## Slash 命令

在 Claude CLI 中输入 `/` 触发：

- `/clear` — 清除对话
- `/compact` — 压缩上下文
- `/help` — 显示帮助
- `/cost` — 显示费用
- `/model` — 切换模型
- `/config` — 打开配置
- `/init` — 初始化项目
- `/memory` — 内存管理

GUI 层不处理这些命令，完全由 Claude CLI 处理。

## Bash 模式

输入 `!` 开头进入 Bash 模式：

```
! npm test
! git status
```

输出添加到对话上下文，支持 Ctrl+B 后台运行。

## 多行输入

Claude CLI 支持：

| 方法 | 快捷键 |
|------|--------|
| 快速转义 | `\` + Enter |
| Alt 键 | Alt+Enter（需配置） |
| Shift+Enter | Shift+Enter（部分终端支持） |
| 控制序列 | Ctrl+J |

## 视图切换

| 场景 | 触发 |
|------|------|
| 启动无收藏 | → WelcomeView |
| 启动有收藏 | → ProjectSelectView |
| 选择项目 | → TerminalView |
| 点击返回 | → ProjectSelectView |
| Escape（终端） | Claude CLI 处理（不退出视图） |

## 鼠标交互

- **文本选择**：xterm.js 原生支持
- **链接点击**：WebLinksAddon 处理
- **复制粘贴**：
  - Ctrl+C/V（需聚焦终端）
  - 右键菜单（系统上下文菜单）
- **侧边栏**：点击外部区域关闭侧边栏

## 与 Electron 架构的差异

| 特性 | Electron | Tauri |
|------|----------|-------|
| 快捷键拦截 | `before-input-event` | Rust global-shortcut → emit → 前端 listen |
| Ctrl+W 处理 | 需手动拦截发送 | xterm.js 原生处理 |
| 焦点恢复 | webContents.focus | 不需要（OS 层拦截） |
| 事件系统 | ipcMain/ipcRenderer | invoke/listen |
| 进程通信 | webContents.send | emit |

## GUI 增强边界

| 增强 | 做 | 不做 |
|------|----|------|
| 终端主题 | 浅色 + 预设 | AI 补全 |
| 多终端 | 标签切换 | 复杂布局 |
| 会话历史 | SerializeAddon | 导出文件 |
| 项目收藏 | 快速切换 cwd | 拖拽排序 |
| 快捷命令 | 命令面板（Phase 3） | 拦截 Claude 命令 |
| 搜索 | SearchAddon | 高级过滤 |
