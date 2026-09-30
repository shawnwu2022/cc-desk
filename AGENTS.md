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
- 不做独立的 prompt 管理系统（CLI 的 /memory 和 AGENTS.md 已覆盖）

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

```text
UnifiedSession catalog（Legacy/Native adapters）→ UnifiedProjectGroup[] → SessionsPanel → ProjectNode → SessionList → SessionItem
已归档 catalog records → ArchivedSessionsDrawer → 同一 SessionList/SessionItem
```

- 统一树直接混排 Claude Code / Codex CLI；`ProjectNode` 和 `SessionList` 不再接收旧 tabs/history，也不依赖旧 PTY attention store。`SessionsPanel` 接收显式统一投影或默认读取 `unifiedSessions` store；adapter 初始化和生命周期 dispatch 由后续 workspace container 负责。
- 项目行只有一个新建快捷动作；固定/取消固定、重命名、查看归档、打开目录、移除走同一 `AppMenu`，鼠标右键和 overflow 共用动作。`new-session-request` 携带 projectKey/projectPath，不能交给旧容器的 `new-session` Legacy 启动 handler。
- normal `projectGroups` 保持排除已归档记录。UI 为仅剩归档的项目保留一个空项目壳以访问其归档菜单；面板全局归档入口在搜索无结果时仍可用。恢复仅发 `restore-request`，不直接恢复或启动，成功后由调用方发布 catalog 更新。
- 运行态 archive 必须发 `SessionTreeConfirmationRequest { kind: 'stop-and-archive', sessionId, projectKey, projectPath }`；未知/启动态不发归档请求。树组件不得直接 stop/archive，也不得 fallback 到 Legacy PTY；完整确认 UI 在后续任务实现。
- 展开状态按 projectKey 显式保存；搜索临时展开但禁止修改手动状态。项目名/原 basename/路径命中展示组内普通会话，会话名命中只展示匹配的普通会话。清空搜索恢复之前手动状态；nested controls 的 Enter/Space 不触发父级折叠，菜单/编辑器 Escape 不关闭整个面板。
- 旧 `session.ts` 的分组、`resolveSwitchAction`、历史缓存、`projects.json` persistence 和 ProjectSelectView 管理行为仍属于兼容数据/管理路径。它们不再是新的双 CLI 树 UI 边界，也不能用旧历史删除接口删除 Native 记录。
- `projects.json` pin/archive/displayName/delete 增量写继续通过独立 `projects.json.lock` 跨进程锁、增量 apply 和原子返回状态；统一项目状态由 `projectsState` store 读取。Native workspace 保持自己的 revision/CAS 和 authenticated document bridge。
- 组件 gate：`npm test -- tests/components/projectSessionTree.test.ts tests/sidebarKeyboardHandlers.test.ts && npm run typecheck`。真实 Windows 1024×640、100%/125%/150% 缩放和渲染可访问性仍是最终平台门禁。

### 性能边界

- `get_home_data` 单次扫描 `~/.claude/projects`，同时生成项目列表与真实路径映射；近期会话直接复用该映射的目录列表，`get_home_data` / `get_sessions` 的同步文件 IO 统一放入 `spawn_blocking`。
- JSONL 按字节行流式解析：项目路径在首个有效 `cwd` 后停止，名称继续扫描到 EOF 以保留末尾 `custom-title` 优先级；峰值内存为 O(最大 JSONL 单行)。首页、项目历史和 all-recent 共享 `~/.cc-box/session-name-index.json` 派生名称索引：`length + 高精度 mtime` 完全一致才 exact-hit（读取 0 JSONL bytes），任一变化都 full rebuild，不使用 append cursor。
- 名称索引每个请求只读一次快照，前台共享锁内只读取有界 raw bytes、锁外解析；业务值先返回，delta 在 detached `spawn_blocking` 中写回。后台使用 replacement stamp 复核、entry/bucket CAS、整文件 raw CAS 和唯一临时文件；排他锁内只做 64 KiB 分块 raw compare 与原子替换。8 MiB 以上压缩至 6 MiB，16 MiB 为读取硬上限；损坏、未知版本、锁/写失败均只降低命中率，不改变业务结果。
- `SettingsOverlay` 首次打开时才加载，之后保持挂载以保留关闭动画与内部状态；编辑器依赖位于独立异步 chunk。WebGL 仅在新终端启用 WebGL renderer 时动态加载，DOM renderer 仍为默认。

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
  - 若 Git bash 中 `cargo build/test` 提示 `link: extra operand`，在 `.cargo/config.toml` 中显式指定 linker 路径
- **代理设置**：
  - 推送到 GitHub 需要代理：
    ```bash
    set HTTP_PROXY=http://127.0.0.1:33210
    set HTTPS_PROXY=http://127.0.0.1:33210
    ```
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
- 当前只产出 CC Desk signed candidate artifacts；公开 GitHub Release/updater promotion 仍禁用，不得借 Gitee/OSS 绕过
- signed candidate 使用 CC Desk 自有 Tauri signing secret；私钥只存 GitHub Secrets，禁止提交到仓库、日志或支持包
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
| [docs/native-cli-v3.md](docs/native-cli-v3.md)               | **Native CLI v3 权威架构**：双 CLI、鉴权、输入输出、证据与发布边界        |
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
| [docs/release-process.md](docs/release-process.md)           | signed candidate、D20/promotion gate、签名与未来发布边界                 |

外部参考：[Claude Code 线上文档](https://code.claude.com/docs/llms.txt)

外部参考：[Tauri 2.x JS API线上文档](https://v2.tauri.org.cn/reference/javascript/api/)

## 约定

- 每次修改后，核心更新同步到 AGENTS.md，细节更新同步到 docs/*.md
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
- **树形项目会话管理测试**：`tests/stores/sessionTree.test.ts`（分组/排序/过滤/展开/多项目历史选择器 getHistoryFor）+ `tests/composables/projectTreeNavigation.test.ts`（resolveSwitchAction 切换语义 noop/activate/resume/new，D/E 参数直传无竞态）

### DevTools JSON 粘贴

- 实际粘贴不再自动压缩 JSON；只规范行尾。
- Windows 完整粘贴帧保持原始 `ESC[200~…ESC[201~` 字节，通过一次逻辑 `write_all` 提交；不再使用旧版 ESC INPUT_RECORD 改写或 pipe drain 补丁。禁止用 sleep 或单行化代替完整性保证。
- 前端各入口与生产 Rust writer 共享黄金样本；必须验证包含起止标记的完整正文。出现 Pasted text 折叠标签不算真实编辑器验收。见 docs/paste-framing.md。


### 粘贴诊断构建

- 0.17.3 诊断包通过 `paste_diag` 仅记录入口来源、UTF-8 字节数、字符数、LF 数、起止标记位置、writer 路径和耗时；严禁记录剪贴板正文。
- 日志位于 `~/.cc-box/logs/YYYY-MM-DD.log`，并包含版本与构建 SHA。用户复现后应截取 `Build identity` 和 `paste_diag` 行定位边界。
- 诊断包不得被表述为已修复版本；结果用于区分 WebView/IPC 未形成完整帧，与特定 Windows/Claude 对完整帧处理失败。


### Windows 粘贴架构约束

- 禁止把 bracketed-paste 的 ESC 改写为 Win32 INPUT_RECORD 序列；该协议要求
  `CSI ? 9001 h` 协商，不能由应用猜测。
- Windows 粘贴必须与 Windows Terminal 一致：完整、未经改写的 frame 通过
  一次逻辑 write_all 提交，不在 marker 或正文中插入 FlushFileBuffers 边界。
- Node raw stdin 不等价于 Claude Code 的控制台输入模式；发布门禁必须使用
  `paste_cli_submit` 捕获真实 UserPromptSubmit 正文。

### 实际窗口粘贴追踪（默认关闭）

- `pasteText.commitPaste` 在异步读取前分配事务编号；`api/tauri.ts` 在原有 `pty_input` 调用附加可选元数据；`paste_trace::pty_input` 包装器仍委托原 `commands::pty_input`，不改 payload、分块、shell 或重试行为。
- 仅诊断构建同时启用 `VITE_CC_DESK_PASTE_TRACE=1` 与 `CC_DESK_PASTE_TRACE=1`；首次粘贴后最多 60 秒、256 个输入事件、32 个前端 PTY 上下文。普通构建不附加参考正文。
- 规范化剪贴板参考文本只随同一次本地 IPC 在 Rust 内存中严格比较，不写日志；参考不超过 2 MiB 时提供 `exact` 与首次差异偏移，超出只标未知、不截断输入。日志只含编号、计数、布尔值，见 `docs/paste-runtime-trace.md`。
- `send_seq` / `recv_seq` 是 IPC 投递/接收顺序，不是 writer 锁获取顺序；诊断不是修复，仍需在受影响 Windows 环境定位，不能以发送成功替代真实草稿/提交完整性。

### D12 来源隔离（派生缓存基础）

- 派生名称索引 schema v2 使用 CLI、已验证 sourceRootKey、identityEpoch 和项目目录身份组成的缓存键；旧 v1 缓存重建，不跨根命中。未知目录身份跳过缓存，不猜测默认根。
- 旧 Claude 项目映射按来源根与路径分区，最多保留 64 个派生分区；显式根扫描不得覆盖默认根，失效操作清空所有分区。
- 配置面板请求用本地选择所有权拒绝迟到的成功/失败/finally；清空与切换立即隐藏旧配置，不记录原始异常载荷。
- `SourcePartition` 只是缓存身份，不是授权 SourceScope / 文件系统沙箱。鉴权读取已由下节 `native_get_scope`、`native_list_resources` 与 `cli/native_projection` 实现；不要重复实现或让新双 CLI UI 借道旧默认根读取。
- D11 的 `NATIVE_RUNTIME_NOT_READY` 保持关闭，详见 `docs/superpowers/execution/D12.md`。

### D12 authenticated native projections

- New native reads go through `native_get_scope` / `native_list_resources`, D11 document admission, and `cli/native_projection` held directory capabilities. A frontend path/owner or an opaque scope ID alone never authorizes a read.
- `SourceRef.basis` is an observation source, not effective CLI state. Keep shell/raw/unknown-argument roots unknown. Never authorize transcript cwd or plugin install paths outside a granted root.
- Return only the kind-specific projection DTO; do not add raw config/env/argv/headers to resource items or error logs. Scan failure must not remove registered projects.
- D22-D24 已完成 Native Claude/Codex UI adoption。新的双 CLI 代码必须继续使用 authenticated API/store，不得 fallback 到 legacy root/delete/PTY commands。
- Run the committed `tests/native-cli/scope-core` harness (actual production sources), frontend tests/build, and Windows production/live WebView tests. Headless core success is not real CLI or package certification.

### D13 observer isolation

- `observer_registry`/`observer_http`/`observer_host` own a bounded, authenticated optional side channel. `/hook` is no longer an unauthenticated compatibility path; legacy Claude launches also mint per-PTY leases.
- The native reservation winner adds verified observer plugin assets and a fresh capability to the frozen launch only when enabled. New profiles default off; raw/Codex/Shell never receive the Claude overlay. Strip only Desk capability environment names from ambient inputs, never user API credentials.
- Observer leases follow exact run/document lifetime, never own process control. Dropping/invalidating a lease cannot kill or restart a CLI. The `NATIVE_RUNTIME_NOT_READY` gate is unchanged.
- Only bounded allowlisted metadata reaches the owner WebView; prompt/assistant/error/env bodies and capabilities are not published. No sequence is invented for parallel Claude hooks: activity remains unknown even while the process runs.
- New UI consumers use exact-run `subscribeObservation(target, handler)` and its projected state, not raw event kind as a current activity claim. Native 与 legacy event topic 分离；D22-D24 的双 CLI UI adoption 已完成。
- Verification, recovery history and limitations: `docs/superpowers/execution/D13.md`; final-head CI evidence belongs in PR #20.

### Unified workspace session facade

- New Claude and Codex sessions use the Native adapter; historical resume retains the catalog runtime. Native operations never fall back to Legacy PTY commands.
- Scoped catalog refresh replaces only that project. Per-project request ownership prevents distinct concurrent refreshes or late full refreshes from dropping newer projections.
- Archived records remain in the addressable catalog, while normal project groups hide them. Legacy archive IDs retain compatibility; Native archive IDs include runtime, CLI, project and authenticated history session identity to avoid cross-source collisions.
- Native active archive requires an exact known history origin; ambiguous origins fail before process side effects. Execution evidence and remaining gates are recorded in `docs/superpowers/execution/unified-workspace-progress.md`.

### Shared UI interaction primitives

- New workspace/settings/project consumers reuse `src/components/ui/` controls instead of rebuilding menu, modal, tooltip or feedback behavior. Control sizes are `compact` 28px, `normal` 32px and `primary` 36px; GUI focus remains 2px ink-blue and selection remains amber.
- `AppMenu` takes controlled `open`, accessible `label` and typed `items`; hidden actions are omitted, danger actions sort last, arrow/Home/End navigation skips disabled items, and Enter/Space selects by ID. Escape restores the opener; Tab/outside pointer closes without stealing the destination focus. Consumers position the shared menu near their trigger and keep its content identical across context/overflow entry points.
- `AppDialog`/`AppDrawer` use controlled `open` (`update:open`, `close`) and a required title; modal focus is trapped by the top shared overlay and returns on close/unmount. Danger controls use `AppButton variant="danger"` or `data-danger="true"`; never mark them as initial focus. Tooltip default slots have exactly one trigger; `IconButton` requires its human-readable label.
- `useNotificationsStore().pushToast({ kind, messageKey, dedupeKey? })` accepts existing locale keys only, never raw Error/message payloads. Optional dedupe identities are bounded opaque action keys, never paths, credential values or transport text. Store copies only allowed fields, retains the latest three messages and expires short feedback after five seconds; the toast host pauses expiry for pointer/focus reading. Errors that require action stay in inline/page feedback.
- `ErrorDetails` accepts an error code and optional context, then renders only `mapSafeUserError`'s known diagnostic code behind native details. Resource/error consumers must not pass raw errors, arbitrary resource paths, env/header values or secrets through slots or notice strings.
- Interaction/store regression tests: `tests/components/uiPrimitives.test.ts`, `tests/stores/notifications.test.ts`; include `tests/designTokens.test.ts` and `npm run typecheck` for shared UI edits. Rendered Windows/scaling/localization matrices remain separate final visual gates.

### Shared session icon contract

- `SessionStatusIcon` consumes `SessionVisualState`; six bundled self-owned SVG contours distinguish starting/running/needs-user/confirming/ended/failed without row status text. It supplies localized `aria-label` and keyboard/pointer Tooltip through the shared `AppTooltip` actual trigger. Reduced-motion CSS overrides all three motion rules at matching specificity.
- `CliAppIcon` consumes `claude | codex`, shows self-owned neutral 16px marks with full CLI names in its Tooltip/accessible label, and keeps neutral image color independent of session state. GUI dark theme brightens the same neutral artwork for ≥3:1 contrast on supported row surfaces, including selected overlays. `CC`/`CX` appear only after the current SVG image fails; CLI changes clear the failure and detached image errors are ignored.
- Assets and ownership notes are in `src/assets/icons/cli/` and `src/assets/icons/session-status/`; no official-brand asset license is inferred. Regression gate: `npm test -- tests/components/sessionIcons.test.ts && npm run typecheck`. Windows/scaling and rendered-screen-reader checks remain final platform gates.

### Unified session row and menu contract

- `SessionItem` consumes only `UnifiedSession`, `selected`, an optional primary action and menu visibility. Its fixed five-column 38px grid reserves status/CLI/title/age-or-action/overflow slots; only the title ellipsizes. The one primary control overlays the age on hover/focus without resizing the title. No inline status/runtime/configuration identity text is rendered.
- `sessionPresentation.ts` owns the single typed secondary-action definition array. Overflow, pointer context menu and keyboard context menu render it through `SessionOverflowMenu`/`AppMenu`. Unknown/starting states cannot offer restart or archive; running archive is explicitly “Stop and archive”. Components emit action requests only; lifecycle, confirmation and diagnostic redaction remain caller responsibilities.
- `relativeTime.ts` supplies one reference-counted minute clock shared by all mounted rows and releases it after the final consumer. Compact ages use the existing formatter; keyboard/pointer Tooltip shows the full local activity timestamp.
- F2/menu rename edits only the title slot through `AppInput`; Enter/save commits the trimmed name, Escape cancels, and external saving disables repeated submission. Session identity changes discard the previous draft. Row controls reuse `IconButton` with the row-specific 20px width/28px height matching the fixed trailing column.
- `SessionList` is strictly unified and only forwards unified action/rename/activation requests. Its Task 8 temporary tabs/history boundary is removed. `menuTeleport` defaults true; archived drawer consumers set it false so the same menu remains inside `AppDrawer`'s modal focus boundary. Do not weaken the shared modal focus trap to accommodate menus.
- Regression gate: `npm test -- tests/components/sessionItem.test.ts tests/i18n/translations.test.ts && npm run typecheck`. Real Windows scaling, font geometry and rendered accessibility remain final visual/platform gates.
