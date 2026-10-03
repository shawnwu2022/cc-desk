# 交互规则

## 应用级快捷键与统一路由

`config/appShortcuts.ts` 定义五项可配置应用动作，`useAppShortcuts` 在 window 的捕获阶段读取同一份已保存绑定。匹配后阻止事件继续进入终端，由 App 发统一动作请求；未匹配的输入交给实际终端。

| 默认绑定 | 应用动作 |
|---|---|
| Mod+N | 当前项目新建会话；无项目时进入添加流程 |
| Mod+W | 请求关闭当前会话，运行态使用精确所有权确认 |
| Mod+P | 项目管理 |
| Mod+, | 设置 |
| F2 | 当前会话重命名 |

Mod 在 Windows/Linux 为 Ctrl，在 macOS 为 Cmd。设置中的快捷键编辑器支持取消绑定、冲突确认和恢复默认。普通编辑字段、IME、重复按键及共享模态框阻止全局捕获；终端 helper 输入遵循应用绑定。列表重命名使用同一绑定。旧兼容快捷键路由和独立标签切换已退役。

## 终端输入边界

Native Claude/Codex 输入经过 authenticated document bridge，Legacy Claude 输入保留自己的 PTY adapter。应用快捷键之外的 CLI 快捷键由对应 CLI 解释。后台终端只接收已识别的协议回复，用户输入仍受可见性和精确运行身份约束。下列旧 Claude 输入说明仅描述其终端路径，不能替代 Native 输入契约或真实 CLI 验收。

## 终端快捷键（Claude CLI 处理）

终端内的快捷键由 xterm.js 原生处理，通过 `onData` 发送到 PTY：

| 快捷键 | 功能 | 由谁处理 |
|--------|------|----------|
| Ctrl+C | 取消输入/生成 | xterm.js → PTY → Claude CLI |
| Ctrl+D | 退出 Claude Code | xterm.js → PTY → Claude CLI |
| Ctrl+L | 清屏 | xterm.js → PTY → Claude CLI |
| Ctrl+R | 反向搜索历史 | xterm.js → PTY → Claude CLI |
| Ctrl+B | 后台运行任务 | xterm.js → PTY → Claude CLI |
| Ctrl+W | 默认属于应用关闭会话；解除该绑定后才进入 CLI | 应用绑定优先 |
| Alt+P | 切换模型 | xterm.js → PTY → Claude CLI |
| Alt+T | 扩展思考 | xterm.js → PTY → Claude CLI |
| Ctrl+A/E | 行首/行尾 | xterm.js → PTY → Claude CLI |
| Ctrl+K/U | 删除到行尾/行首 | xterm.js → PTY → Claude CLI |

### Ctrl+W 处理

默认 Mod+W 由应用请求关闭当前会话；运行中的会话先进入统一确认。只有用户取消或改设此应用绑定后，未匹配的 Ctrl+W 才继续交给终端/CLI。

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

### JSON 粘贴压缩

`buildPastePayload` 在规范化 LF 之前先经 `compactJsonForPaste`：剪贴板文本是**合法多行 JSON** 时压缩成单行，非 JSON 原样保留。

为什么需要：Windows ConPTY 输入解析器会吞掉 `ESC[200~`/`ESC[201~` bracketed paste 标记（CSI 序列不透传子进程），Claude Code 只能靠 burst 启发式识别粘贴；大段多行无标记 burst 识别会间歇性失败，按逐键处理时输入编辑器静默丢弃头部或尾部（上游 [claude-code#49673](https://github.com/anthropics/claude-code/issues/49673)、[#49337](https://github.com/anthropics/claude-code/issues/49337)，官方关闭不修）。实测单行 burst 丢失率显著更低，且小中型 JSON 压缩后整体可靠。

无损性：合法 JSON 的裸换行只会出现在 token 之间（字符串内部换行必须是 `\n` 转义），移除「换行 + 后续缩进空白」不影响任何值；JSON.parse 仅做校验，刻意不用 parse+stringify 往返——超出 2^53 的整数 ID 会被静默取整。

残余风险：压缩后仍超约 4KB 的大段粘贴在 Claude 识别失败时仍可能截尾（上游未修，带内无 100% 可靠方案）；超大内容建议让 Claude 读文件（`@路径`）或用 CLI 的 `Ctrl+G` 外部编辑器。src-tauri/tests/paste_claude_e2e.rs（`#[ignore]`，真实 claude CLI 端到端）可人工监测上游行为变化。

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

AppShell 是所有构建的唯一外壳，一级入口为工作区、项目、设置。启动目的地遵循保存的界面偏好，不启动 CLI。项目与会话选择只更新统一上下文；工作区及其终端宿主在跨页导航时保持挂载。空状态在 Workspace/Projects 内展示，没有独立欢迎页或 Native 页面。

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
