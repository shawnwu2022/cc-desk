# 布局与功能设计

## 设计理念

**浅色主题 + 深色终端。GUI 简洁美观，终端保持可读性。**

- 窗口进入即项目选择，符合用户第一步操作
- 进入项目后全屏终端，最大化内容面积
- 侧边栏面板按需展开，不打扰核心流程

## 窗口布局

### 欢迎引导视图（WelcomeView）

首次使用或无收藏项目时显示，居中 Logo + "Select Project Directory" 按钮。

### 项目选择视图（ProjectSelectView）

有收藏项目时显示：项目卡片列表 + 近期会话列表 + "Add new project" 按钮。

### 终端视图（TerminalView）

进入项目后常驻 DOM（v-show 控制）。

```
┌────┬──────────────┬───────────────────────────────┐
│    │              │  ← my-project            ⚙    │  Header (38px)
│ I  │  SidebarPanel├───────────────────────────────┤
│ c  │              │                               │
│ o  │  sessions/   │                               │
│ n  │  skills/     │     XTermTerminal             │
│ B  │  agents/     │     (浅灰背景 #f8f9fa)        │
│ a  │  mcp/        │                               │
│ r  │  plugins/    │     > _                       │
│    │              │                               │
│40px│  260px       │     flex:1                     │
└────┴──────────────┴───────────────────────────────┘
```

布局结构：
- **IconBar**（40px）：左侧图标栏，面板切换入口
- **SidebarPanel**（260px）：按需展开的侧边栏面板
- **TerminalHeader**（38px）：项目名 + 返回按钮
- **XTermTerminal**（flex:1）：终端区域

Windows 自定义标题栏（TitleBar）位于最顶部（32px）。

## 布局规格

| 区域 | 尺寸 | 说明 |
|------|------|------|
| TitleBar | 32px | 自定义标题栏（仅 Windows） |
| IconBar | 40px | 左侧图标栏 |
| SidebarPanel | 260px | 侧边栏面板宽度 |
| TerminalHeader | 38px | 终端标题栏 |
| XTermTerminal | flex:1 | 自动填充 |

## 色彩体系

主色调：**墨蓝 (#1e3a5f) + 琥珀金 (#d4a574)**，温暖米灰基底 (#faf9f6)。

| 层级 | 用途 | 关键变量 |
|------|------|----------|
| GUI 背景 | 主/次/第三层 | `--bg-primary: #faf9f6` / `--bg-secondary: #f5f3ee` / `--bg-tertiary: #ebe8e0` |
| GUI 强调 | 墨蓝 + 琥珀金 | `--accent-primary: #1e3a5f` / `--accent-gold: #d4a574` |
| GUI 文字 | 深炭灰层级 | `--text-primary: #1a1816` / `--text-secondary: #5a5550` / `--text-tertiary: #8a8680` |
| 终端 | 浅灰背景 + 深文字 | `--terminal-bg: #f8f9fa` / `--terminal-fg: #1a1816` |
| 状态 | 成功/信息/警告/错误 | `--status-success: #3d8c6e` / `--status-info: #2a5082` / `--status-warning: #c4964a` / `--status-error: #c45c4a` |

### 终端主题（独立于 GUI 浅/暗）

终端区域支持独立配色方案，内置 16 个预设（CC-Box 浅/暗 + Dracula / Gruvbox / Nord 等程序员主题）。由 `src/config/terminalThemes.ts` 定义；`appStore.terminalTheme` 驱动 xterm 字符栅格；`computeTerminalSurfaceVars` 产出局部 CSS 变量（`--terminal-surface-bg`/`--terminal-scrollbar`）驱动终端容器背景、空态、滚动条。切换 GUI 浅/暗不影响终端。仅保证 16 色 ANSI 一致，256 色沿用 xterm 默认，truecolor 不受影响。

## 字体

```css
/* GUI */
font-family: var(--font-sans);  /* SF Pro Text / Segoe UI / Noto Sans */
font-size: 14px;

/* 终端 */
font-family: var(--font-mono);  /* Cascadia Code / Fira Code / JetBrains Mono / Consolas */
font-size: 12px (默认，可调节 10-24)
```

## 圆角与阴影

| 元素 | 圆角 | 阴影 |
|------|------|------|
| 按钮 | `--radius-md` (6px) | 无 |
| 终端容器 | `--radius-lg` (8px) | 无 |
| 弹窗 | `--radius-xl` (12px) | `--shadow-lg` |

## 侧边栏面板

侧边栏通过 IconBar 按钮切换面板内容，`v-show` 控制显示（保留各面板状态）：

| 面板 | 组件 | 内容 |
|------|------|------|
| Sessions | SessionsPanel | 全局项目树（项目→会话两级，跨项目一步切换 + 状态徽标 `●N` 运行/琥珀点 pending） |
| Skills | SkillsPanel | 按 source 分组的 Skill 列表 |
| Agents | AgentsPanel | 按 source 分组的 Agent 列表 |
| MCP | McpPanel | MCP Server 列表 + 工具/提示/资源详情 |
| Plugins | PluginsPanel | Plugin 列表 + 包含的 Skills/Agents/MCP |
