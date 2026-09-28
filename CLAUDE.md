# CC Desk

> **当前架构权威入口**：[docs/native-cli-v3.md](docs/native-cli-v3.md)。Native CLI v3 代码侧已完成到 D27；D20 真实 Claude Code / Codex CLI Layer-C 认证仍需授权目标环境。Provider 管理、bundled CLI installer、独立 MCP runtime 等旧文档口径均已废弃。

面向 Claude Code 与 Codex CLI 的多项目、多会话桌面工作台。Tauri 2 + Vue 3 + xterm.js + Rust 负责宿主、终端与运行生命周期，真实 CLI 继续负责交互语义。

## 核心思想

**面向 Claude Code / Codex CLI 重度用户的多会话工作台。GUI 做宿主增强，不重做 CLI 已经拥有的能力。**

### 产品定位

- **面向谁**：已熟练使用 Claude Code、Codex CLI 或两者的开发者，尤其是需要同时管理多个会话、多个项目的重度用户
- **解决什么问题**：CLI 在单会话交互上已经足够好，但在多会话并行、信息总览、跨会话状态追踪上力不从心
- **核心价值**：
  1. **多会话并行管理** — 一个窗口内同时运行多个 Claude/Codex 会话，快速切换、互不干扰
  2. **信息可视化增强** — MCP 工具详情等 CLI 不方便展示的信息，通过侧边栏面板呈现
  3. **工作流加速** — 快捷命令、prompt 片段、项目预设等 CLI 之外的外围辅助

### 设计原则

- **CLI 优先，GUI 增强** — Native CLI v3 直接运行真实 Claude Code / Codex CLI；GUI 负责宿主能力，交互语义仍归 CLI
- **轻量透明** — 原生 CLI 资源默认只读投影；Native workspace 使用独立 revision/CAS 存储，兼容旧 `~/.cc-box/` 数据但不让旧状态覆盖新状态
- **功能边界** — CLI 里已经很好用的功能（对话交互、slash 命令、快捷键、模型切换），不在 GUI 里重复实现；GUI 专注于管理、可视化、辅助三类增强
- **可逆性** — 用户可随时回到纯 CLI；CC Desk 不拥有 Provider/API Key 配置，不把 native resource projection 变成第二个配置写入器
- **最小依赖** — 不依赖 Claude/Codex 内部 SDK；版本相关能力必须通过真实 CLI/公开契约验证，不能把宿主测试冒充真实 CLI 认证

### 不做什么

- 不做 AI 补全/输入建议（CLI 已有）
- 不做 slash 命令的 GUI 封装（CLI 已有）
- 不做对话消息的结构化展示（终端原生渲染足够好）
- 不做 Provider/API Key 管理，也不做通用 native CLI 配置写入器；配置切换交给 CLI 或 cc-switch 等外部工具
- 不做独立的 prompt 管理系统（CLI 的 /memory 和 CLAUDE.md 已覆盖）

## 技术栈

Tauri 2.x (Rust) + Vue 3 + TypeScript + Vite + xterm.js + portable-pty + Pinia + 自定义 CSS

## 项目架构

> Native CLI v3 的权威边界见 [docs/native-cli-v3.md](docs/native-cli-v3.md)。旧 Claude workspace 仅用于兼容；新的双 CLI 功能不得借道旧 PTY/API。

```text
cc-desk/
├── src-tauri/src/
│   ├── cli/                    # Native CLI 核心：workspace/profile/document/launch/projection
│   ├── terminal_input.rs       # 有序、分段、no-replay 输入
│   ├── terminal_transport.rs   # 有界输出、offset/ACK、全局预算
│   ├── run_supervisor.rs       # owned run 生命周期
│   ├── observer_*.rs           # 可选 observer 隔离
│   ├── pty.rs                  # legacy Claude PTY 兼容路径
│   ├── checks.rs               # legacy 启动环境检查
│   └── store.rs                # legacy Claude 数据读取
├── src/
│   ├── components/
│   │   ├── NativeCliWorkbench.vue
│   │   ├── NativeCliTerminal.vue
│   │   ├── TerminalView.vue    # legacy Claude workspace
│   │   └── XTermTerminal.vue   # legacy terminal
│   ├── stores/
│   │   ├── nativeWorkbench.ts
│   │   ├── nativeTabs.ts
│   │   ├── cliProfiles.ts
│   │   └── cliWorkspace.ts
│   ├── api/
│   │   ├── cli.ts
│   │   ├── nativeProjection.ts
│   │   └── tauri.ts
│   └── terminal/               # native host protocol / input queue / binding
├── tests/native-cli/           # Native CLI host/unit regression
├── docs/superpowers/execution/ # Dxx 执行账本
└── docs/native-cli-v3.md       # 当前架构权威说明
```

## 核心数据流

### Native CLI v3（forward path）

```text
NativeCliTerminal
  ↕ authenticated document bridge
cli launch / terminal_input / terminal_transport
  ↕ owned PTY
Claude Code | Codex CLI
```

- launch 绑定 cli/project/profile revision/request/run/generation/action；
- input 使用有序 intent + authenticated staged writer，partial/unknown 不重放；
- output 使用 bounded stream + exact ACK，错 owner/generation/stream 一律拒绝；
- resource projection 使用 backend-held scope，前端 path/opaque id 本身不构成授权；
- Native UI 不得 fallback 到 legacy `ptySpawn` / `ptyInput` / `ptyKill`。

### Legacy Claude workspace（compatibility path）

```text
XTermTerminal ←→ legacy Tauri IPC ←→ pty.rs ←→ Claude Code
```

Legacy 路径继续维护兼容性，但不是新的双 CLI 架构扩展点。

### Hook 监控数据流

```
Claude CLI hook 触发 → report-hook.sh → curl POST → hook_server.rs (axum) → emit('hook-event') → stores/hook.ts (事件总线) → useStatusMonitor (状态监控)
```

- Plugin 通过 `--plugin-dir` 按 session 加载，注入 11 个 hook 事件
- 每个 PTY 注入 `CC_BOX_HOOK_PORT`（服务器端口）和 `CC_BOX_SESSION_ID`（终端标识）
- `stores/hook.ts`：纯事件总线，模块通过 `subscribe(eventTypes[], handler)` 注册消费
- `useStatusMonitor`：hook 事件 → Tab 的 `working`/`pending` 状态 + 任务栏跳动
- 详细架构 → [docs/hook-monitor.md](docs/hook-monitor.md)

### 全局项目树数据流

```
sessionStore（tabs + historySessions）→ buildProjectGroups（分组+孤儿）→ sortProjectGroups（置顶→字母序→孤儿置底）→ filterProjectGroups（搜索）→ SessionsPanel 组装 ProjectNode 树
点会话节点 → resolveSwitchAction（纯函数，D/E 参数直传无竞态）→ TerminalView handler（切 cwd + 切 tab / --resume）；点项目节点 = 展开/折叠（toggleExpand），不切换
```

- Sessions 面板从「当前项目扁平列表」升级为「项目→会话全局树」：终端视图内跨项目一步切换 + 并行项目状态徽标（`●N` 运行 / 琥珀点 pending）一眼可见，后端 `projects.json` + `get_projects_state`（共享锁读）+ pin/unpin/archive/restore/set_display_name 5 增量 command（独立 `projects.json.lock` 跨进程排他锁，置顶/存档/别名持久化）
- `resolveSwitchAction`：纯函数决策切换语义（activate / resume，点会话节点；点项目节点 = 展开/折叠不经此函数），输入全显式参数、不读写全局单值中间态，连续调用互不影响
- `getHistoryFor(path)`：多项目历史选择器，按项目路径隔离历史，跨项目切换不串扰
- 展开状态：`expandOverride`/`toggleExpand`/`isExpanded`，纯手动展开（不自动展开当前/active），其余折叠
- **项目别名（display name）**：`projects.json` 的 `displayNames`（normalizedPath → 别名）→ `loadProjectsState` → `displayNames` reactive Map → `getDisplayName`（别名优先 basename 回退）→ `buildProjectGroups`（含孤儿）/ `TitleBar` / native window title（watch `getDisplayName(cwd)` 实时刷新）/ `ProjectSelectView` 项目行 + 已存档视图；搜索查 displayName+basename+path 三字段（`matchProjectQuery`）
- 编辑入口：管理页 `editingPath` 多行独立 input + 全局树 ProjectNode 单实例 editState，`editReducer` 状态机（成功才关 / 失败保留 + 错误 / 防重复 / retry + request id）
- **多实例并发安全**：projects.json 写走后端独立 `projects.json.lock`（std `File::lock`）跨进程排他锁 + apply 增量操作（pin/unpin/archive/restore/setDisplayName 各一 async command，`spawn_blocking` 内锁定读最新 → canonicalize → 校验应用 → 原子写 → 返回最新）；Windows 已有文件通过 `ReplaceFileW` 原子覆盖。前端 `session.ts` 的 `opLock` 串行完整 action/reload request + apply；窗口聚焦 reload 共享锁读。config.json 的 hiddenProjects/lastOpened 暂未纳入（同 pattern 可扩展）。升级时须先关闭旧版本实例（见 spec §8 迁移风险）
- 详细架构 → [docs/components.md](docs/components.md)

## 设计系统

- **[DESIGN.md](DESIGN.md)** 是视觉系统唯一规范：「工匠终端 (Artisan Terminal)」主题、双主题 token、命名规则（Quiet Chrome / Amber Activation / Warm Neutral / 14px Baseline / Flat-First）。UI 改动须遵循其 Do's and Don'ts；`.impeccable/` 仅为本地设计工具状态，不纳入公开仓库
- 色彩/字体/圆角/阴影 token 一律引用 `src/styles/global.css` 的 CSS 自定义属性，浅色/暗色两套值成对修改；终端层主题独立（`--terminal-*`）

详细架构 → [docs/terminal-integration.md](docs/terminal-integration.md)

## 开发命令

```bash
npm install                # 安装依赖
npm run tauri:dev          # 开发模式（前端 :1420 + Rust 热重载）
npm run tauri:build        # 生产构建
```

### Windows 环境配置
- **Rust 工具链**：使用 MSVC 工具链（`rustup default stable-x86_64-pc-windows-msvc`）
- **前置依赖**：需安装 [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选「C++ build tools」和「Windows 11 SDK」
  - 安装后确保 `link.exe` 为 MSVC 版本而非 Git coreutils 的 `link`
  - 若 Git bash 中 `cargo build/test` 提示 `link: extra operand`：仓库的 `.cargo/config.toml` 已移除机器专属 linker（可移植，cargo 默认发现 MSVC）；本机需 linker workaround 时先 `git update-index --skip-worktree src-tauri/.cargo/config.toml`，再在本地 config.toml 加 `[target.x86_64-pc-windows-msvc] linker = "<本机 MSVC link.exe 绝对路径>"`（不进仓库）
- **代理设置**：
  - 推送到 GitHub 需要代理：
    ```bash
    set HTTP_PROXY=http://127.0.0.1:33210
    set HTTPS_PROXY=http://127.0.0.1:33210
    ```
  - 推送到 Gitee 不需要代理
- **打包代理设置**：首次打包下载 NSIS 组件时需要代理，设置环境变量：
  ```bash
  set HTTP_PROXY=http://127.0.0.1:33210
  set HTTPS_PROXY=http://127.0.0.1:33210
  npm run build:win
  ```

### 独立仓库

- **GitHub**：`https://github.com/shawnwu2022/cc-desk`
- 项目源自 `orczh-hj/cc-box`，现按独立产品方向维护；来源与版权说明见 `NOTICE.md`
- `~/.cc-box/`、`CC_BOX_*` 与 `cc-box-light` / `cc-box-dark` 暂作为兼容标识保留，避免旧用户配置和插件协议失效
- 发布默认只面向 CC Desk 的 GitHub Releases，不再自动同步或发布到原项目的 Gitee / OSS 渠道
- 首次发布前必须换用 CC Desk 自有 Tauri updater 密钥；私钥只存 GitHub Secrets，禁止提交到仓库
### 版本/发布边界（当前：signed candidates only）

- 普通开发 PR 只跑验证，不做版本 bump/tag/release。
- `.github/workflows/release.yml` 当前只构建并上传 signed candidate artifacts。
- `scripts/release-policy.mjs` 必须保持 fail-closed（`mayPublish() === false`）。
- 不得因为 CI 全绿就恢复 GitHub Release/updater 发布路径。
- 真实 Claude Code / Codex CLI Layer-C 证据属于 D20，和普通代码 CI 分层记录。
- 公开发布必须另行设计显式 promotion：绑定不可变候选产物、真实 CLI 证据、审批与回滚。

详细流程 → [docs/release-process.md](docs/release-process.md)

## 详细文档

| 文档                                                           | 内容                                                    |
|--------------------------------------------------------------|-------------------------------------------------------|
| [docs/测试编写原则.md](docs/测试编写原则.md)   | 项目如何编写测试                                              |
| [docs/manual-test-cases.md](docs/manual-test-cases.md)   | **手动测试条目**：自动化无法覆盖的 UI 交互与端到端测试                  |
| [docs/terminal-integration.md](docs/terminal-integration.md) | 终端集成架构、PTY 生命周期、IPC 命令与事件对照                           |
| [docs/hook-monitor.md](docs/hook-monitor.md)                 | **Hook 监控系统**：Plugin 注入、事件采集、状态机、多终端区分                |
| [docs/layout-design.md](docs/layout-design.md)               | 布局设计、窗口结构、色彩系统、排版规范                                   |
| [docs/components.md](docs/components.md)                     | 组件树、各组件职责与 props/events、Store 结构                      |
| [docs/interaction.md](docs/interaction.md)                   | **快捷键处理架构**、三场景输入处理、DOM 捕获期监听                         |
| [docs/capabilities.md](docs/capabilities.md)                 | **Tauri 权限管理**、查询/确认/添加 capabilities 权限的方法            |
| [docs/data-persistence.md](docs/data-persistence.md)         | 数据存储架构、文件路径、JSON 结构                                   |
| [docs/env-injection.md](docs/env-injection.md)               | **环境变量注入**：PTY 启动时注入环境变量、注入顺序、扩展方式       |
| [docs/startup-checks.md](docs/startup-checks.md)             | 启动先决条件检查、路径检测与自动保存                                    |
| [docs/roadmap.md](docs/roadmap.md)                           | 开发路线图、进度跟踪、待办事项                                       |
| [docs/logging.md](docs/logging.md)                           | 日志文件路径、级别策略、轮转与清理机制                                   |
| [docs/release-process.md](docs/release-process.md)           | 版本号管理、本地打包、CI/CD 发布、签名与分发                             |

外部参考：[Claude Code 线上文档](https://code.claude.com/docs/llms.txt)

外部参考：[Tauri 2.x JS API线上文档](https://v2.tauri.org.cn/reference/javascript/api/)

## 约定

- 每次修改后，核心更新同步到 CLAUDE.md，细节更新同步到 docs/*.md
- Rust 结构体返回前端时统一使用 `#[serde(rename_all = "camelCase")]`
- 添加新 Tauri Command：commands.rs 定义 → lib.rs 注册 → api/tauri.ts 封装
- 添加新 Tauri JS API 调用时，必须确认 `capabilities/default.json` 中有对应权限（`<plugin>:default` 不包含大部分写操作，需显式添加）→ 详见 [docs/capabilities.md](docs/capabilities.md)

### 测试要求

- **开发必须搭配测试**：新增功能、修改逻辑、修复 bug 时，同步编写或更新对应测试。遵循 [测试编写原则](docs/测试编写原则.md)
- **Bug 修复必须先写测试**：修复 bug 时，先编写测试复现问题，确认测试失败，然后修复代码直至测试通过
- **自动测试优先**：能用自动测试覆盖的场景，必须写成自动测试，不要写入手动测试文档
- **手动测试条目**：仅记录自动化测试无法覆盖的场景（如真实 PTY 进程环境、跨组件端到端交互、视觉表现），记录到 `docs/manual-test-cases.md`，每个条目包含：测试目标、前置条件、操作步骤、预期结果
- **测试文件独立存放**：
  - 前端：项目根目录 `tests/` 文件夹，运行 `npm test`
  - 后端：`src-tauri/src/tests/` 文件夹，运行 `cd src-tauri && cargo test`
- **测试基础设施**：
  - 前端：Vitest + jsdom + `@tauri-apps/api/mocks`（mockIPC）
  - 后端：Rust `#[cfg(test)]` 模块，被测函数 `pub(crate)` 可见性
  - Store 测试：`setActivePinia(createPinia())` + `mockIPC`
- **命名规范**：英文函数名 `Feature_SubFeature_SeqNum` 格式，中文注释描述目标
- **什么要测**：纯函数、数据转换、解析逻辑、状态管理、边界条件和错误路径
- **什么不测**：getter/setter、类型定义、简单 props 传递、第三方库能力
- **树形项目会话管理测试**：`tests/stores/sessionTree.test.ts`（分组/排序/过滤/展开/多项目历史选择器 getHistoryFor）+ `tests/composables/projectTreeNavigation.test.ts`（resolveSwitchAction 切换语义 noop/activate/resume/new，D/E 纯函数参数直传无竞态）
- **焦点队列测试**：`tests/composables/attentionQueue.test.ts`（attentionFromEvent 事件→关注项分类，含 codex 反驳 SessionStart 不报完成/PostToolUseFailure 不算 error；severityRank/buildAttentionQueue 去重排序）+ `tests/stores/attention.test.ts`（store ingestEvent upsert、ackPty/clearPty、queue getter）+ `tests/stores/session.test.ts` PtyExit_ClearPending（codex pending 泄漏回归）
- **设计 token 对比度回归**：`tests/designTokens.test.ts`（直接解析 `global.css`，断言 tertiary/secondary/primary/琥珀文字 token 对三档背景双主题 WCAG AA ≥4.5:1、主文字 AAA，tag token 双主题成对存在；调色必须先过此测试）
- **侧边栏键盘事件回归**：`tests/sidebarKeyboardHandlers.test.ts`（折叠头包含嵌套 ToggleSwitch/操作按钮时，Enter/Space 仅由当前折叠头处理，不拦截子控件默认行为）
