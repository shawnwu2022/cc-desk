<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="CC Desk" width="80" height="80">
</p>

<h1 align="center">CC Desk</h1>

<p align="center">
  <strong>Claude Code 与 Codex CLI 原生桌面工作台 — 多项目、多标签页</strong><br>
  一个窗口。多个项目。快速切换会话。
</p>

<p align="center">
  <img src="https://img.shields.io/badge/平台-Windows%20%7C%20macOS%20%7C%20Linux-blue" alt="Platform">
  <img src="https://img.shields.io/badge/Tauri-2.x-orange" alt="Tauri">
  <img src="https://img.shields.io/badge/Vue-3-green" alt="Vue">
  <img src="https://img.shields.io/badge/许可证-MIT-yellow" alt="License">
</p>

---

[English](README.md) | 简体中文

---

## 为什么选择 CC Desk？

Claude Code 和 Codex CLI 最适合继续掌控自己的原生终端、认证、权限、模型和扩展能力；但当你同时管理**多个项目和多个 CLI 运行实例**时，多个终端窗口会迅速变得难以维护。

CC Desk 是一个**运行用户真实安装 CLI 的原生桌面工作台**。它提供独立项目登记、per-CLI profile、多标签页运行、启动/恢复状态以及按 CLI/root 隔离的只读资源视图，而不是用 SDK 或 Provider 代理替代 CLI。

**把它看作 Claude Code 与 Codex CLI 重度用户共用的一体化桌面工作台。**

---

## 界面截图

<p align="center">
  <img src="screenshots/projectselect.png" alt="项目选择" width="400">
  <img src="screenshots/project.png" alt="会话管理" width="400">
</p>

---

## 核心功能

### 多项目管理

在一个窗口中浏览所有项目。查看哪些项目有活跃会话，一键启动新会话，即时切换项目。不再需要频繁 `cd` 切换目录或管理多个终端窗口。

### 多会话并行

可以同时打开独立的 Claude Code 与 Codex CLI 原生终端标签页。每个标签页冻结自己的 CLI/profile/project/run 身份，切换标签页不会把输入或输出串到其他运行实例。

### 快速启动与预设

使用 per-CLI profile，并支持 New、原生 resume picker、显式 Session ID 和精确 raw argv。新原生 profile 不会由 CC Desk 自动注入 model/approval/sandbox 策略。

### Legacy Claude Provider 管理

现有 Claude Provider 面板继续服务于 legacy Claude 工作流。激活 Provider 是显式的 Claude-only 操作，只会把所选 env/model 字段合并到 `~/.claude/settings.json`；这些设置不会复制到 Codex profile。

### 侧边栏面板

非遮罩式侧边栏，不会抢占焦点：

- **会话** — 浏览、搜索、切换所有会话。状态指示灯显示运行/思考/等待状态。
- **MCP 服务器** — 查看已连接的 MCP 服务器、可用工具及其参数结构
- **Skills & Agents** — 在所选 CLI/root 支持时提供按来源隔离的只读视图
- **插件** — 查看已安装的插件及其组件

### 原生终端体验

Native CLI 工作台通过应用自有 PTY 与 xterm parser 运行用户真实安装的 Claude Code 或 Codex CLI。宿主尽量保留 CLI 自己的交互，而不是重新实现命令、approval、认证或模型路由。

---

## 先决条件

- 运行时至少安装一个受支持 CLI：**Claude Code 和/或 Codex CLI**
- 每个 CLI 通过自己的原生流程完成认证
- **Windows 源码构建**：需要 Microsoft C++ Build Tools 与 Windows SDK；Git for Windows 仅对 legacy shell 工作流可选，不是 Codex 的全局前置条件

---

## 快速开始

### 1. 下载安装

前往 [**Releases**](https://github.com/shawnwu2022/cc-desk/releases) 页面下载对应平台的安装包：

| 平台 | 文件 |
|------|------|
| **Windows** | `.exe` (NSIS 安装包) |
| **macOS** | `.dmg` (Apple 芯片) |
| **Linux** | `.AppImage` |

### 2. 启动使用

1. 打开应用
2. 选择或添加项目目录
3. 打开 **Native CLI**，选择或创建 Claude/Codex profile，然后选择 New/Resume/Raw
4. 按需继续打开原生标签页，每个 run 保持独立所有权

---

## 从源码构建

<details>
<summary>点击展开</summary>

### 前置要求

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) stable 工具链（Windows 使用 MSVC）
- 构建应用只需要 Node.js 与 Rust
- 测试原生运行能力时再安装 Claude Code 和/或 Codex CLI
- **Windows 用户**: Microsoft C++ Build Tools 和 Windows SDK

### 安装

```bash
git clone https://github.com/shawnwu2022/cc-desk.git
cd cc-desk
npm install
```

### 开发

```bash
npm run tauri:dev     # 启动开发模式（热重载）
```

### 构建

```bash
npm run tauri:build   # 构建当前平台

# 或指定平台：
npm run build:win     # Windows (x86_64-pc-windows-msvc)
npm run build:mac     # macOS (通用)
npm run build:linux   # Linux (x86_64)
```

构建产物在 `src-tauri/target/release/bundle/`。

</details>

---

## 常见问题

<details>
<summary><strong>会修改我的 CLI 配置吗？</strong></summary>

新的 native workspace 使用 CC Desk 自己的 profile/项目登记，并把原生 CLI 历史与配置视为只读来源。Codex profile 不会继承 Claude legacy env/权限设置。现有 Claude Provider 激活仍是显式的 legacy Claude-only 写入，只会合并到 `~/.claude/settings.json`，并保留其他无关设置。
</details>

<details>
<summary><strong>能用所有 CLI 功能吗？</strong></summary>

Native host 的目标是保留真实 CLI/TUI，而不是对白名单中的命令逐项重写。兼容性按具体 OS/CLI/安装包目标组合认证；未测试或刚发生变化的 CLI 版本不会自动标记为已认证。
</details>

<details>
<summary><strong>性能如何？</strong></summary>

基于 Tauri 2 (Rust 后端)，安装后约 10 MB，内存占用极低。终端通过 xterm.js 渲染，性能与原生终端相当。
</details>

<details>
<summary><strong>Claude Code 或 Codex CLI 更新后怎么办？</strong></summary>

只要配置的 executable/profile 有效，未知版本仍可启动；但发布认证是版本相关的。Pinned/stable canary 会记录二进制身份，CLI 发生变化后需要重新认证相关 target，CC Desk 不会宣称未来所有 CLI 版本都天然兼容。
</details>

---

## 项目来源

CC Desk 最初基于 [orczh-hj/cc-box](https://github.com/orczh-hj/cc-box) fork，随后因产品目标与实现路径逐渐不同，现作为独立项目维护，并非原项目的官方后继版本。

项目保留原始 MIT 许可证和版权声明，详见 [LICENSE](LICENSE)；更完整的来源说明见 [NOTICE.md](NOTICE.md)。

CC Desk 使用新的应用标识，因此会与 CC-Box 分开安装；既有设置仍从 `~/.cc-box/` 复用。确认新版本运行正常后，再单独卸载旧应用。

---

## 技术栈

Tauri 2 (Rust) + Vue 3 + TypeScript + xterm.js + portable-pty

---

## 许可证

[MIT](LICENSE)