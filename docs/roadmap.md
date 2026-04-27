# 开发路线图

## Phase 1 — 基础终端 ✅

**目标：能够运行 Claude CLI**

- ✅ Electron 脚手架
- ✅ xterm.js + node-pty 集成
- ✅ PTY 双向数据流（onData → ptyInput，onPtyOutput → term.write）
- ✅ 环境检查（Git Bash、Claude CLI、node-pty）
- ✅ 项目收藏（favorites.json）
- ✅ 浅色主题（GUI + 终端）
- ✅ 基础侧边栏（项目列表）
- ✅ 快捷键透明传递

## Phase 2 — 多会话管理 ✅

**目标：支持多会话和 Push 模式侧边栏**

- Push 模式侧边栏
  - ✅ 移除 overlay，改为 flexbox 布局
  - ✅ 终端区域自动收缩适应
  - ✅ 无遮罩层，无焦点切换

- 多会话管理
  - ✅ 创建新会话（继承 cwd）
  - ✅ 会话切换（保留原会话运行）
  - ✅ 会话重命名（inline 编辑）
  - ✅ 会话搜索过滤
  - ✅ 运行状态指示（绿标）
  - ✅ 会话重启功能

- 终端设置
  - ✅ 字体大小调节（即时生效）
  - ✅ 设置面板（折叠展示）

## Phase 3 — 多会话增强 + 信息面板

**目标：补齐核心差异化功能，让多会话体验完整可用**

| # | 功能 | 优先级 | 关键文件 | 说明 |
|---|------|--------|----------|------|
| 3A | 会话输出缓冲恢复 | P0 | `XTermTerminal.vue`, `stores/session.ts` | 切换 tab 时用 SerializeAddon 序列化/恢复终端内容。依赖已装但未用的 @xterm/addon-serialize |
| 3B | Token 用量/成本统计 | P0 | `commands.rs`, `store.rs`(扩展聚合), `StatsPanel.vue`(新建), `tauri.ts` | 后端已有 total_tokens/total_cost 解析（store.rs:477-510），需新增聚合命令和前端面板 |
| 3C | 会话历史持久化 | P1 | `stores/session.ts`, `store.rs` | 退出时序列化 tabs 到 `~/.claude-gui/tabs.json`，启动时恢复 |
| 3D | MCP 工具详情完善 | P1 | `McpSubItem.vue`, `McpItem.vue` | 增加 inputSchema 展示、工具名复制、连接状态刷新 |

## Phase 4 — 工作流加速 + 外观

**目标：提升日常使用效率，增加主题支持**

| # | 功能 | 优先级 | 关键文件 | 说明 |
|---|------|--------|----------|------|
| 4A | 快捷命令面板 | P0 | `CommandPalette.vue`(新建), `useTerminalCommand.ts`(扩展), `stores/sidebar.ts` | 自定义常用命令，选中后通过 ptyInput 发送 |
| 4B | 项目级预设参数 | P1 | `stores/app.ts`, `stores/config.ts`, `store.rs` | 不同项目自动加载不同 Claude 参数 |
| 4C | 深色主题 | P1 | `global.css`, `XTermTerminal.vue`, `SettingsModal.vue` | CSS 变量 dark 主题 + xterm dark theme。SettingsModal 已有 "Dark (Coming soon)" 选项 |
| 4D | 会话全局搜索 | P2 | `store.rs`, `commands.rs`, `SessionsPanel.vue` | 跨项目搜索历史会话 |

## Phase 5 — 高级功能

**目标：进阶用户的效率工具**

| # | 功能 | 优先级 | 说明 |
|---|------|--------|------|
| 5A | 分屏布局 | P0* | TerminalView 重构为分片容器，CSS Grid，比例可拖拽 |
| 5B | 系统托盘 | P1 | tauri-plugin-tray |
| 5C | 全局快捷键 | P2 | Tauri global shortcut API |
| 5D | 多预设主题 | P2 | Catppuccin、Nord 等主题文件 |

*Phase 5 的 P0 是该阶段内的相对优先级

## 待解决问题

| 问题 | 状态 | 说明 |
|------|------|------|
| Windows node-pty 编译 | ✅ 已解决 | npm rebuild node-pty |
| Git Bash 检测 | ✅ 已解决 | 自动检测并设置 CLAUDE_CODE_GIT_BASH_PATH |
| Ctrl+W 冲突 | ✅ 已解决 | 阻止关闭窗口，手动发送到 PTY |
| 终端空白 | 🔍 待验证 | 检查 PTY output 是否正确发送 |

## 技术债务

- 添加单元测试
- 完善 TypeScript 类型
- 错误处理机制
- 日志系统