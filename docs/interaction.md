# 交互规则

## 核心原则

**终端输入直接发送到 PTY，由 Claude CLI 处理；应用级快捷键由 `useAppShortcuts.ts` 通过 DOM `keydown` capturing phase 统一处理。**

```
用户按键 → window keydown (capturing phase)
                ↓
       匹配应用快捷键？
       ├─ 是 → preventDefault + stopPropagation → 执行应用操作
       └─ 否 → 正常传递到 xterm.js.onData → PTY → Claude CLI
```

## 快捷键处理架构

### 架构：DOM capturing phase → useAppShortcuts.ts

所有应用快捷键在 `src/composables/useAppShortcuts.ts` 中注册，通过 `window.addEventListener('keydown', handler, true)` 在 capturing phase 拦截：

```typescript
// useAppShortcuts.ts
function handleGlobalKeydown(e: KeyboardEvent) {
  const mod = e.ctrlKey
  // 匹配 → e.preventDefault() + e.stopPropagation()
  // 不匹配 → 传递到 xterm.js → PTY → Claude CLI
}

function setupShortcutListeners(): (() => void)[] {
  window.addEventListener('keydown', handleGlobalKeydown, true)
  return [() => window.removeEventListener('keydown', handleGlobalKeydown, true)]
}
```

### 终端视图可见性检查

部分快捷键（Alt+N/R、Alt+↑↓、Ctrl+Shift+H）仅在终端视图可见时生效：

```typescript
function isTerminalVisible(): boolean {
  const terminalView = document.querySelector('[data-terminal-view]')
  return terminalView !== null && terminalView.checkVisibility()
}
```

### 三种输入场景

| 场景 | 焦点位置 | 应用快捷键 | 终端输入 |
|------|---------|-----------|----------|
| **1. 终端聚焦** | xterm.js | DOM capturing → 执行 | xterm.js → PTY |
| **2. 标题栏点击** | 窗口框架 | DOM capturing → 执行（正常工作） | 无法输入 |
| **3. 窗口不在前台** | 其他应用 | OS 不派发 keydown → 不触发 | 正常 |

## 应用级快捷键

所有应用级快捷键在 `src/composables/useAppShortcuts.ts` 中定义：

| 快捷键 | 功能 | 作用域 |
|--------|------|--------|
| Ctrl+, | 打开设置 | 全局 |
| Ctrl+Shift+N | 新建应用实例 | 全局 |
| Ctrl+Shift+← | 窗口左移半屏 | 全局 |
| Ctrl+Shift+→ | 窗口右移半屏 | 全局 |
| Ctrl+Shift+R | 重启应用 | 全局 |
| Ctrl+Shift+H | 回到项目列表 | 全局 |
| Ctrl+= | 增大字体 | 全局 |
| Ctrl+- | 缩小字体 | 全局 |
| Ctrl+0 | 重置字体 | 全局 |
| Alt+N | 新建会话 | 终端可见时 |
| Alt+R | 重启会话 | 终端可见时 |
| Alt+↑ | 上一个标签 | 终端可见时 |
| Alt+↓ | 下一个标签 | 终端可见时 |

**Ctrl++ 特殊处理**：物理键盘上 `Ctrl++` 实际是 `Ctrl+Shift+=`，代码中 `e.key === '='` 匹配两种情况。

## 终端快捷键（Claude CLI 处理）

终端内的快捷键由 xterm.js 原生处理，通过 `onData` 发送到 PTY：

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

### Ctrl+W 处理

Tauri 无特殊绑定，xterm.js 原生处理：用户按 Ctrl+W → `term.onData('\x17')` → PTY → Claude CLI readline 删除前一个单词。无需额外代码。

### Ctrl+V 粘贴处理

```typescript
// src/components/XTermTerminal.vue
term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
  if (event.type !== 'keydown') return true

  // Cmd+C (macOS) 复制选中内容
  if (event.metaKey && !event.ctrlKey && event.key === 'c') {
    const selection = term.getSelection()
    if (selection) {
      event.preventDefault()
      writeText(selection).catch(() => {})
      return false
    }
    return true
  }

  // Ctrl+C 复制（有选中）或 SIGINT（无选中）
  if (event.ctrlKey && !event.metaKey && event.key === 'c' && !event.shiftKey) {
    const selection = term.getSelection()
    if (selection) {
      event.preventDefault()
      writeText(selection).catch(() => {})
      return false
    }
    return true
  }

  // Ctrl+Shift+C 强制复制
  if (event.ctrlKey && event.shiftKey && event.key === 'C') {
    event.preventDefault()
    const selection = term.getSelection()
    if (selection) {
      writeText(selection).catch(() => {})
    }
    return false
  }

  // Ctrl+V / Cmd+V 粘贴
  if ((event.ctrlKey || event.metaKey) && event.key === 'v') {
    event.preventDefault()
    // 不走 term.paste：xterm 会把 \r?\n 转成 \r（回车），在 Claude 的 Ink TUI 里
    // 触发光标回行首、后续覆盖前面。commitPaste 走完整流程：同步捕获 ptyId →
    // readText() → isPasteStale 复核（防 restart 重建后把旧粘贴写到新 PTY）→
    // 构造 payload（规范化 LF + bracketed 包装）→ 写 PTY。
    // 剪贴板无文本（截图场景 readText reject）时经 imageFallback 转发 CLI 图片
    // 粘贴键字节，由 CLI 自行读剪贴板插 [Image #N]。
    // 依赖注入，便于测试"重启重建后不写新 PTY"的竞态行为。
    commitPaste(
      readText,
      () => terminalInstances.get(tabId),
      text => buildPastePayload(text, term.modes.bracketedPasteMode, term.options.ignoreBracketedPasteMode ?? false),
      ptyInput,
      () => imagePasteBytes(platform),
    ).catch(() => {})
    return false
  }

  // Shift+Enter => 插入换行（模拟 \ + Enter）
  if (event.shiftKey && event.key === 'Enter') {
    event.preventDefault()
    const instance = terminalInstances.get(tabId)
    if (instance) {
      ptyInput(instance.ptyId, '\\\r')
    }
    return false
  }

  return true
})
```

`commitPaste` 的核心竞态守卫：`readText()` 是异步的，等待期间 restartTab 可能重建同 tabId 的新 PTY；实现先同步捕获按键瞬间的 ptyId，完成后复核当前实例仍是同一 ptyId（`isPasteStale`），否则丢弃过期粘贴，避免旧 bracketed 模式落到新实例。详见 `src/utils/pasteText.ts`。

### 图片粘贴分流

剪贴板**无文本**时（`readText()` 返回空串或 reject——剪贴板只有截图时插件底层 arboard 返回错误，实际走 reject），`commitPaste` 经 `imageFallback` 向 PTY 转发 CLI 图片粘贴键字节，由 Claude CLI 自行读系统剪贴板、插入 `[Image #N]` 芯片，GUI 全程不接触图片数据：

| 平台 | 转发字节 | 对应键位（`chat:imagePaste` 官方默认） |
|---|---|---|
| Windows | `\x1bv` | `Alt+V`（Windows/WSL 专用绑定） |
| macOS / Linux | `\x16` | `Ctrl+V` |

- `Alt+V` 未被应用拦截，经 xterm.js 编码 `\x1bv` 直传 PTY，与分流路径等价；macOS `Cmd+V` 已被 `metaKey` 条件拦截进 `commitPaste`，分流行为同 `Ctrl+V`。
- 文本优先：剪贴板同时有文本和图片时贴文本，与 CLI 原生 `Ctrl+V`/`Alt+V` 职责分离一致。
- **键位契约**：分流按 CLI **默认键位**硬编码，不解析 `~/.claude/keybindings.json`（该文件可重绑/解绑 `chat:imagePaste` 且热加载）。用户重绑后分流可能失效或触发重绑后的其他动作——此为已知限制，原生键盘路径不受影响。
- 语义为 best effort：GUI 读文本判定与 CLI 读图是两次独立剪贴板访问，不保证同一快照。

### 中文 IME Shift 切换中英文（搜狗等）

搜狗等中文输入法用 Shift 切换中英文时，把已输入的拼音作为字母通过 `input` 事件（`inputType=insertText`、`composed=true`）提交到 textarea。xterm.js 的 `_inputEvent`（`node_modules/@xterm/xterm/src/browser/Terminal.ts`）发送条件为 `(!ev.composed || !this._keyDownSeen)`——Shift 的 keydown 已把 `_keyDownSeen` 置 true，于是 `composed=true && _keyDownSeen=true` 时整条 input 被 xterm 丢弃，已输入的字符不进 PTY。

修复（`attachImeInputFix`，在 `term.open` 后绑定）：应用侧镜像 xterm 的 `_keyDownSeen`（keydown 置 true、keyup 置 false），只在精确漏发分支（`composed=true && keyDownSeen=true`）补发 `term.input(data)`，并排除走了真实 composition 生命周期的输入（微软拼音等由 xterm 原生 composition 路径处理）。

```typescript
// src/components/XTermTerminal.vue
const onInput = (e: Event) => {
  const ie = e as InputEvent
  if (ie.inputType === 'insertText' && ie.composed && ie.data && state.keyDownSeen && !state.compositionSeen) {
    term.input(ie.data)
  }
}
```

不与 xterm 重复：xterm 自己发送的 composed insertText 必然是 `_keyDownSeen=false`，而本监听器要求镜像的 `keyDownSeen=true`，两者互斥。注意不能靠 `stopPropagation` 区分——xterm 的 `cancel()` 默认无效（`cancelEvents=false`），既不 preventDefault 也不 stopPropagation。详见 [docs/manual-test-cases.md](manual-test-cases.md) 的「终端输入（IME）」条目。

## 视图切换

| 场景 | 触发 |
|------|------|
| 启动无收藏 | → WelcomeView |
| 启动有收藏 | → ProjectSelectView |
| 选择项目 | → TerminalView |
| 点击返回 | → ProjectSelectView |
| Ctrl+Shift+H | 终端 ↔ 项目列表 |

## 鼠标交互

- **文本选择**：xterm.js 原生支持
- **链接点击**：WebLinksAddon 处理
- **复制粘贴**：Ctrl+C/V（需聚焦终端）
- **侧边栏**：点击外部区域关闭侧边栏

## GUI 增强边界

| 增强 | 做 | 不做 |
|------|----|------|
| 终端主题 | 浅色主题 + CSS 变量 | AI 补全 |
| 多终端 | 标签切换 + 状态指示灯 | 复杂布局 |
| 会话管理 | 创建/切换/重命名/恢复 | 导出文件 |
| 信息面板 | MCP/Skills/Agents/Plugins 只读展示 | 编辑配置 |
| 搜索 | SearchAddon | 高级过滤 |
| 自动更新 | GitHub Releases 检测 + 下载安装 | 后台静默更新 |
