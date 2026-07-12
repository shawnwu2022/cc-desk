# 组件结构

## 组件树

```
App.vue
├── TitleBar.vue                    # 自定义标题栏（Windows 去除原生装饰）
├── SettingsOverlay.vue             # 全局设置浮层
├── TerminalView.vue                # 终端主视图（常驻 DOM，v-show 控制）
│   ├── IconBar.vue                 # 左侧图标栏（面板切换入口）
│   ├── SidebarPanel.vue            # 侧边栏面板容器
│   │   ├── SessionsPanel.vue       # 会话管理面板（组装 ProjectNode 全局树 + 搜索 + 空状态 + 孤儿分组）
│   │   │   ├── ProjectNode.vue     # 项目节点（图标+名+状态徽标 ●N/琥珀点 + ▸展开 + hover 新建/菜单）
│   │   │   ├── SessionList.vue
│   │   │   └── SessionItem.vue + SessionStatus.vue
│   │   ├── SkillsPanel.vue         # Skills 面板
│   │   ├── AgentsPanel.vue         # Agents 面板
│   │   ├── McpPanel.vue            # MCP Servers 面板
│   │   └── PluginsPanel.vue        # Plugins 面板
│   ├── TerminalHeader.vue          # 终端标题栏（项目名 + 返回按钮）
│   └── XTermTerminal.vue           # xterm.js 终端核心
│
├── WelcomeView.vue                 # 欢迎引导页（覆盖层，无收藏项目时）
└── ProjectSelectView.vue           # 项目选择页（覆盖层，有收藏项目时）
```

## 组件详情

### App.vue — 视图切换 + 环境检查

- 管理三个视图：`welcome` / `projects` / `terminal`
- TerminalView 使用 `v-show` 常驻 DOM（保持 PTY 和终端实例不销毁）
- WelcomeView/ProjectSelectView 使用 `v-if` 覆盖层叠加在终端之上
- 环境检查失败时显示全屏遮罩
- 初始化：hook store、快捷键监听、自动更新检查

### XTermTerminal.vue — 终端核心

职责：
- 管理多个终端实例（Map<tabId, TerminalInstance>）
- 加载 FitAddon、SearchAddon、WebLinksAddon、SerializeAddon
- 双向数据绑定：onData → ptyInput，onPtyOutput → term.write
- Tab 创建/切换/重启/关闭
- Ctrl+V 粘贴处理
- 会话匹配轮询（通过 sessionStore）
- 终端主题配色：xterm theme 由 `appStore.terminalTheme` 驱动（`getTerminalTheme`），与 GUI 浅/暗独立；watch 联动所有 tab；容器背景/滚动条用 `--terminal-surface-bg`/`--terminal-scrollbar`（继承自 TerminalView）

Props：
- `fontSize: number` — 终端字号

Events：
- `ptyStarted(tabId, ptyId)` — PTY 启动成功

### TerminalView.vue — 终端主视图

> 终端容器表面色：根节点（`.terminal-view`）由 `computeTerminalSurfaceVars` 设置局部 CSS 变量 `--terminal-surface-bg`/`--terminal-scrollbar`（随 `appStore.terminalTheme` 变化），向下继承给 `.terminal-container`、`.xterm-container`、滚动条、空态。


布局：
```
┌────┬──────────────┬───────────────────────────┐
│Icon│  SidebarPanel │  TerminalHeader (38px)     │
│Bar │  (sessions/   ├───────────────────────────┤
│(40px│  skills/     │                           │
│    │  agents/      │  XTermTerminal (flex:1)   │
│    │  mcp/         │                           │
│    │  plugins)     │                           │
└────┴──────────────┴───────────────────────────┘
```

职责：
- 组合 IconBar、SidebarPanel、TerminalHeader、XTermTerminal
- 管理会话操作（新建/切换/重命名/恢复/关闭）
- 监听 cwd 变化，加载项目配置和历史会话
- 初始化 `useWindowAttention`（窗口聚焦状态）和 `useStatusMonitor`（hook 事件→Tab 状态）

### IconBar.vue — 左侧图标栏

固定宽度 40px，提供面板切换入口：
- Sessions、Skills、Agents、MCP、Plugins 图标按钮
- Settings 按钮
- Open Folder 按钮

### SidebarPanel.vue — 侧边栏面板

根据 `sidebarStore.activePanel` 显示对应面板内容，支持 `v-show` 切换（保留各面板状态）。

### TitleBar.vue — 自定义标题栏

Windows 平台去除原生装饰后自定义的拖拽区域 + 窗口控制按钮。

## Composables

### useProjectTreeNavigation — 切换语义纯函数

`resolveSwitchAction(input)` 是树形项目会话管理的决策核心（对抗审查 D/E 的可测单元）。纯函数、无副作用、不读写全局单值中间态；输入全部显式参数直传，连续调用互不影响，避免竞态。

输入 `SwitchInput`：`projectPath` / `sessionId?`（点项目名时不给）/ `isCurrent`（是否当前 cwd）/ `tabs` / `history` / `activeTabId`。

输出 `SwitchAction`：
- `noop` — 当前项目且已有 active tab，不打断
- `activate` — 点具体会话且对应 tab 存在，或点项目名且最近活跃 tab 为 running/stopped → 切到该 tab
- `resume` — 点历史会话无对应 tab，或点项目名且历史非空 → `--resume` 该会话
- `new` — 无 tab 无历史 → 新建会话

`TerminalView.vue` 的 `handleSwitchToProjectSession` 等 handler 消费该结果，复用 `startResumeSession` 完成「切 cwd + 切 tab / --resume」。`SidebarPanel.vue` re-emit 新事件透传。

### 已知限制（§5.2 follow-up）

每项目历史分页（5 条 + 显示更多懒加载）尚未实现，历史全量展示——重度多项目用户渲染性能待优化（follow-up，按实际体验定优先级）。

## Store 结构

### app.ts — 应用状态

```typescript
cwd: string                           // 当前工作目录
theme: string                         // 主题
fontSize: number                      // 终端字号
pendingResume: PendingResume | null   // 待恢复会话信息
checkResults: CheckResult[]           // 环境检查结果
cachedProjects: Project[]             // 项目列表缓存（分页）
cachedRecentSessions: SessionInfo[]   // 近期会话缓存
defaultClaudeOptions: DefaultClaudeOptions  // 持久化默认启动参数
claudeOptions: ClaudeOptions          // 当前启动参数
```

方法：loadAppConfig、runChecks、loadCache、loadMoreProjects、setCwd、setFontSize、getClaudeArgs 等

### session.ts — 会话管理

```typescript
// Tab 数据模型（跨越 PTY 生命周期的稳定 UI 单元）
interface TerminalTab {
  tabId: string              // 稳定 ID
  projectPath: string
  ptyId: string | null       // PTY 进程 ID（停止时 null）
  sessionId: string | null   // Claude session ID（匹配后赋值）
  name: string
  status: 'starting' | 'running' | 'stopped'
  createdAt: number
  lastActiveAt: number
  working: boolean           // 正在工作中（用户发消息后、响应返回前）
  pending: boolean           // 需要用户关注（响应完成但用户未看到）
  model?: string             // 模型名
}

tabs: Map<string, TerminalTab>        // 所有 Tab
activeTabId: string | null            // 当前活跃 Tab
historySessions: HistorySession[]     // 未被 Tab 占用的历史会话
```

方法：createTab、setTabPty、handlePtyExit、closeTab、assignSessionIdByPtyId

**全局项目树相关**（Sessions 面板从扁平列表升级为项目→会话全局树）：
- `buildProjectGroups`：按项目路径分组 tabs + 历史，无 tab/历史的孤儿项目单独收集
- `sortProjectGroups(groups, currentCwd)`：排序——当前项目置顶 → 有活跃会话 → 最近时间 → 孤儿项目置底
- `filterProjectGroups(groups, query)`：搜索——匹配项目名 + 已加载历史会话名（`getHistoryFor`）+ 该组 tabs 的 name/sessionId
- `getHistoryFor(projectPath)`：多项目历史选择器，按项目路径隔离历史，跨项目切换不串扰
- `expandOverride` / `toggleExpand(path, opts)` / `isExpanded(path, opts)`：展开状态，默认当前项目 + 有活跃会话项目展开，其余折叠；`opts.hasActive`/`opts.isCurrent` 决定默认值

### sidebar.ts — 侧边栏状态

```typescript
activePanel: SidebarPanelType  // 'sessions' | 'skills' | 'agents' | 'mcp' | 'plugins' | null
panelVisible: boolean
showSettings: boolean
// 预加载数据
skills: SkillInfo[]
agents: AgentInfo[]
mcpServers: McpServerInfo[]
plugins: PluginInfo[]
updateInfo: UpdateInfo | null
```

方法：togglePanel、loadAllSidebarData、openSettings

### config.ts — 项目配置

```typescript
projectConfig: ProjectConfigResult | null  // 当前项目 Claude 配置（只读展示）
```

方法：loadProjectConfig（带缓存）

### hook.ts — Hook 事件总线

纯事件总线，不包含业务逻辑。模块通过 `subscribe(eventTypes[], handler)` 注册，`init()` 时开始监听 Rust 后端 emit 的 hook-event 并 dispatch。

```typescript
subscribe(eventTypes: string[], handler: (payload) => void): () => void
dispatch(payload: HookEventPayload): void
init(): void
clearSession(key: string): void
```

## 色彩系统

主色调：**墨蓝 + 琥珀金**，温暖米灰基底。

```css
/* GUI 层 */
--bg-primary: #faf9f6;        /* 温暖米灰 */
--accent-primary: #1e3a5f;    /* 深邃墨蓝 */
--accent-gold: #d4a574;       /* 琥珀金 */

/* 状态语义色 */
--status-success: #3d8c6e;    /* 墨绿 */
--status-info: #2a5082;       /* 墨蓝 */
--status-warning: #c4964a;    /* 琥珀 */
--status-error: #c45c4a;      /* 赭红 */

/* 终端层 */
--terminal-bg: #f8f9fa;       /* 浅灰背景 */
--terminal-fg: #1a1816;       /* 深炭灰文字 */
```
