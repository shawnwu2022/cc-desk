# CC-Box

针对 Claude Code 开发的多终端管理器，用 Tauri 2 + Vue 3 + xterm.js + portable-pty 直连 Claude CLI，在原生终端体验基础上增加外围功能和信息呈现。

## 核心思想

**面向 Claude Code 重度用户的多会话管理器。GUI 做增强不做替代，让 CLI 做不好的事变得容易。**

### 产品定位

- **面向谁**：已熟练使用 Claude Code CLI 的开发者，尤其是需要同时管理多个会话、多个项目的重度用户
- **解决什么问题**：CLI 在单会话交互上已经足够好，但在多会话并行、信息总览、跨会话状态追踪上力不从心
- **核心价值**：
  1. **多会话并行管理** — 一个窗口内同时运行多个 Claude 会话，快速切换、互不干扰
  2. **信息可视化增强** — Token 用量、会话成本、MCP 工具详情等 CLI 不方便展示的信息，通过侧边栏面板呈现
  3. **工作流加速** — 快捷命令、prompt 片段、项目预设等 CLI 之外的外围辅助

### 设计原则

- **CLI 优先，GUI 增强** — 直接运行 Claude CLI 二进制文件，输入行为与原生终端完全一致；GUI 只做 CLI 做不好或做起来不方便的事
- **轻量透明** — JSON 文件存储，不引入数据库/路由/状态机；原生数据只读，GUI 配置独立保存在 `~/.claude-gui/config.json`
- **功能边界** — CLI 里已经很好用的功能（对话交互、slash 命令、快捷键、模型切换），不在 GUI 里重复实现；GUI 专注于管理、可视化、辅助三类增强
- **可逆性** — 用户可随时回到纯 CLI，GUI 不修改任何 Claude Code 原生配置文件，不留副作用
- **最小依赖** — 完全兼容 Claude Code 任何更新，无需适配 SDK API；新功能通过读取原生配置文件和 CLI 命令获取

### 不做什么

- 不做 AI 补全/输入建议（CLI 已有）
- 不做 slash 命令的 GUI 封装（CLI 已有）
- 不做对话消息的结构化展示（终端原生渲染足够好）
- 不做 Claude Code 配置的编辑器（只做只读展示，编辑由 CLI/settings.json 完成）
- 不做独立的 prompt 管理系统（CLI 的 /memory 和 CLAUDE.md 已覆盖）

## 技术栈

Tauri 2.x (Rust) + Vue 3 + TypeScript + Vite + xterm.js + portable-pty + Pinia + 自定义 CSS

## 项目架构

```
cc-box/
├── src-tauri/                  # Rust 后端
│   ├── src/
│   │   ├── main.rs             # 入口
│   │   ├── lib.rs              # 初始化、插件注册、Command 注册
│   │   ├── pty.rs              # PTY 管理（portable-pty 封装）
│   │   ├── commands.rs         # Tauri IPC 命令
│   │   ├── store.rs            # Claude Code 原生数据读取
│   │   ├── mcp.rs              # MCP 协议客户端（HTTP/SSE + stdio）
│   │   └── checks.rs           # 环境检查
│   ├── capabilities/           # Tauri 权限配置
│   ├── Cargo.toml
│   └── tauri.conf.json
│
├── src/                        # Vue 3 前端
│   ├── api/tauri.ts            # Tauri invoke/listen 封装
│   ├── components/             # UI 组件
│   │   ├── XTermTerminal.vue   # xterm.js 终端
│   │   ├── TerminalView.vue    # 终端容器
│   │   ├── SidebarPanel.vue    # 侧边栏
│   │   ├── sidebar/            # 侧边栏子面板（Session、MCP、Agent 等）
│   │   ├── mcp/                # MCP 组件（McpPanel > McpGroup > McpItem > McpSubItem）
│   │   ├── settings/           # 设置子组件
│   │   ├── WelcomeView.vue     # 欢迎页
│   │   └── ProjectSelectView.vue
│   ├── stores/                 # Pinia（app、session、sidebar、config）
│   ├── types/                  # TypeScript 类型定义
│   ├── composables/            # 组合式函数
│   └── styles/global.css       # CSS 变量与全局样式
│
├── docs/                       # 详细文档
├── package.json
└── vite.config.ts
```

## 核心数据流

```
xterm.js ←→ Tauri invoke/listen ←→ pty.rs (Rust) ←→ portable-pty ←→ Claude CLI
```

- 用户输入 → `onData` → `invoke('pty_input')` → PTY writer → Claude CLI
- CLI 输出 → PTY reader → `emit('pty-output')` → `term.write()` → xterm.js

详细架构 → [docs/terminal-integration.md](docs/terminal-integration.md)

## 开发命令

```bash
npm install                # 安装依赖
npm run tauri:dev          # 开发模式（前端 :1420 + Rust 热重载）
npm run tauri:build        # 生产构建
```

### Windows 环境配置
- **MinGW 设置**：`set PATH=C:\ProgramData\mingw64\mingw64\bin;%PATH%`
- **打包代理设置**：首次打包下载 NSIS 组件时需要代理，设置环境变量：
  ```bash
  set HTTP_PROXY=http://127.0.0.1:33210
  set HTTPS_PROXY=http://127.0.0.1:33210
  npm run build:win
  ```

## 详细文档

| 文档 | 内容 |
|------|------|
| [docs/terminal-integration.md](docs/terminal-integration.md) | 终端集成架构、PTY 生命周期、IPC 命令与事件对照 |
| [docs/layout-design.md](docs/layout-design.md) | 布局设计、窗口结构、色彩系统、排版规范 |
| [docs/components.md](docs/components.md) | 组件树、各组件职责与 props/events、Store 结构 |
| [docs/interaction.md](docs/interaction.md) | 交互规则、快捷键处理、输入透明机制 |
| [docs/data-persistence.md](docs/data-persistence.md) | 数据存储架构、文件路径、JSON 结构 |
| [docs/roadmap.md](docs/roadmap.md) | 开发路线图、进度跟踪、待办事项 |

外部参考：[Claude Code 线上文档](https://code.claude.com/docs/llms.txt)

## 约定

- 每次修改后，核心更新同步到 CLAUDE.md，细节更新同步到 docs/*.md
- Rust 结构体返回前端时统一使用 `#[serde(rename_all = "camelCase")]`
- 添加新 Tauri Command：commands.rs 定义 → lib.rs 注册 → api/tauri.ts 封装
