<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="CC Desk" width="80" height="80">
</p>

<h1 align="center">CC Desk</h1>

<p align="center">
  <strong>Claude Code / Codex CLI 原生桌面工作台 — 多项目、多会话管理</strong><br>
  一个窗口。真实 CLI。项目、Profile 与终端标签彼此隔离。
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

Claude Code 和 Codex CLI 的核心价值都在原生终端行为。CC Desk 不用 SDK 或自绘聊天界面替代它们，而是在真实 CLI 二进制外增加桌面工作区，用于管理**多个项目**、**多个 CLI Profile**和**独立终端标签页**。

Native CLI 工作台分别支持 Claude Code 与 Codex CLI；纯 Codex 环境不依赖 Claude 配置。已有的 Claude 项目/会话面板继续作为 legacy / 只读增强保留。

**终端仍是权威入口；桌面 UI 只负责工作区、启动、恢复以及可选投影。**

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

可在同一工作区中同时打开 Claude Code 与 Codex CLI 原生终端标签。每个 run 都冻结自己的 CLI、Profile、项目、输入输出流、进程生命周期和重启 generation，不会跨标签串线。

### 快速启动与预设

使用 per-CLI Profile，并显式选择 New、原生 Resume Picker、指定 Session ID 或 Raw argv。新原生 Profile 不会被 CC Desk 静默注入 model、approval、sandbox 或权限默认值。

### Claude Provider 管理（legacy 工具）

原有 Claude Provider 预设继续服务旧 Claude 工作流。它不是 Codex Provider/认证层，也不会应用到 Codex 原生 Profile。

### 侧边栏面板

非遮罩式侧边栏，不会抢占焦点：

- **会话** — 浏览、搜索、切换所有会话。状态指示灯显示运行/思考/等待状态。
- **MCP 服务器** — 查看已连接的 MCP 服务器、可用工具及其参数结构
- **Skills & Agents** — 快速访问 Claude Code skills 和 agent 配置
- **插件** — 查看已安装的插件及其组件

### 原生终端体验

Native CLI 工作台通过应用自有 PTY 和 xterm host 直接运行用户安装的 Claude Code 或 Codex CLI 二进制。Slash 命令、原生交互提示、终端编辑器、键盘/协议流、resize、剪贴板和恢复仍走 CLI 原生路径，而不是改造成聊天 UI。

---

## 先决条件

至少安装并认证一个你实际要使用的 CLI：

- **[Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)**
- **[Codex CLI](https://developers.openai.com/codex/cli/)**

纯 Codex 的 Native CLI 工作台不要求 Git Bash。Git Bash 仅与 legacy / 显式 shell 启动模式相关。

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

1. 打开应用并进入 **Native CLI**
2. 添加或选择项目目录
3. 创建/选择 Claude Code 或 Codex CLI Profile
4. 使用 New / Resume / Raw 启动会话；每个原生标签独立运行

---

## 从源码构建

<details>
<summary>点击展开</summary>

### 前置要求

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) stable 工具链（Windows 使用 MSVC）
- 运行时测试可选：[Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) 和/或 [Codex CLI](https://developers.openai.com/codex/cli/)
- **Windows 源码构建**：Microsoft C++ Build Tools 和 Windows SDK
- 只有测试显式/legacy Git Bash 启动路径时才需要 Git for Windows

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
<summary><strong>会修改我的 Claude Code 配置吗？</strong></summary>

CC Desk 对 Claude 会话和历史数据只读，大多数 GUI 设置仍保存在兼容旧版本的 `~/.cc-box/` 目录中。唯一主动写入原生配置的场景是用户显式激活 Provider：应用会把所选 env/model 字段合并到 `~/.claude/settings.json`，并保留其他无关设置。
</details>

<details>
<summary><strong>能用所有 CLI 功能吗？</strong></summary>

可以。Slash 命令、快捷键、模型切换、权限提示 — 所有功能透明传递给真实 CLI。
</details>

<details>
<summary><strong>性能如何？</strong></summary>

基于 Tauri 2 (Rust 后端)，安装后约 10 MB，内存占用极低。终端通过 xterm.js 渲染，性能与原生终端相当。
</details>

<details>
<summary><strong>Claude Code 更新后会失效吗？</strong></summary>

应用直接运行 CLI 二进制文件，不依赖任何内部 API。只要 CLI 在 PATH 中，任何版本都能正常工作。
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