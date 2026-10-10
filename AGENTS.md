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

> Native CLI v3 的权威边界见 [docs/native-cli-v3.md](docs/native-cli-v3.md)。旧 Claude runtime 通过统一 adapter 保持兼容；新的双 CLI 功能不得借道旧 PTY/API。

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
│   │   ├── shell/AppShell.vue   # 唯一全局外壳
│   │   ├── workspace/UnifiedTerminalHost.vue
│   │   ├── NativeCliTerminal.vue
│   │   ├── TerminalView.vue    # content-only Legacy terminal port
│   │   └── XTermTerminal.vue   # legacy terminal
│   ├── stores/
│   │   ├── unifiedSessions.ts
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

### Legacy Claude runtime（compatibility path）

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

- 统一树直接混排 Claude Code / Codex CLI；`ProjectNode` 和 `SessionList` 不再接收旧 tabs/history，也不依赖旧 PTY attention store。`SessionsPanel` 接收显式统一投影或默认读取 `unifiedSessions` store；adapter 初始化和生命周期 dispatch 由 `useUnifiedWorkspaceRuntime` 负责；`UnifiedTerminalHost` 保持一个 Legacy 聚合器及每个 Native 会话的独立终端常驻。
- 项目行只有一个新建快捷动作；固定/取消固定、重命名、查看归档、打开目录、移除走同一 `AppMenu`，鼠标右键和 overflow 共用动作。`new-session-request` 携带 projectKey/projectPath，不能交给旧容器的 `new-session` Legacy 启动 handler。
- normal `projectGroups` 保持排除已归档记录。UI 为仅剩归档的项目保留一个空项目壳以访问其归档菜单；面板全局归档入口在搜索无结果时仍可用。恢复仅发 `restore-request`，不直接恢复或启动，成功后由调用方发布 catalog 更新。
- 项目分组固定优先，分区内按显示名称自然字母排序，再按精确 projectKey 决定同名顺序；活动时间不能移动项目。普通、空和仅归档项目使用同一排序，项目内会话按持久化的最近打开时间排序；切换已打开会话或后台活动不能调序。
- 运行态 archive 必须发 `SessionTreeConfirmationRequest { kind: 'stop-and-archive', sessionId, projectKey, projectPath }`；未知/启动态不发归档请求。树组件不得直接 stop/archive，也不得 fallback 到 Legacy PTY；完整确认 UI 由 Task16 接入；normal App 的运行态关闭/归档在确认后仍须重新检查精确所有权，不能从目录旧快照推断已停止。
- 展开状态按 projectKey 显式保存；搜索临时展开但禁止修改手动状态。项目名/原 basename/路径命中展示组内普通会话，会话名命中只展示匹配的普通会话。清空搜索恢复之前手动状态；nested controls 的 Enter/Space 不触发父级折叠，菜单/编辑器 Escape 不关闭整个面板。
- 旧 `session.ts` 的历史缓存、Legacy PTY 所有权和兼容数据继续由统一 adapter 使用；项目管理只经 `ProjectsView` / `projectManagement`。旧历史删除接口不能删除 Native 记录。
- `projects.json` pin/archive/displayName/delete 增量写继续通过独立 `projects.json.lock` 跨进程锁、增量 apply 和原子返回状态；统一项目状态由 `projectsState` store 读取。Native workspace 保持自己的 revision/CAS 和 authenticated document bridge。
- 组件 gate：`npm test -- tests/components/projectSessionTree.test.ts tests/sidebarKeyboardHandlers.test.ts && npm run typecheck`。真实 Windows 1024×640、100%/125%/150% 缩放和渲染可访问性仍是最终平台门禁。

### 统一项目管理

- normal App 的 Projects 页使用 `ProjectsView` / `ProjectRow` 紧凑列表；`projectManagement` 合并真实注册、Legacy discovery、统一会话与 canonical `projectsState` 元数据。空项目也进入同一会话树，不启动 CLI。
- 添加目录使用不预注册的选择器，再经 `ensureNativeProjectRegistration` → 已有 `workspace.ensureRegistered` 采用规范化路径身份。注册仍遵循 trusted-main-window 的现有 profile-independent 契约；传入启动配置身份时才检查 CLI/修订绑定，不创建配置或使用另一 CLI 的配置。
- 隐藏只更新已有配置可见性集合，不注册 Legacy-only 项目。移除在明确确认后隐藏常规列表并取消已有 Native 注册、清理 pin；归档、显示名、启动偏好、CLI history 和项目文件保留，可在“显示隐藏项目”中找回或重新添加。Legacy-only 移除与隐藏具有同一持久化可见性，不能宣称永久忘记。
- 任何打开（包括 stopped、unknown 和未提交）/preparing 终端都会阻止隐藏或移除；进入变更时重新检查真实 owning stores。隐藏/移除期间按项目阻止新建与两种 runtime 的恢复 admission；Legacy恢复逐调用冻结屏障版本并在历史读取后检查，屏障结束也不能重新放行旧请求。隐藏在实际串行配置写入边界重新检查所有权，不隐式 close/stop。切换项目仅切换选择，不改变进程所有权。
- 配置可见性读取、启动 hydration 和写入按发布版本保护，较晚启动迁移不能覆盖新的确认写入。project mutation 冲突/未知回执仅重读，不自动重放或补偿；多存储变更可能部分完成，安全提示区分重新加载成功与无法重新加载。
- Task 14 gate：`npm test -- tests/components/projectsView.test.ts tests/stores/projectRegistrationFlow.test.ts tests/stores/nativeProjectRegistration.test.ts && npm run typecheck`。真实 CLI 和 Windows 平台验收仍独立未完成。

### 统一只读项目资源

- normal App 的 shell context slot 使用 `ProjectResourcesDrawer`，六类结构化资源绑定当前统一会话；不增加全局导航、第二个抽屉或配置写入器。无会话时不借用项目默认配置或 home。
- `projectResources` 优先冻结的 Native run/generation；仅持有该 request/run/generation 的正向未开始证明且 stopped/null 时接受精确 profile revision/注册项目；`nativeTabs` 在开始准备前撤销该证明，failed/null 回执不等于未提交。历史会话另用冻结来源。无效、撤销或消失的 run 不回退当前默认配置。跨会话/attempt/CLI/配置/分类旧完成不能发布；仅同一 owner 刷新保留过期内容。
- `nativeProjection.readScoped` 通过原 authenticated client 独立读一个有界页，不与兼容面板共享结果槽。`hasMore` 明示 partial，不发明分页快照完整性。typed display DTO 使用配置 allowlist、固定来源标签及保守自由文本筛查，不输出 env/header/凭证、路径、原始错误或 JSON dump。
- Legacy 只读 stores 仅返回显式绝对项目路径的逐请求结果；settings/MCP 使用现有 config DTO 的 source.path 精确匹配项目固定配置文件，缺失/父目录/其他项目来源均省略；不能使用包含祖先记录且无路径的 getAllMcpServers project 标签。项目插件还须精确匹配 projectPath。ambient user/global/plugin 子资源省略并明示 project-only/partial；说明文档无现有契约，保持 unavailable。
- Task 15 gate：`npm test -- tests/components/projectResourcesDrawer.test.ts tests/stores/projectResources.test.ts tests/native-cli/projectionScope.test.ts tests/productBoundary.test.ts && npm run typecheck`。实际 CLI、平台缩放/渲染和终端连续性仍须独立验收。详见 [docs/project-resources.md](docs/project-resources.md)。

### 性能边界

- `get_home_data` 单次扫描 `~/.claude/projects`，同时生成项目列表与真实路径映射；近期会话直接复用该映射的目录列表，`get_home_data` / `get_sessions` 的同步文件 IO 统一放入 `spawn_blocking`。
- JSONL 按字节行流式解析：项目路径在首个有效 `cwd` 后停止，名称继续扫描到 EOF 以保留末尾 `custom-title` 优先级；峰值内存为 O(最大 JSONL 单行)。首页、项目历史和 all-recent 共享 `~/.cc-box/session-name-index.json` 派生名称索引：`length + 高精度 mtime` 完全一致才 exact-hit（读取 0 JSONL bytes），任一变化都 full rebuild，不使用 append cursor。
- 名称索引每个请求只读一次快照，前台共享锁内只读取有界 raw bytes、锁外解析；业务值先返回，delta 在 detached `spawn_blocking` 中写回。后台使用 replacement stamp 复核、entry/bucket CAS、整文件 raw CAS 和唯一临时文件；排他锁内只做 64 KiB 分块 raw compare 与原子替换。8 MiB 以上压缩至 6 MiB，16 MiB 为读取硬上限；损坏、未知版本、锁/写失败均只降低命中率，不改变业务结果。
- `SettingsView` 首次进入时异步加载，之后在统一主内容列保持挂载；复杂编辑器使用共享对话框。WebGL 仅在新终端启用 WebGL renderer 时动态加载，DOM renderer 仍为默认。

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
- 公开发布只允许经过授权的 protected-main promotion；不得借 Gitee/OSS 绕过门禁
- signed candidate 使用 CC Desk 自有 Tauri signing secret；私钥只存 GitHub Secrets，禁止提交到仓库、日志或支持包
### 版本/发布边界（授权恢复：protected main promotion）

- 普通开发 PR 只跑验证，不做版本 bump/tag/release。
- `.github/workflows/release.yml` 只从当前 protected main 的精确 SHA 构建并发布同一 workflow run 的三个平台产物。
- `scripts/release-policy.mjs` 必须保持 fail-closed：版本一致、当前 protected main 的强制 CI 检查成功、tag/release/draft 无冲突，并在发布前重新检查。用户允许无法自然验证的宿主专项不阻断完整功能发布；仅源码审核的 18 个 Job-free 测试可在实际外部 Job 中明确列为未验证，其余原测试库存必须执行并核对。发布须验证同 SHA/run/attempt 的精确 CI artifact 实际字节、报告及原始日志；不得把正常套件成功宣称为原 All 或真实往返验收通过，不能使用调用者本地 JSON 或任意排除项替代证据。
- 这次恢复依据用户明确授权；普通 PR、feature branch、失败或未完成的 CI 均不可发布。
- 真实 Claude Code / Codex CLI Layer-C 证据属于 D20，和普通代码 CI 分层记录。
- updater 必须验证三个实际文件的签名；既有 draft 替换要求 hash-verified backup 和明确 recovery transaction。客户端自动安装策略保持不变。
- 0.18.1 精确恢复只认旧 draft `406663556` / source `5ed35db9a560e093a91a5ef32a1eb6dd171f27fd` 和五个固定资产。新三平台验签后先上传并独立下载复核同 run/attempt 备份，再将同一 draft 的 target_commitish 更新为实际 current main SHA、保留同一 v0.18.1 tag_name。旧五个资产只改为独立标注的 preserved .bin 名称，原 ID/字节保留；新九资产完整 staging、回下载核对哈希和三个 updater 签名，再重查 current main/CI/coverage 后发布原 draft ID。没有 archive tag、资产删除或自动重放未知回执。普通 unused-version gate 保留，read-only candidate recovery 不构成发布 waiver。详见发布流程。
- 用户另授权对实际部分准备状态进行精确重新准备：只认 `scripts/release-prepared-recovery.json` 固定的完整旧快照、db517757/37899773556/attempt1 marker 和原备份 artifact11604434521 的不可变 ID/名称/大小/SHA。独立复核原备份 JSON/五资产字节与当前 snapshot 后，可保留五个原资产，将同一 draft 绑定新的实际 protected main 和新 release run 的完整九资产清单。旧失败 run/旧 CI/旧签名只证明旧状态来源，不得作为新 main 的通过证据；新完整 CI/coverage 和新同 run 三平台实际签名必须重新验证，最终 staging/bytes/signatures/source/tag 检查不变。未知写入不重放，未知/改变/已增加资产的部分状态仍拒绝。

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

- **CI 性能与来源**：Windows 普通 Rust CI 一次编译四个原始 harness，以同 source SHA/run/attempt 绑定的编译包确定性分为 16 片；`Rust checks` 在全片成功、完整名称与原始忽略集合及原始输出计数核对后才通过。18 条 Job-free 专项仍按原政策披露为未验证，不增排除。roundtrip 的普通 release 与惰性 debug 场景并行，原 required policy 聚合全部成功。发布候选可在同源 CI 等待期间构建，正式发布仍受原完整来源、coverage 与平台签名门控。详见 [docs/ci-performance.md](docs/ci-performance.md)。 分片必须流式保留原始输出和未完成名称诊断；单 harness 20 分钟、job 30 分钟上限超出后失败并保留部分证据，不能把 timeout 或先打印的成功摘要计作完整通过。

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
- `AppTooltip` portals its passive description to `body` above modal backdrops so transformed/overflow ancestors cannot clip viewport coordinates. The original trigger retains focus, hover/Escape behavior and `aria-describedby`; tooltips never join the modal focus stack. New-session Create stays in the shared non-shrinking footer with native form association and rejects repeated submission after the draft closes. All 13 Linux Chromium baseline PNGs were independently inspected and accepted from artifact `11153226557` (source `d90ec90`, tested merge `2ce9e195`); exact hashes are in `tests/visual/__screenshots__/approval-manifest.json`. The final no-update gate, Windows manual acceptance and D20 real CLI certification remain pending; see `docs/visual-testing.md`.
- Interaction/store regression tests: `tests/components/uiPrimitives.test.ts`, `tests/stores/notifications.test.ts`; include `tests/designTokens.test.ts` and `npm run typecheck` for shared UI edits. Rendered Windows/scaling/localization matrices remain separate final visual gates.

### Shared session icon contract

- `SessionStatusIcon` consumes `SessionVisualState`; bundled self-owned SVGs use solid circular backplates with contrasting semantic marks, including the preserved activity details, without row status text. It supplies localized `aria-label` and keyboard/pointer Tooltip through the shared `AppTooltip` actual trigger. Known starting/working breathe slightly and thinking dots brighten sequentially. Permission/input and completion entry animate once only on actual state changes, never selection/refresh/remount. Reduced-motion disables all animation.
- `CliAppIcon` consumes `claude | codex`, shows corresponding Claude and Codex application marks at 16px with full CLI names in its Tooltip/accessible label. Brand artwork remains independent of session state; Claude retains its official starburst color, and Codex uses theme-appropriate monochrome contrast. `CC`/`CX` appear only after the current SVG image fails; CLI changes clear the failure and detached image errors are ignored.
- Assets and provenance/ownership notes are in `src/assets/icons/cli/` and `src/assets/icons/session-status/`; third-party application marks are attributed separately from CC Desk-owned status artwork. Regression gate: `npm test -- tests/components/sessionIcons.test.ts && npm run typecheck`. Windows/scaling and rendered-screen-reader checks remain final platform gates.

### Unified session row and menu contract

- `SessionItem` consumes only `UnifiedSession`, `selected`, an optional primary action and menu visibility. Its 38px grid reserves status/CLI/title/age-or-action/overflow slots; only the title ellipsizes. Closed-history launch/retry and archive-restore controls are always visible beside age, while the open-row Close overlay keeps its existing hover/focus geometry. No inline status/runtime/configuration identity text is rendered.
- `sessionPresentation.ts` owns the single typed secondary-action definition array. Overflow, pointer context menu and keyboard context menu render it through `SessionOverflowMenu`/`AppMenu`. Unknown/starting states cannot offer restart or archive; running menus do not offer archive. The underlying stop-and-archive confirmation contract remains for existing typed requests. Components emit action requests only; lifecycle, confirmation and diagnostic redaction remain caller responsibilities.
- `relativeTime.ts` supplies one reference-counted minute clock shared by all mounted rows and releases it after the final consumer. Compact ages use the existing formatter; keyboard/pointer Tooltip shows the full local activity timestamp.
- F2/menu rename edits only the title slot through `AppInput`; Enter/save commits the trimmed name, Escape cancels, and external saving disables repeated submission. Session identity changes discard the previous draft. Row controls reuse `IconButton` with the row-specific 20px width/28px height matching the fixed trailing column.
- `SessionList` is strictly unified and only forwards unified action/rename/activation requests. Its Task 8 temporary tabs/history boundary is removed. `menuTeleport` defaults true; archived drawer consumers set it false so the same menu remains inside `AppDrawer`'s modal focus boundary. Do not weaken the shared modal focus trap to accommodate menus.
- Regression gate: `npm test -- tests/components/sessionItem.test.ts tests/i18n/translations.test.ts && npm run typecheck`. Real Windows scaling, font geometry and rendered accessibility remain final visual/platform gates.


### Unified application shell boundary

- `useShellStore` owns exactly `workspace | projects | settings`, logical responsive mode and global column state. `AppShell` is the only normal global layout: 44px navigation, 288px session tree (240–360), flexible main, optional344px resources (300–420). Do not recreate global shells in content views or add another tab strip.
- Context overlays below1180 CSS pixels through shared `AppDrawer`; sessions can collapse below900 while preserving the separate desktop choice. Read `window.innerWidth`, not physical scale. Native default/minimum is1024×640; min-width zero/overflow-hidden containers avoid global horizontal scroll under scaling.
- Every build of `App.vue` mounts only `AppShell` and loads app/GUI preferences independently of CLI availability. `useUnifiedWorkspaceRuntime` configures the real adapters and performs read-only bootstrap; no old automatic Legacy startup or alternate DEV product route remains. The retired compatibility flag has no effect.
- Task9 typed tree requests retain exact catalog/project identity through `SidebarPanel`. `new-session-request` must never map to old `newSession`/Legacy launch events. `WorkspaceRequest` is presentation-only; the latest ephemeral intent and sequence require explicit runtime-owner handling, never automatic replay. Clearing an older sequence cannot discard newer intent.
- `WorkspaceView` and its one `terminal` slot stay mounted across navigation; Task11 owns unified adapter admission, Native authenticated runtime ports and actual terminal hosts. Projects uses the real project-management view; shared new/resume/confirmation dialogs, scoped resources and all seven Settings sections are connected to their owning stores. Resource context stays read-only and must not accept old default-root projections or raw transport errors.
- `TitleBar` consumes the unified context title, ellipsizes long text, and preserves OS window controls. GUI theme updates change neither terminal preferences nor session selection. OS menu Settings/Shortcuts route safely to the one Settings section; directory/restart events only request actions until runtime integration.
- Exact shell gate: `npm test -- tests/components/appShell.test.ts tests/productBoundary.test.ts && npm run typecheck`. Native bridge safeguards remain mandatory. Actual Windows 100%/125%/150% scaling, rendered1024×640 layout and platform/accessibility screenshots remain separate final gates.

- Persistent tree surfaces must receive explicit `active`/`surfaceActive` ownership from the shell. Leaving Workspace or hiding the session column closes its archived modal, project menu and session menus, including teleported content, without discarding search, expansion, inline rename state or terminal hosts. Optional Boolean activity props default true at every standalone list boundary; do not let Vue's absent-Boolean false disable independent/drawer consumers. Shared menu/modal primitives retain their existing focus behavior.

### 快捷新建会话（Task 12）

- 项目 `＋` 和工作区普通新建按钮都先使用 `NewSessionMenu`，更多选项才打开高级对话框；选择 Claude/Codex 后由 normal App → shell → `useUnifiedWorkspaceRuntime` → unified catalog → Native adapter/host 执行；不走旧 PTY。
- `newSessionDraft` 保存 UI 草稿与独立 CLI 默认选择，并消费 `projectsState.launchPreferences` 的项目+CLI 最近成功配置；成功写入只经过 `setLaunchPreference` / projects.json 单一 writer。偏好顺序为项目+CLI 最近成功配置、CLI 默认配置、显式创建时的安全默认配置；只有匹配 request/run/generation 的 `running` receipt 才记录成功，不把 tab admission 或文件系统 preflight 当成真实 CLI 成功。
- 创建先插入 catalog 占位行，再异步准备配置/注册。`workspace.ensureRegistered` 是 Task14 可复用的显式创建前置步骤；bootstrap 仍只读。配置使用既有 patch/CAS；冲突/未知提交只 reload、不自动重复写或启动。失败占位保留并支持明确重试；取消后迟到完成不能启动，unknown 不重放。
- 高级对话框按基本/更多/开发者选项纵向组织。权限仅描述 Desk 配置的标志并说明实际行为仍受已保存 argv/CLI 设置影响，不承诺实际权限模式，不提供不存在的 per-launch override，不隐式改写已有配置。Raw argv 默认逐行、JSON 显式切换，内部始终精确 `string[]`。
- 恢复选项发出 `restore-session { project, cli?, mode }`，由 Task13 的统一恢复对话框处理。完整错误映射与运行态确认已由 Task16 接入。
- 菜单/对话框使用共享 AppMenu/AppDialog，并随所属活动界面失活而关闭。详细接口与门禁见 `docs/components.md` 和 `docs/terminal-integration.md`。

- Task12 review repairs：pre-ready 创建后的本地会话取消/选择不依赖无关来源 bootstrap；占位行重新选择在 admission 时按最新选择所有权转移到 Native 行，不抢占更新选择。未知偏好写入确认在 projectsState writer 队列内只读恢复，恢复失败使快照失效，后续写入必须重新读取成功或停止。

### 统一恢复与历史搜索（Task 13）

- 普通 App 的快捷恢复、历史行激活/恢复和高级恢复意图共用 `ResumeSessionDialog`；恢复前显式确认，已打开的同源会话只切换现有尝试，包括 unknown 状态，绝不自动重启。搜索支持标题、Session ID、CLI、时间和当前/全部项目；旧请求或失活窗口不能发布结果、抢焦点或迟到启动。
- Native 历史目录身份包含 CLI、配置 ID/修订、注册项目 ID/路径与 `sourceSessionKey`。恢复冻结原来源，并在异步历史检查后再次查找已打开的尝试；不以当前配置替换历史配置。完整历史分页走既有 authenticated document bridge；失败/部分来源不证明会话不存在。
- 按 Session ID 和 CLI 自带恢复列表要求显式已有启动配置和已注册项目，冻结所选配置修订/注册身份，直接进入现有 Native adapter/runtime；不得调用新建准备逻辑、自动注册或创建配置。来源已变更/缺失时提示刷新并重新选择。
- 缺失记录移除先重新验证来源/不存在证据，再经 `projectsState` 移除精确可选 UI 元数据并清理当前目录，不调用 `deleteSessions`，不删除 CLI 文件、不创建永久忽略标记。以后成功发现的真实记录可以重新出现。
- 原 Native archive key 唯一匹配时继续兼容，显式恢复可移除该旧键；多来源歧义时保留元数据并提示，不猜测。新归档使用完整来源键。Legacy 普通历史按项目过滤占用/归档；`getCatalogHistoryFor` 始终保留归档供统一目录处理。
- 门禁：`npm test -- tests/components/resumeSessionDialog.test.ts tests/stores/unifiedResume.test.ts tests/native-cli/nativeProjectionStore.test.ts && npm run typecheck`。真实 CLI/平台缩放验收仍未执行，见手动测试清单。

- Task13 review repair：Native 不存在证据必须来自同一原始 `sourceRootKey` 的单次完整鉴权响应，并与保存的 `sessionKey` 匹配。offset 多页没有共同快照契约，只可提供正向发现，不能证明缺失/允许移除；大历史中无法证明缺失的记录需保留并提示不确定。合并恢复请求分别保留每个调用方的取消所有权，新显式确认可接管尚未完成的检查；旧已取消调用方仍拒绝，全部取消时不准入。

### 统一确认与安全反馈（Task 16）

- normal App 的停止并归档和未知状态重启通过类型化 `SessionConfirmDialog`；0.18.3 的普通关闭按用户授权直接执行。打开对话框只冻结目标，不执行进程操作。确认或直接关闭均固定 Native request/run/generation、CLI/配置修订/项目/来源/启动动作，或 Legacy Tab/PTY/generation/项目/Session 身份，并在异步边界重新检查。导航、项目/会话切换和新意图撤销旧确认，不能把旧错误或完成提示附到新选择。
- 未知状态重启只授权既有 exact recover/stop 契约：仍未知或未确认停止时保留原尝试；只有状态已知且旧进程确认结束才分配新 generation。Native 不借道 Legacy PTY。Legacy 运行态关闭先 await 停止；异步重命名也重新检查原 PTY 所有权。
- `ProjectConfirmDialog` 复用项目移除和启动配置删除展示。项目移除保留 Task14 admission/visibility 屏障、原注册身份和不删除文件边界；取消后不能继续后续写。配置删除由 `cliProfiles.requestDelete` / `confirmDelete` 提供真实 CAS 契约，冻结配置与工作区 revision，并阻止删除期间新 Native admission。Task19 已接入真实编辑器，并仅在准确启动回执证明身份冻结后允许删除运行会话的保存配置；未准入/准备中的会话仍阻止删除。不能用 `patch({ op: 'delete' })` 绕过确认。
- `userError` 仅以 own-property 白名单映射固定键/代码，原始异常与原型继承键不进入渲染状态。普通重试错误内联显示，单个 CLI 故障用工具级横幅；只有全部来源不可读、无缓存和打开会话时显示工作区错误页，终端宿主不卸载。复制、重命名、归档、项目固定/移除和配置删除只在已确认完成且上下文仍匹配时发短 Toast。
- 冲突/未知写回执只读重新协调，不自动重复 mutation/启动/输入。`projectsState` 是索引元数据唯一 writer，归档/恢复等失败也在其队列内只读恢复。配置删除在自己的 mutation 队列内恢复；配置变化后旧确认不能借用新 revision，需重新检查并明确确认。
- 精确门禁：`npm test -- tests/components/interactionFeedback.test.ts tests/stores/staleActionFeedback.test.ts tests/utils/nativeErrorCode.test.ts && npm run typecheck`。实际 CLI、Windows/macOS/Linux 渲染和缩放验收仍独立记录，不能以宿主测试替代。
- Task16 确认的准入检查必须穿透实际 writer 队列：`workspace.remove` 在 CAS 调用前检查原确认/注册/打开会话；`projectsState.archiveSession` 与项目移除的 `unpinProject` 在初始读取和队列等待结束后执行同步检查。Native/Legacy adapter 都把准确 owner 检查传给 canonical writer；未准入取消不当作未知写、不触发 writer 恢复或错误。已经发出的停止/隐藏/注销不补偿，后续未发出的步骤停止。


### Unified settings shell (Task 17)

- Normal Settings has exactly `general | appearance | terminal | launch-configurations | shortcuts | update | about`; compatibility `startup` navigation maps to General. OS Settings opens General; OS Shortcuts opens Shortcuts. Terminal preferences are implemented in Task 18; configuration editing is implemented in Task 19; shortcuts/update/about are implemented in Task 20 with the current release-policy limitation below.
- General owns interface language, next-start Workspace/Projects destination and the chooser/form's default CLI. Startup never starts/restores a CLI; a delayed load cannot override a newer navigation, including a same-destination click. Closing the window is described only as the existing application exit; no tray preference is invented. Legacy continue/permission/argument/IDE/env keys remain stored and retain their compatibility contracts.
- Appearance controls GUI light/dark/system mode, standard/compact density and real session sidebar width 240–360 (default 288). The existing ink-blue focus/amber selection accents stay fixed. GUI theme mode resolves to the existing light/dark DOM contract; terminal theme/font/renderer, active selection and terminal host identities remain independent. System-theme listeners have store-scope cleanup.
- Simple settings use one optimistic, serialized writer with per-field intent/confirmed-commit ownership. Known persistence failure rolls back only the latest field intent to its last confirmed value; stale results and hydration cannot roll back a newer choice. Startup migration shares this submission lane. Unknown acknowledgements only obtain a fresh read within the lane; they are not resubmitted, and failed recovery blocks subsequent writes until an explicit operation can read the saved state.
- New optional typed AppConfig fields are `guiThemeMode`, `guiDensity`, `sidebarWidth`, `startupDestination`, `defaultNewCli`. Raw incremental config writes preserve old/future stored keys instead of round-tripping the read DTO. No new configuration bridge or runtime dependency is introduced.
- Task 17 gate: `npm test -- tests/components/settingsShell.test.ts tests/stores/app.test.ts tests/i18n/translations.test.ts && npm run typecheck`. Actual Rust execution, Windows/macOS/Linux rendering/scaling and real CLI continuity require their separate recorded gates.


### Shared terminal preferences (Task 18)

- Settings → Terminal contains all 16 existing theme cards, a static non-PTY preview, font stack selection, size (10–24), line height (1–2), cursor style/blink and renderer preference. Preview uses ordinary inert markup and never imports or opens a terminal/CLI bridge. Existing `cc-box-*` and third-party theme IDs retain their colors; GUI-based fallback is initial legacy migration only, not a continuing theme dependency.
- `app.terminalPreferences` is the single computed preferences object consumed by Legacy Claude, Native Claude and Native Codex. `config/terminalPreferences.ts` owns normalization, platform CJK/emoji fallbacks and xterm appearance deltas. Color and cursor changes mutate existing options only. Metric changes coalesce into one fit for the visible terminal; hidden/minimized/closed ownership is rechecked at the frame boundary and hidden metrics fit when shown. No terminal/PTY recreation, launch/input replay, scrollback reset or selection mutation is performed by these changes.
- Renderer preference applies only to newly created terminals, explicitly labeled in Settings. Both runtimes use the frozen creation-time choice; WebGL loading/context failure retains the same terminal/theme options and DOM fallback. Native optional-addon callbacks check terminal lifetime; Legacy keeps its existing raw/proxy-safe registry and reload cleanup.
- All seven terminal fields now extend the Task17 serialized simple-settings writer, including per-field intent/commit fences, confirmed rollback baselines, shared-read origin and unknown-result read barrier. Startup theme migration rechecks its field intent, commit and read sequence at submission; it cannot overwrite a newer terminal choice or recovery snapshot. Old `terminalTheme`, `fontSize` and `webglRenderer` keys remain compatible; new read DTO fields are optional `terminalFontFamily`, `terminalLineHeight`, `terminalCursorStyle`, `terminalCursorBlink`, and the formerly missing `webglRenderer` read field.
- Exact gate: `npm test -- tests/components/terminalSettings.test.ts tests/config/terminalThemes.test.ts tests/components/terminalThemeContinuity.test.ts && npm run typecheck`. Rust DTO tests are authored but NOT RUN in this cloud environment (cargo/rustc unavailable); no host/real-CLI/platform acceptance is implied. Details: [docs/terminal-preferences.md](docs/terminal-preferences.md).


### 启动配置设置（Task 19）

- Settings 的启动配置列表按 Claude Code/Codex CLI 分组，显示应用图标/名称/默认标记；每行只有一个编辑快捷按钮，其余复制/重命名/设为默认/删除共用 AppMenu。复杂编辑使用 AppDialog 和明确 Save/Cancel，字段分为基础、更多和开发者选项；环境仅显示名称与 set/unset/inherit 状态，现有值和 host reference 从不进入表单/DOM。
- 编辑器冻结 workspace revision 与原配置 revision，`cliProfiles.saveConfiguration` 在既有 writer 队列实际准入时检查当前编辑所有权和修订，冲突/未知写只读恢复、不自动重试；关闭/切换后迟到完成不能关闭新编辑器或发布旧反馈。argv 每行一项，不 trim/shell 拆词；含换行或唯一空参数自动使用精确 JSON。
- 删除继续走 Task16 类型化确认/CAS。`nativeTabs.hasFrozenLaunchReceipt` 仅认可准确 run/request/generation/来源的正向回执；未准入、准备中或无可信回执的未知尝试阻止删除。统一准备尚无 Native Tab 时也保留屏障。删除保存配置不停止/重启/改写已准入 run；资源继续使用 backend 保留的 run snapshot，不退回当前默认配置。未来重启/历史恢复仍须原配置存在且修订匹配，否则明确失败。
- 全局默认仍由 `newSessionDraft` 的既有本地偏好管理，项目最近成功配置仍由 `projectsState` 写入。删除默认后选择同 CLI 现有配置；删除最后一个仅清除失效全局偏好并显示安全默认提示，真正创建延后到下一次明确 New session 的既有准备流程，不修改历史来源或项目偏好。
- Gate：`npm test -- tests/components/launchConfigurations.test.ts tests/stores/cliProfiles.test.ts && npm run typecheck`。真实 CLI、Rust/平台认证和 Windows 缩放验收未由此任务执行。


### Remaining settings and shortcuts (Task 20)

- Normal App uses the five canonical configurable bindings in `config/appShortcuts.ts`: Mod+N/W/P/comma and F2 by default. N uses the actual unified project and existing chooser/add flow; W requests the existing exact-owner close/confirmation path; P/Settings use the real shell. No Legacy PTY/restart fallback is invoked. Focused session rows receive the same rename binding by injection; standalone rows keep F2. Global capture excludes ordinary edit fields, IME/repeat/extra modifiers and active shared dialogs. Terminal helper inputs honor configured application bindings.
- Shortcuts search covers localized action and displayed keys. Shared capture dialogs do not overwrite collisions: explicit Replace unassigns only the frozen conflicting action, after a latest-map check. Per-item default conflicts use that same explicit choice; all-reset has shared confirmation. The optional `shortcutBindings` AppConfig field joins the Task17/18 serialized writer and read/commit/recovery fences. Editing first hydrates the saved map; unacknowledged group writes and failed unknown recovery block the editor until read-only reconciliation. Late callbacks cannot close or publish errors into newer dialogs.
- F2 reveals the selected editor without changing project/session/process selection. The catalog retains editing/saving ownership across same-source refreshes, including collapsed or search-hidden rows; cancellation, disappearance, full source/origin changes or adapter attempt invalidation release it and discard the old draft. Queued rename rechecks that owner before dispatch, saving rows reject new editor admission, and canonical rename/stop/close continue through existing adapters. The tree remains the only session tabs.
- Update UI separates stable/candidate/test labels and unverified observations. Existing artifact product/channel/non-publication markers remain negative exclusion evidence only. The user-authorized 0.18.2 updater uses the existing public official release mechanism: exact version/tag/source and source trust key, uploaded package/signature asset identity and SHA-256, and the Tauri Minisign check. A backend-held one-shot admission owns the actual Update; frontend plugin install permissions are absent. Installation rechecks proof and shared admission quiescence, blocks both runtime starts/mutations, and never replays an unknown installer result. Independent optional HTTP/HTTPS updater proxy settings never import or modify CLI launch environments. Public errors use fixed code/stage; upstream updater raw logs are redacted. Endpoints, signing key and publication workflow remain unchanged. Platform install/restart and real CLI clipboard acceptance require separately recorded evidence.
- About displays compile-time version and Git HEAD commit (or honest source-archive absence), product positioning, MIT and the existing official repository/CLI docs links. Diagnostic clipboard output is a new structured allowlist of validated version/commit/platform, enum/bounded appearance fields and numeric owner counts. It contains no identities, paths, environment/credentials, prompt/output bodies, raw errors or arbitrary object spreads. Only an acknowledged current clipboard write receives a toast.
- Exact gate: `npm test -- tests/components/remainingSettings.test.ts tests/composables/appShortcuts.test.ts tests/stores/update.test.ts && npm run typecheck`; affected shell/runtime/settings, API/channel-exclusion, localization and token tests remain mandatory. Rust/actual CLI/platform/scaling and updater install/relaunch are NOT RUN by this task. Task21 retired the development compatibility route and its old shortcut consumer; only the canonical unified action routing remains.

### Unified surface retirement (Task 21)

- `NativeCliWorkbench`, `LegacyCompatibilityApp`, old Welcome/ProjectSelect, IconBar/TerminalHeader, SettingsOverlay/StartupSection and the orphan Legacy resource panels are removed after caller tracing. The project/session tree is the only session tabs, and resources use six structured read-only context views. No raw resource JSON or separate Native product page remains.
- `nativeWorkbench.ts` had no caller outside the retired page and is removed; `useUnifiedWorkspaceRuntime`, `nativeTabs`, `cliProfiles`, `cliWorkspace`, both adapters and authenticated API/runtime helpers remain authoritative. Runtime/storage names do not imply a second UI.
- `TerminalView` is only the Legacy aggregate port: explicit start/stop/restart/rename/recover, focus/fit and `useStatusMonitor` remain; nonembedded navigation, implicit startup/history/config reads on mount and old launch event listeners are gone. An exact-PTY start still adds the owned project and refreshes new-session history; stale events cannot use a global cwd fallback. `XTermTerminal` core subscriptions, PTY ownership, hidden parsing/protocol replies and all Native IO are unchanged.
- Boundary tests now inspect the actual terminal and structured resource consumers for inert DOM, no payload logging, projection-only APIs and no Legacy PTY fallback. Gate: `npm test -- tests/productBoundary.test.ts && npm run typecheck && npm run build`; affected adapter/terminal/shell tests also run. Rust, D20 real CLI and final platform/visual acceptance remain separate and unperformed by this cleanup.

### Responsive and accessibility contracts (Task 22)

- Shell decisions use logical CSS viewport width: `<1180` overlays resources, `<900` keeps a separate collapsible-sidebar choice; DPR must not be applied a second time. The contract matrix covers 1024×640, 1280×720, 1366×768, 1440×900 and 1920×1080 with DPR 1/1.25/1.5, zh/en and light/dark. These are jsdom/static inputs, not rendered Windows scaling acceptance.
- Session status/CLI/title/time/menu columns remain present with 200-character titles and 80-character project names; only text columns truncate. Shared menu/footer labels wrap without reducing compact/normal/primary minimum control heights. Tooltip placement uses measured trigger/tooltip bounds in viewport-fixed coordinates, updates on resize/ancestor scroll, and retains existing focus/hover/Escape descriptions.
- Shared dialog Tab/initial focus excludes closed-details content while retaining its first summary. Focus return waits for the navigation DOM commit, refuses hidden/inert/disabled openers and newer modal/destination ownership, and may return to a visible parent container with tabindex=-1. Settings inactivity still closes its actual editor rather than retaining a hidden modal.
- Locale checks inspect source object keys before duplicate overwrites and require literal keys referenced by active UI surfaces in both languages. Reduced-motion and focus-ring CSS strategies are source contracts; actual pixel geometry, fonts, pointer hit areas, screen-reader behavior and OS scaling remain Task23/platform gates.
- Exact gate: `npm test -- tests/components/responsiveLayout.test.ts tests/components/accessibilityContracts.test.ts tests/i18n/translations.test.ts tests/designTokens.test.ts && npm run typecheck`. No CLI/platform/Rust certification is implied.

### Isolated visual regression harness (Task 23, BLOCKED_VISUAL)

- `src/visual/VisualFixtureApp.vue` composes actual production shell/tree/project/resource/settings/dialog controls with fabricated DTOs and the static non-PTY terminal preview. It never imports normal App/runtime startup. `build/visualFixture.ts` requires serve + visual mode + explicit flag; normal dev rejects visual modules/routes and production has no visual alias/entry. Only the dedicated fixture module graph replaces Tauri imports with a counted, fail-closed stub.
- Playwright has 10 required named snapshots, three supplemental states, a 120-case viewport/DPR/locale/GUI/density geometry matrix and keyboard/focus/overlay/tooltip checks. Time and screenshot motion/caret controls are test-scoped; production layout/overflow/focus styles are not hidden or masked. `test:visual:unit` runs the separate fixture graph; ordinary Vitest excludes that directory and retains actual API boundaries.
- Shared browser setup actions have real-fixture DOM regressions: project-toggle locator membership stays stable across expansion and the session row gains focus before clicking its overflow button. DOM ordering evidence does not substitute for rendered hit testing or screenshot acceptance.
- Task23 rendered acceptance is BLOCKED: official browser downloads returned invalid ZIP content, installed Chromium could not create its required socket, and the managed browser rejected the loopback preview. No baselines were generated; missing-baseline RED, pixel inspection and no-diff PASS remain pending. Linux Chromium with Windows-style UI/DPR is not native Windows scaling or WebView2 acceptance. See `docs/visual-testing.md` for exact commands and limits.

### Migration and adversarial host coverage (Task 24)

- Unified row rename is Desk display metadata only. Both adapters consume `projectsState.sessionRecords`, and normal history/live rename uses its queued writer; Legacy row rename must never inject `/rename` or other terminal input. Saved names overlay existing discovered/owned records only, never create sessions or authorize source access.
- Native historical names use the full existing history catalog key (CLI, configuration ID/revision, registered project/path and authenticated source session key). Resumed live names use that same known source identity. A new/raw tab with no authenticated historical association keeps an exact tab-key name; later unrelated discovery cannot inherit it by raw Session ID, title or current defaults. Legacy names bind their project/session identity. Persisted record fields must agree with the catalog key's projected identity.
- Rename ownership is checked again at actual `projectsState` queue admission and after receipt. Invalidated history, replaced Native attempts and Legacy PTY generations cannot issue a queued metadata write. Pre-admission cancellation triggers no unknown-write reconciliation; issued conflicts/uncertain writes reconcile read-only without replay. Optional session/launch metadata containers and individual entries are validated independently with existing backend bounds, preserving valid siblings.
- Gate: `npm test -- tests/stores/unifiedMigration.test.ts tests/stores/unifiedStress.test.ts tests/native-cli/unifiedUxAdversarial.test.ts tests/native-cli tests/stores/session.test.ts tests/stores/nativeTabs.test.ts && npm run typecheck`. Stress covers 50 projects × 100 mixed sessions, 30 open descriptors, 120 state/selection/layout changes, 200-character titles and Windows paths longer than 260 characters. Timings are host/jsdom observations; 30 real CLI processes, output throughput, Rust, actual platform and rendered visual acceptance remain separate gates.


### Final unified verification preparation (Task 25)

- `DESIGN.md`, `PRODUCT.md` and current component/persistence/terminal/roadmap docs describe the one implemented shell and separate source evidence from unperformed acceptance. `docs/superpowers/execution/U01-U10.md` binds local results to the input HEAD and leaves final source/PR-merge/workflow/package identities pending until observed.
- Task25 runs locked npm install, full frontend tests, typecheck/build and the separate visual fixture unit/collection gates. Two historical session-tree failure fixtures now model canonical persisted readback after a successful pin/archive and assert exactly one reread/mutation; production recovery semantics remain unchanged.
- `.github/workflows/unified-visual.yml` is prepared for the final PR, with no development push trigger or publication permissions. It verifies committed baselines first, preserves failure logs/traces, optionally captures 13 unapproved candidates when all baselines are absent, and propagates the original verification result. Candidates never auto-approve/commit baselines. Branch-only `workflow_dispatch` is unavailable before the workflow exists on default; opening a PR while local rendering is blocked changes the freeze sequence. The owner approved the final draft/CI sequence on 2026-10-01 after independent review; the parent owns external actions, and pixel acceptance remains pending.
- Task23 remains BLOCKED_VISUAL: missing-baseline RED, all PNG reviews and no-diff/rendered acceptance are pending. Cargo/Rust and Linux host development packages are unavailable locally; final Windows CI/package/manual, native platform rendering/accessibility and D20 remain separate unperformed gates. No source or local test PASS can imply U10 completion or release promotion.

- Task25 final package trigger extension: `conpty-integration.yml` adds only PR path coverage for unified frontend, changed Rust metadata files, build configuration and focused tests. Original development push filters, manual trigger, jobs, permissions and publication boundaries stay unchanged. The final authorized PR can start ordinary CI, visual evidence and Windows package together; no local worker dispatch occurs. The earlier full 1,564-test result precedes this workflow/test-only delta, whose 10 focused policy cases, typecheck and four release-policy cases pass separately.

### Final review repair contracts (R1–R6)

- Both visual Playwright-to-tee steps explicitly use Bash/pipefail. The executable workflow regression replaces only the producer with exit 7 and verifies that both actual step scripts preserve that status. The original verification outcome remains the final gate; candidate PNGs remain unapproved.
- Session lists default menu teleport to true; AppMenu mouse selection stops bubbling even inside the archive drawer. Row F2/menu and global rename all obtain canonical exact-attempt edit ownership before displaying a draft. UI Save requires that admitted editor and cannot recapture a cancelled/replaced owner.
- Session diagnostics use the shared AppDialog and a typed allowlist of CLI, state, open/history/preparing location, runtime, bounded generation and safe error code. No title, source identity, raw path, configuration, argv, environment or raw transport error is projected. Runtime/generation and safe codes remain under explicit details. Selection/navigation/new intent or owner invalidation closes the surface.
- Native activity advances on launch/state/diagnostic changes and exact-attempt admitted input/output. Duplicate status receipts preserve activity; input/output publication is bounded to once per second per attempt, with no activity timer. Optional observer attention uses the existing ordered projection only; raw/unordered hooks never establish needs-user. Replaced/disposed attempts cannot publish activity.
- Arrow keys move focus through mounted visible project/session rows, including filtered results, without starting history. The existing configurable `projects` shortcut now opens Workspace, reveals the tree and focuses its search; Enter on a quick-switch project result selects that project, while historical session Enter retains the explicit resume flow. Editable/IME/modal/terminal keyboard ownership is preserved.
- Focused regressions: `tests/components/finalWorkspaceReview.test.ts`, `tests/config/visualWorkflow.test.ts`, `tests/native-cli/unifiedTerminalParser.test.ts`, `tests/native-cli/unifiedTerminalIdentity.test.ts` and affected tree/runtime/native store suites. BLOCKED_VISUAL, Rust/real-platform and D20 acceptance are unchanged; no local DOM or host result certifies rendered/native CLI behavior.

- Narrow R5/R6 repair-review follow-up: quick-switch mode is transient and clears on Escape or sidebar deactivation; reopening the persistent tree restores ordinary project Enter expansion. A starting Native tab retains at most one safe latest observer attention projection for its exact request/run/generation, publishes it only after the same attempt's running receipt, and discards it on unknown/failure/end/restart/close. Newer unknown projections replace earlier waiting; no observation establishes running or authorizes input.

### User feedback corrections (2026-10-01)

- TitleBar marks each noninteractive native hit target explicitly, including title text, app image and spacer; controls and their SVG descendants stay outside drag targets. The app image disables HTML image dragging. Tauri owns drag-region double-click maximize, avoiding a second Vue toggle. DOM regressions do not certify physical Windows dragging.
- Project expand/collapse controls retain localized accessible names, expanded state and keyboard behavior without tooltip bubbles. Session state silhouettes are circular, with separate inner symbols and accessible state names.
- Empty workspace guidance must fill and center within the available main terminal surface, including sidebar resizing/collapse. Visual fixtures must exercise the production empty surface rather than a narrower substitute.
- These changes follow user testing of source `9eccaf1`; changed screenshot baselines require fresh actual-pixel review and a subsequent no-update verification. Native Windows scaling/accessibility and D20 real CLI certification remain separate.

- Native launch request validation owns copying action/argv DTOs; do not `structuredClone` Pinia/Vue reactive objects before validation. The real terminal/store/entry composition is covered through the authenticated IPC test boundary for both CLIs.
- Profile and project mutations share one backend workspace CAS revision. Before the first safe-default profile creation write, refresh a previously loaded profile cache; never replay a failed or uncertain mutation automatically.
- Explicit Close selects an available remaining open session only while its original selection intent still owns the handoff; closing the final session returns to guidance. Stop/CLI exit retains ended scrollback. Failed runtime dispatch acknowledges only its claimed request sequence.
- This user-authorized repair prepares version 0.18.0 consistently across npm/Cargo/Tauri and test installer naming; it does not authorize Release/tag/updater publication.

- The final ordinary CI and Windows test-package workflows pin Rust 1.98.1, the compiler already verified for 0.18.0, after floating stable drifted to 1.99.0 mid-batch. Keep strict Clippy `-D warnings` and the declared 1.89 MSRV; toolchain upgrades require their own validation. No atomic runtime behavior is changed for this build reproducibility repair.

### Native launch prerequisites and explicit recovery

- A newly created `desk-safe-*` configuration does not imply a trusted executable. Nonlegacy `programPath: inherit` is `PROGRAM_TRUST_REQUIRED`, as enforced by the shared frontend/Rust availability fixture. New-session preparation checks the exact configuration revision before admitting a Native tab. The existing configuration editor may repair a never-admitted preparation; saving alone never launches. Explicit Retry can adopt the saved revision of that same configuration and CLI, but cannot relax an admitted attempt's frozen identity.
- Explicit Resume of an ended open Legacy session restarts its original project/session identity through the existing lifecycle, from both the row and history chooser. Plain activation only displays retained scrollback. A newer selection or changed owner cancels a delayed resume before admission.

- Workspace partial-source warnings retain up to twelve deduplicated, allowlisted source-category/error-code pairs behind collapsed details. Only the current refresh can publish them; late catalog reads cannot overwrite newer refresh results. No paths, profile/session identities, titles or raw exception fields enter diagnostics. Preserve source failures and existing filesystem/admission limits; see `docs/workspace-source-diagnostics.md`.
- Failed launch preparation retains the allowlisted availability issue separately from its preparation/retry state. Only `PROGRAM_TRUST_REQUIRED` offers automatic backend discovery; unavailable program/runner, missing environment source, legacy-read and invalid-request failures retain their own diagnostics. Do not copy raw issue fields.
- Automatic program discovery lists existing candidates without executing them. The user explicitly confirms a displayed candidate and Windows runner once; saving through the ordinary editor still never launches. The dedicated discovery confirmation binds retry to the acknowledged profile revision, rejects changed state, and preserves navigation/selection/edit cancellation through final adapter admission. It never retries an uncertain write. See `docs/cli-program-discovery.md`.
- Native projection failures expose only the original allowlisted code and a fixed diagnostic stage, never field/index values or raw parser messages. Preserve profile/storage/run codes through frontend wrapping; distinguish document/decode/validation, profile/environment/project registration, capability and source enumeration. Workspace warnings bound and deduplicate source/code/stage triples while preserving refresh ownership. See `docs/workspace-source-diagnostics.md`.
- Native host environment capture is shared by availability, discovery, history and launch. On Windows, preserve raw entries until case-equivalent inherited names are grouped; conflicting values use the OS-effective value, never map order. If that value is missing or absent from the captured group, fail safely. Keep explicit environment-layer conflicts strict. Never log inherited names or values; the opt-in host probe reports counts and fixed categories only.
- Windows inherited environment names may contain a single leading `=` (including Explorer's reserved pseudo-drive entry), with a nonempty tail and no additional `=`. Do not restrict inherited names to drive letters. Keep these names forbidden in profile/legacy/terminal/observer layers and on other platforms; NUL, empty names and embedded `=` remain rejected. Verify desktop-source regressions through a confirmed Explorer parent, not only a Codex-launched test process.
- History listings use bounded metadata observations for large transcripts; Messages/Search retain full-file validation. A truncated prefix is never complete history evidence: propagate `historyMetadataIncomplete` even when project filtering removes its row, and never infer absence from such a response. Preserve capability/identity checks and the existing file, aggregate, entry and time caps. Claude excludes only real project-level `memory` and per-session `subagents`/`tool-results` directories from main-session enumeration; symlinks and unknown layouts still fail. Read-only root selection admits explicit Cmd shims and the tested root-neutral flag allowlist, never arbitrary shell commands, raw/legacy argv, root overrides or conflicting flags; it does not certify a launcher or take over an external process.
- Shared reader/environment tests also compile in the independent D12/D13 crates. Keep their Windows dependencies explicit, and keep test-only command helpers independent of application-only modules while suppressing auxiliary console windows.
- Windows-to-cloud handoff: preserve bounded restart-fixture failure observations without changing deadlines, rejection, cleanup or production admission. A diagnostic observation is not a recovery fix or acceptance pass. The source, CI, installed-build and outstanding acceptance boundaries are recorded in `docs/local-to-cloud-handoff-2026-10-04.md`.
- Restart worker terminal diagnostics run only after the original restart-fixture rejection: at most three read-only observations within a 20ms sampling window and the remaining original 300000ms worker wait budget. Keep the original failure even if accounting later reaches zero, retain owned handles/cleanup, and publish only fixed counters/booleans; failed or incomplete queries stay unknown. See `docs/testing/restart-worker-diagnostics.md`.

- Historical-version unknown mutation receipts and failed held-state inspections use ownership-aware fallback copy; generic catalogue-read retry copy and known safe error categories remain separate. Saved generic preparation failures are normalized again when cleanup or a later successful inspection publishes unknown/handoff-issued ownership; the original failure remains available after confirmed cancellation. Late inspections cannot replace newer switch feedback or enable mutation replay. Manager visual fixtures separately capture the fully visible ownership warning across all four surfaces; evidence stays unapproved and is not native acceptance. See `docs/historical-version-preparation.md`.
- Historical payload policy is a static exact version/digest/size/inventory table. Its nine fixed entries must match the sanitized test-only `reviewed-measurements.json`; production must never import that fixture as authority. Keep the original 0.17.7 entry unchanged, exclude source-only companions from target output, and preserve all live ownership/restore checks. The user-authorized ordinary Windows x64 capability policy enables these exact reviewed targets through the existing FreshSettings coordinator and preserved-current-context return path. Runtime signature, unique per-user installation, unelevated same-user process, all-foreign-Job rejection, session and unknown-transaction checks remain mandatory. Paired payload measurements and source/UI tests do not constitute actual native roundtrip acceptance. Acceptance-feature builds retain their separate fixed deny-only source/target binding; future package trust changes require independent review.

### Session close entry deduplication

- Open Native and Legacy rows expose Close through the single trailing × primary button, including starting, running, unknown, stopped and failed states. The accessible native button remains focusable; rename editing temporarily uses that slot for Save.
- Overflow, pointer context and keyboard context menus omit Close and Stop. Running sessions cannot offer Stop and archive; non-running history keeps its separate archive action. Restart/rename and exact status recovery retain their existing state restrictions.
- The user-authorized 0.18.3 primary-action dispatch closes directly without a second confirmation. Existing exact-attempt stop/cleanup and ownership rechecks remain mandatory; unknown native attempts still require authoritative stop/cancel recovery and cannot be discarded without proof.
- The Close-only hover/menu baseline update accepts only the two actual images independently inspected from visual run `37318672270`; eleven existing baseline images remain byte-identical. Keep exact zero-pixel comparison and the thirteen-image inventory. The new confirmation/state/menu captures remain separate unapproved evidence. A full no-update run is required after this reviewed baseline update; see `docs/visual-testing.md`.

### Direct targeted history resume

- Only an explicit Resume control on a closed catalog row calls `resumeCatalogSession`; row clicks, double clicks and row Enter/Space cannot start or restore history. The explicit control directly calls `resumeCatalogSession` for the frozen selected source. No second confirmation is shown. Open rows still activate directly, while the global history entry keeps its searchable picker.
- Direct resume retains the original adapter admission and frozen source/profile-revision checks. Replacement requests, project/section navigation and disposal revoke pending admission; repeated current requests coalesce. A missing target remains unavailable rather than falling back to another row.
- Retryable direct-resume failures retain only that request's frozen source for an explicit Retry, even if a failed read removes the catalog row. The retry owns its own request sequence, rechecks the original source/profile/project, and cannot switch to a new row or default configuration. Navigation/replacement cancels both late admission and old feedback.
- Visual evidence clocks retain the same starting date but allow `Date.now()` to advance. A frozen clock can cause Vue capture/bubble timestamp guards to reject real keyboard events; keep native-event regressions and all Escape/focus assertions rather than hiding tooltips or weakening checks.

### Selective branch consolidation for 0.18.1

- Prepare npm/Cargo/Tauri metadata and both lockfile root identities consistently as 0.18.1; it remains an unreleased candidate. Preserve prior 0.18.0 test-build history and the fixed roundtrip acceptance inputs. The later user-authorized release recovery and production historical capability changes have separate exact-source validation; neither permits bypassing publication preflight or native runtime guards.
- The 0.18.1 baseline update accepts only the two Settings actual PNGs independently inspected from visual run `37351948916`; every changed pixel is confined to the footer version's last digit. Eleven other baselines remain byte-identical, with thirteen images and zero-pixel comparison unchanged. The original 224/226 run remains failed; all 226 current cases need a fresh full no-update run. See `docs/visual-testing.md` and the approval manifest for hashes and provenance.
- The explicitly enabled visual fixture fixes the displayed application version to synthetic `0.18.1`, matching those reviewed Settings baselines. Only the existing isolated visual plugin may override that value. Actual Vite resolver tests prove normal development and every build retain package identity; the ordinary identity test checks the compiled value. Baseline PNG changes require explicit authorization, actual pixel inspection and exact source/hash approval provenance; original geometry/interaction assertions and zero-pixel thresholds remain required. Failed 0.18.2 version-text runs remain failed evidence; a fresh full no-update visual run is required.
- The authorized 0.18.3 baseline update copies only nine independently inspected actual PNGs from old visual run `37965393533` (source `198c3f8`, tested merge `79fec77d`, artifact `11633985429`). Exact repository transport is evidence commit `728ae1fedd2b07040c58eb2c1e1484c1d2ad228b`; the approval manifest binds every image hash/ZIP entry. Four other PNGs remain unchanged. These are old-source expected bytes, never current-head CI evidence. Native receipt/title flex changes require a new complete 230-case no-update run. Previous failed runs remain failed, four new Native receipt captures remain separate unapproved evidence, and Native current working/completed is still unverified.
- The frontend `log_message` IPC keeps its signature, severity mapping and `[Frontend]` scope but emits only a UTF-8 byte-count summary. Do not retain raw frontend paths, errors, prompts, clipboard/terminal content or guessable digests; other backend allowlisted diagnostics remain independent.
- Output transport latches throttling in the same lock that reaches the high watermark, including when ACK arrives before the next capacity check. Only low-water recovery clears it. Preserve exact owner/offset ACKs, FIFO global-budget admission and revoke/drop wakeups; test-only wait observations do not enter release builds.
- The legacy `release.js` and npm release/OSS entry points are fail-closed shims. They cannot publish, change Git state, read credentials or edit proxies; no flags re-enable them. Future public promotion remains a separately verified path, not an option on the retired publisher.

- The five original Windows GUI supervising tests share one test-only parent lease before spawning workers; all other tests retain ordinary parallel execution. Worker processes never acquire that lease. Original modes, per-worker deadlines, assertions, inventory and ignore classifications remain unchanged. The exact five-test Actions comparison preserves full/selected listings and the real outer result; it is resource isolation, not proof of a root cause, installation/return or authenticated CLI acceptance. Production runtime and release qualification are unchanged.
- Native user-input receipts must match the frozen request identity and exact byte count; mode epochs observe xterm 5.5.0 public modes only after parser batches. Do not claim detection of a mode round trip within one batch. Epoch exhaustion/disposal stays fail-closed. `NATIVE_INPUT_PAUSED` uses only fixed allowlisted diagnostic copy with no replay/recovery action or raw payload.
- Ordinary Windows registry scope fixtures follow `CARGO_PKG_VERSION`; a valid-PE current-version control and mismatched-version refusal preserve exact production admission. Do not change the separate fixed 0.18.0 roundtrip acceptance bindings when updating ordinary fixtures. Linux source-contract checks are not Windows runtime acceptance.

### Session behavior repair for 0.18.3

- Archive is an external trailing quick button only for ended/failed non-archived rows without preparation ownership. Overflow/context menus omit it. Existing typed running archive requests still require the original exact-owner stop-and-archive confirmation.
- Preserve complete historical status detail/cause vocabulary and row/project priority; missing/unordered activity remains explicitly unknown. Legacy uses exact-PTY explicit attention receipts; Native uses only existing exact-run ordered activity projections. No completion from Stop, exit, silence or output; observer remains default off and raw/Codex/Shell receive no Claude overlay. See `docs/session-status-semantics.md`.
- Native Claude recent/unread notices are independent receipt observations from the existing authenticated optional hook bus. Keep fixed kinds/IDs/receipt time only, private exact request/run/generation proof and bounded non-evicting deduplication; publish starting receipts only after that owner's running launch receipt. Unknown/end/failure/restart/close invalidate notices. Only actual visible/selected/focused ownership acknowledges an unread reply-end ID; inactive observation cannot initiate a new flash. Never promote a Stop receipt to current completed or alter profile defaults/CLI flags to provide this feature.
- Unified terminal host owns one window focus subscription and attention coordinator. Actual visible/selected/focused ownership acknowledges permission/completion, never sticky errors; initial unproven focus cannot acknowledge, and late subscription registration disposes after unmount. Selection alone never proves acknowledgment.
- A Native generation change detaches old provenance/ACK before a bounded byte CAN+RIS parser fence, verifies attempt/term/token/publication ownership, resets and installs new bindings only afterward. Partial VT/UTF-8, queued old protocol replies, canceled/unmounted/superseded parser preparation cannot contaminate or launch the new attempt. Recover/activate/hide/theme do not reset.


### Session opening order and safe proxy testing

- The catalog/archive retain persisted `lastOpenedAt` metadata. The visible normal session tree snapshots `lastActivityAt` descending with deterministic ID ties on entry or its existing Refresh sessions action, then freezes existing relative order until another entry/explicit refresh. New sessions, reopening closed history and archive restore update opening time; switching an already open terminal changes selection only. Missing historical times use a fixed zero fallback; imported live tabs use their immutable creation time. Background output/status/completion never changes opening time or steals selection. Project pin/alphabetical ordering and archive grouping remain intact; there is no new session-pin subsystem.
- Opening-time metadata uses the existing locked incremental projects store, preserving concurrent same-identity display-name writes. Record only admitted openings and exact history-identity transfers; do not write on every activity refresh or automatically replay failed writes.
- Repeated close clicks join only the exact still-owned in-flight attempt. A failed stop remains failed and retains the terminal; the user may explicitly retry. New attempt identities cannot join an old close, and late receipts cannot recreate closed terminals. The original five-second unconfirmed-stop guard and direct close without another confirmation remain required.
- Update proxy address/port are visible while userinfo fields stay masked, including pasted authenticated URLs. Preserve unedited encoded authentication bytes. `test_updater_proxy` uses the real updater proxy parser/shared HTTP client to read only the bounded official latest manifest; it neither creates installer admission nor downloads packages, changes saved/system proxy or imports CLI configuration. Fixed safe errors/timing, timeout bounds and stale-result fences remain mandatory. See `docs/update-proxy.md`.
- Native App/terminal regression tests use synthetic IPC receipts and do not certify real CLI, installed Windows, taskbar or updater installation. Existing screenshot baselines, original security assertions and complete CI inventory must stay intact.

### Next session-list UX batch (unreleased)

- Row activation is selection-only for an already open owning terminal. Runtime dispatch also rejects closed/preparing-row activation rather than falling back to resume. Explicit history Resume, failed-creation Retry and archive Restore remain native keyboard-accessible, always-discoverable controls; equivalent menu entries are omitted. Open-row Close and ended-row Archive retain their existing contracts.
- `SessionsPanel` owns only a view-local activity-order snapshot. Working/output/completion/status and same-project selection update display metadata without moving existing rows. New/admitted openings and manual archive restores lead; newly discovered old closed history is appended until explicit Refresh. Project entry/Workspace re-entry and Refresh re-sort by latest activity, preserving selection, name/pin project order, and archive grouping. `lastOpenedAt` continues through the existing metadata writer solely to identify explicit opening/restoration; there is no session-pin or backend ordering subsystem.
- Pending exact-source catalog resumes coalesce one operation while preserving each caller's cancellation guard. Pending presentation is projected through both catalog and actual `projectManagement` groups, disabling repeat Resume clicks; navigation and source/profile guards remain authoritative. Preparation Cancel/Close behavior is unchanged.
- Status backplates are solid 16px local SVGs with contrasting clock, work, shield, check, stop, close, alert and question marks; known idle is a plain solid dot and uncertain ownership uses a confirmation clock. Existing activity details (thinking/tool/subagent/compacting/input) and archive metadata have specific glyphs and localized passive hover/screen-reader labels. Known starting/working breathe slightly, thinking dots brighten sequentially, and permission/input/completion animate once on true state transitions; reduced motion is fully static. Native hook ordering/completion evidence and unread/taskbar ownership rules are unchanged.
- The four new status-sheet browser captures are unapproved evidence, separate from all thirteen immutable historical baselines. Do not update baselines, claim real CLI/Windows rendering acceptance, change version metadata, merge or release from this frontend draft.


### Ordinary signed historical installation

- Ordinary historical install is a separate selection/preparation-bound capability with a shared one-shot latch against reviewed roundtrip. Default FreshSettings; complete source installation/Desk/WebView backup and original global control custody precede normal official installer handoff. Use fixed private sibling backup custody from transaction start; preserve signatures, held image identity, ACL/TOCTOU/durability and Job-free guards. No REVIEWED inventory relaxation for strict switch, no change to upgrade-only updater.
- Ordinary receipt means installer started, never installation completed. Unknown receipts cannot replay; ordinary manager offers no Return/Launch. Preserve exact backup location and manual restoration evidence; no automatic deletion. Existing global recovery records require explicit runtime diagnosis and preserved evidence. See `docs/historical-version-preparation.md`; hosted tests do not establish Job-free native installation acceptance.

- 普通安装器宿主覆盖：实际提权 token 下，020/022/024/025 四项非提权成功 integration 单独披露未验证，不计 pass；原 18 项 Job-free 分类独立保留。027–029 拒绝/状态次序/精确 fixture panic 清理合同必须执行；自然非提权环境仍执行原四项全部断言。编译后先复用同次 executable 定向检验，再执行原完整库存；禁止移除 `require_unelevated`、修改宿主权限或将 fixture 清理能力用于生产未知进程。
