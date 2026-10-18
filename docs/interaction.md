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

打开的 Claude/Codex `resume-id` 标签若保留了精确来源键，更名菜单、保存按钮与编辑框说明明确指向“恢复来源会话”，说明中显示来源 ID。新建 Claude 已获得可靠原生 ID 时可明确更名其“历史会话记录”，同样显示记录 ID。终端内 `/new` 或切换会话不改变恢复来源目标。保存前重新核对冻结根与最新 profile revision、项目、原生 ID 和 key，再通过来源的 profile 授权写入；不发送终端按键。raw/picker 和未获可靠 ID 的新建会话不猜目标，原生失败不另存显示别名。Claude 的 Node ≥22.13、文件系统或 helper 条件不满足时给出固定安全错误；不自动安装运行环境。

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

Native Ctrl+V 在读取剪贴板前同步向当前 binding 预留输入 intent，并冻结当前 attempt 与 bracketed-paste mode；随后 Enter 等用户输入排在该 intent 之后。异步结果只可完成原预留；空、不可读或 restart 后变旧的结果仅取消尚未提交的原预留，让已排队输入按既有序号继续，不能借用可变 `binding` 写入新 run。producer 已就绪或传输开始后不可取消，真实 producer/传输失败仍保持 partial/unknown 冻结与诊断。文本为空时的图片探测只确认资源存在，始终关闭 Tauri `Image`，不读取或记录像素。

### JSON 粘贴压缩（已废弃）

`compactJsonForPaste` 曾在 Claude Code 的 bracketed paste 标记被 ConPTY 吞掉后，把合法多行 JSON 压缩为单行以降低 burst 丢失率（上游 [claude-code#49673](https://github.com/anthropics/claude-code/issues/49673)、[#49337](https://github.com/anthropics/claude-code/issues/49337)，官方关闭不修）。**已废弃不再使用**：治标且大段粘贴仍可能截尾（上游未修，带内无 100% 可靠方案）。`src/utils/pasteText.ts` 保留导出仅供历史回归。大段内容建议让 CLI 读文件（`@路径`）或用 CLI 的 `Ctrl+G` 外部编辑器。

### codex Ctrl+V 原生粘贴转发

codex 的 Ctrl+V 能力分版本：**≥0.163.0** 在 Windows 上对纯 Ctrl+V 构造 `PasteImage { text_fallback }`（`chatwidget/interaction.rs`，先尝试图片、无图时回退由 codex TUI 自行读系统剪贴板文本：`clipboard_paste/text.rs` 用 arboard，`app/clipboard.rs:23-52` 处理回退）；**0.162.x** 的 `chatwidget/clipboard.rs` 只处理图片、无文本回退，纯文本按键会被吞掉。

因此 CC Desk 不能无条件转发：`cli_codex_native_paste_support`（`cli/paste_support.rs`）仅在接受探测的 Windows 上运行，按 profile 授权解析 `configured_program` 后运行 `codex --version`，输出必须携带 `codex-cli` 标识 token（裸 semver 如启动器 npx 打印的自身版本不算），版本 ≥0.163.0 才支持原生转发；探测有 5s 期限并回收挂起子进程，结果按可执行路径缓存进程生命周期，失败保守返回不支持。这是运行时能力查询，不属于 launch preflight（preflight 仍不 spawn CLI）。

Ctrl+V keydown 仍先**同步** `reserveUserPaste` 保持输入排序契约；producer 内等待探测结果：支持时返回 1 字节 `\x16`（Ctrl+V 控制字符），由 codex 原生读剪贴板处理文本和图片；不支持时回退 Desk 剪贴板读取路径（文本构造 bracketed frame、图片走键位分流）。

为什么转发：CC Desk 原路径（读剪贴板 IPC → 构造 76011 字节 bracketed frame → 分块 IPC 上传 → 一次 `write_all` 进 ConPTY）端到端约 150ms，但 codex 侧的 paste burst 启发式（`paste_burst.rs`，Windows 上无 bracketed paste 事件只能靠 `PASTE_BURST_CHAR_INTERVAL=8ms` 的字符间隔判定）在大 payload 时 burst 断裂，导致 codex **逐字插入**而非折叠——29198 字符的逐字插入是 O(n²) 的 textarea 编辑，耗时 3.7–10+ 秒且部分展开。转发 `\x16` 绕过整个问题链：codex 收到的是一次完整粘贴事件，自行读剪贴板、自行折叠。

约束与限制：仅适用于 codex（Claude Code 无等效的 Ctrl+V 剪贴板读取键位，仍走剪贴板插件读取路径）。转发分支由 codex 自行处理内容（无 LF 规范化、无 atomic write_all 保证）。codex 升级后需重启 CC Desk 才会重新探测（按路径缓存）。Claude Code 在 Windows 上仍有同样的 bracketed paste 限制（上游未修），大段粘贴建议用 `@路径` 或 `Ctrl+G`。

### 图片粘贴分流

剪贴板**无文本**时（`readText()` 返回空串或 reject——剪贴板只有截图时插件底层 arboard 返回错误，实际走 reject），`commitPaste` 经 `imageFallback` 向 PTY 转发 CLI 图片粘贴键字节，由 Claude CLI 自行读系统剪贴板、插入 `[Image #N]` 芯片。GUI 只探测并立即关闭图片资源，不读取图片像素：

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

会话尾部的关闭/时间槽与外置归档槽具有固定且不同的坐标；关闭后历史行或邻行顶上时，旧关闭坐标不能变成归档，也不触发历史恢复。归档仍可由独立外置按钮或共享菜单执行。首次空数据加载可显示加载文字，后台投影刷新不闪现该文字。

- **文本选择**：xterm.js 原生支持
- **链接点击**：WebLinksAddon 处理
- **复制粘贴**：Ctrl+C/V（需聚焦终端）
- **会话引用拖拽**：会话列表行拖入终端区域，向当前活跃会话注入单行引用线索文本（标题/CLI 类型/项目路径/会话 ID），同类型与跨 CLI 类型均可。载荷只含显示安全字段，transcript 路径绝不离开后端；重命名编辑中禁用拖拽。注入走对应 runtime 的用户写入通道（Native `sendUserText` / Legacy PTY 队列），不提交回车。应用内 DOM 拖拽与 OS 文件拖放（`onDragDropEvent`）是独立通道。曾实验"读取源会话开头消息生成摘要块"，实测真实会话全部超出 Messages 读取预算（SOURCE_TOO_LARGE）后回退；恢复该档位需后端提供 tail/prefix 消息读取。
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

## Structured restart continuity

An already-started Native new or resume-id tab restarts only after checking its frozen run source, a fresh positive history identity and the exact current owner. The root/profile/project are revalidated around stopping; a known conversation restarts with resume-id and its proven source key. Unknown identity, changed source or stale ownership fails explicitly instead of silently starting a new conversation. A positively unstarted attempt remains a launch retry; raw actions remain explicit reruns.
