# 开发路线图

## Phase 1 — 基础终端 ✅

**目标：能够运行 Claude CLI**

- ✅ Tauri 2 脚手架 + 自定义标题栏
- ✅ xterm.js + portable-pty 集成
- ✅ PTY 双向数据流
- ✅ 环境检查（Claude CLI 路径检测 + 启动类型检测）
- ✅ 项目收藏与缓存
- ✅ 浅色主题（工匠终端色彩系统）
- ✅ 基础侧边栏

## Phase 2 — 多会话管理 ✅

**目标：支持多会话和面板式侧边栏**

- ✅ IconBar + SidebarPanel 面板式布局
- ✅ 多会话管理（创建/切换/重命名/搜索/恢复）
- ✅ 运行状态指示灯（Hook 监控驱动）
- ✅ 会话匹配（输出驱动即时匹配 + 轮询兜底）
- ✅ 设置面板（字号/启动参数/关于/更新）
- ✅ 快捷键系统（DOM capturing phase）

## Phase 3 — 信息面板 ✅

**目标：补齐核心差异化功能**

- ✅ MCP Servers 面板（工具/提示/资源详情）
- ✅ Skills 面板（按来源分组）
- ✅ Agents 面板（按来源分组）
- ✅ Plugins 面板（包含的 Skills/Agents/MCP）
- ✅ Hook 监控系统（Plugin 注入 + HTTP 上报 + 状态推导 + 指示灯）
- ✅ 项目配置只读展示
- ✅ 会话消息搜索
- ✅ 日志系统（按日期轮转 + 前端日志桥接）
- ✅ 自动更新（GitHub Releases 检测 + 下载 + 安装）

## Phase 4 — 工作流加速 + 外观

**目标：提升日常使用效率，增加主题支持**

| # | 功能 | 优先级 | 说明 |
|---|------|--------|------|
| 4A | 会话输出缓冲恢复 | P0 | 切换 tab 时用 SerializeAddon 序列化/恢复终端内容 |
| 4B | Token 用量/成本统计 | P0 | 后端已有解析，需新增聚合命令和前端面板 |
| 4C | 会话历史持久化 | P1 | 退出时序列化 tabs，启动时恢复 |
| 4D | 深色主题 | P1 | CSS 变量 dark 主题 + xterm dark theme |

## Phase 5 — 高级功能

**目标：进阶用户的效率工具**

| # | 功能 | 优先级 | 说明 |
|---|------|--------|------|
| 5A | 分屏布局 | P0* | TerminalView 重构为分片容器 |
| 5B | 系统托盘 | P1 | tauri-plugin-tray |
| 5C | 快捷命令面板 | P2 | 自定义常用命令 |
| 5D | 多预设主题 | P2 | Catppuccin、Nord 等主题文件 |

*Phase 5 的 P0 是该阶段内的相对优先级

## 已解决的技术问题

| 问题 | 解决方案 |
|------|----------|
| Windows 编译环境 | portable-pty 纯 Rust，无需 node-pty 编译 |
| Git Bash 检测 | 自动检测并设置 CLAUDE_CODE_GIT_BASH_PATH |
| Claude CLI 启动类型 | 自动检测 native/npm 安装，缓存到 config.json |
| Hook 稳定性 | 始终 exit 0、超时保护、陈旧检测降级 |
