# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

（Tauri 2 桌面应用，系统 webview 渲染。发布 Windows / macOS / Linux，但设计基准为 **Windows 优先**——2026-08 用户确认：以 Windows 体验为准，macOS/Linux 保持功能正确，不做每平台精修。）

## Users

- **主要用户**：已熟练使用 Claude Code CLI 的开发者，尤其是需要同时管理多个项目、多个会话的重度用户。
- **情境**：单窗口内并行运行多个 Claude 会话，跨项目快速切换，多个后台会话的运行/等待状态一眼可见。
- **要完成的事**：让 CLI 做不好的事（多会话并行总览、信息可视化、跨会话状态追踪、工作流辅助）变容易——不替代 CLI 的单会话交互。
- **受众倾斜**（2026-08 用户确认）：重度用户优先。新用户通过预设库渐进上手，不为新用户牺牲高级功能的直接性。

## Product Purpose

CC Desk 是面向 Claude Code 的桌面应用：通过 portable-pty 直连真实 Claude CLI 二进制，保持与原生终端完全一致的输入输出体验，并在此基础上增加 CLI 不擅长的事——多项目/多会话管理、侧边栏信息面板（Sessions / MCP / Skills / Agents / Plugins）、快捷启动预设、Provider 管理、hook 驱动的会话状态监控。

成功 = 重度用户能在单窗口内高效并行管理多项目多会话。路线图方向（docs/roadmap.md）：从「配置只读展示」升级为「开箱即用的 Claude 软件」——可编辑的配置管理与预设库。

## Positioning

- **Purpose-built for Claude Code**：不是通用终端或通用 GUI 客户端；专门围绕 Claude Code 的工作流构建（会话状态、MCP 工具详情、Provider、Claude 原生配置）。
- **透明直连**：运行真实 CLI 二进制，不依赖任何内部 API；slash 命令、快捷键、交互提示全部原样透传，Claude Code 任意更新零适配。
- **轻量可逆**：~10 MB 安装体积、JSON 文件存储（无数据库）、原生数据只读；用户可随时回到纯 CLI。

## Operating Context

- **运行环境**：桌面应用。用户机器已安装并认证 Claude Code CLI；Windows 为主使用平台。
- **核心工作流**：项目选择 → 启动/恢复会话 → 终端内 CLI 原生交互 → 侧边栏面板查看信息/跨会话切换 → hook 事件驱动状态监控（working / pending / attention）。
- **数据边界**：Claude 原生数据（`~/.claude/`）只读；GUI 配置独立存于 `~/.cc-box/`；唯一原生写操作是 Provider 显式激活时合并 env/model 字段到 `~/.claude/settings.json`（可停用恢复，不覆盖其他配置）。
- **事件链**：Claude CLI hook → plugin（`--plugin-dir` 按会话注入）→ HTTP 上报 → 前端状态总线。
- **界面语言**：中英双语（vue-i18n，`src/i18n/`，默认英文，用户可切换）。项目文档、代码注释、提交信息使用中文。

## Capabilities and Constraints

**已确认能力**：
- 终端集成：xterm.js + portable-pty，PTY 生命周期管理，UTF-8 优先/GBK 回退解码
- 多会话：Tab + 侧边栏全局项目树（分组/置顶/存档/别名），跨项目一步切换；projects.json 跨进程排他锁保证多实例并发安全
- Hook 监控：11 个 hook 事件采集，Tab 状态指示，attention 关注队列
- 侧边栏面板：Sessions / MCP / Skills / Agents / Plugins（只读展示，路线图升级为可编辑）
- Provider 管理：50+ 厂商预设、CRUD、显式激活合并、cc-switch 导入
- 启动环境检查、快捷键系统（三场景输入处理）、自动更新（GitHub Releases + latest.json）、日志轮转

**产品边界（明确不做）**：
- 不做 AI 补全/输入建议（CLI 已有）
- 不做 slash 命令的 GUI 封装（CLI 已有）
- 不做对话消息的结构化展示（终端原生渲染足够好）
- 不做通用 Claude Code 配置编辑器（Provider 只管自己的 env/model 字段）
- 不做独立 prompt 管理系统（CLI 的 /memory 和 CLAUDE.md 已覆盖）

**技术约束**：
- 兼容标识 `~/.cc-box/`、`CC_BOX_*`、`cc-box-light`/`cc-box-dark` 暂保留（旧配置与插件协议兼容，非品牌表达）
- 新增 Tauri JS API 调用必须确认 `capabilities/default.json` 权限
- 开发必须搭配测试（前端 Vitest + jsdom，后端 cargo test）；bug 修复先写复现测试

**未决事项**：无（2026-08 init 访谈已覆盖全部材料缺口）。

## Brand Commitments

- **名称**：CC Desk。fork 自 orczh-hj/cc-box，独立产品方向，非官方后继；来源与版权记录于 `NOTICE.md`，MIT 协议。
- **兼容标识**：`~/.cc-box/`、`CC_BOX_*` 等旧标识仅为兼容保留，不作为品牌表达延续。
- **Logo**：`src-tauri/icons/`（README 引用）。
- **分发**：GitHub Releases（shawnwu2022/cc-desk）；发布仅面向 CC Desk 自有渠道，不再同步原项目 Gitee/OSS。
- 视觉风格与 token 规范记录于 [`DESIGN.md`](DESIGN.md)，实现以 `src/styles/global.css` 为准。

## Evidence on Hand

- `README.md` / `README_CN.md`：双语产品说明，含截图（`screenshots/`）
- `CLAUDE.md`：产品定位、设计原则、架构、数据流
- `docs/roadmap.md`：开发进度与「配置管理」阶段规划
- `docs/*.md`：全套架构文档（终端集成、hook 监控、Provider、布局、交互、数据持久化、发布流程等）
- 完整可运行实现（`src/`、`src-tauri/`）：成熟的既有视觉实现，视觉规范见 [`DESIGN.md`](DESIGN.md)
- 测试资产：前端 `tests/`、后端 `src-tauri/src/tests/`、手动测试条目文档
- **不得虚构**：无用户测试数据、testimonial、使用量指标或第三方评价

## Product Principles

1. **CLI 优先，GUI 增强** — GUI 只做 CLI 做不好或做起来不方便的事（管理、可视化、辅助三类），不重复 CLI 已好的功能。
2. **透明可逆** — 原生数据只读、GUI 配置独立、写操作显式可恢复；用户随时可回到纯 CLI。
3. **重度用户效率优先** — 信息密度、状态可见、一步切换；不为新用户牺牲高级功能的直接性。
4. **最小依赖、零适配** — 直连 CLI 二进制，不依赖内部 API；轻量 JSON 存储，完全兼容 Claude Code 任何更新。
5. **渐进开放** — 从只读展示逐步过渡到可编辑管理，行为与 CLI 保持一致。

## Accessibility & Inclusion

- 2026-08 用户确认：无专门无障碍标准要求；遵循常规对比度与控件可达性即可，不设 WCAG AA 硬性验收。
