# CC Desk — Agent 工作指南

本文件是所有 coding agent 的通用规范和执行入口。`CLAUDE.md`、`GEMINI.md` 与 Copilot 指令只引用这里，不维护规则副本。目录内的 `AGENTS.md` 补充该目录的约束；修改文件前，主动读取仓库根到目标目录之间的这些指南，不依赖工具从当前工作目录自动发现全部规则。加载与维护说明见 [docs/agent-guides.md](docs/agent-guides.md)。

建议从仓库根开始会话。Claude Code 深层启动时，祖先标准的导入可能受外部导入信任限制；不要自动修改客户端批准状态或复制规则来规避。根会话读取目标文件后的目录加载与各工具实测边界见上述加载说明。

完整项目约束保留在 [docs/project-contracts.md](docs/project-contracts.md)。**修改相应模块前必须阅读其相关章节及下方对应文档**；不要只依据本文件的概述判断接口或授权语义。Native CLI 架构以 [docs/native-cli-v3.md](docs/native-cli-v3.md) 为权威。

## 工作方式

- 回复中文优先。代码与提交信息遵循项目既有风格；新增代码注释用中文，只解释不明显的意图或原因。
- 先保证正确性，再考虑简洁性和性能。默认最小改动，不做无关重构，不猜接口语义或业务规则。
- 搜索先用 `rg`。大文件先看定义、导出和调用关系，按需读细节；修改前读目标文件，改签名前搜索所有调用方和类型。
- 复杂任务先确定主链路与模块边界，再拆分。多文件修改先改定义，再逐个改调用方并验证；替换失败重读现有内容。同一错误没有新信息时不要重复试同一方案。
- 保留用户和其他 agent 的改动。未获明确要求不 commit、切分支、amend、force push、merge、发布或自动 bump 版本；不执行破坏性操作，不打印或硬编码凭证。
- 行为、接口、配置和约束变化须同步文档、注释和示例。通用规范改本文件，模块细节改对应 `docs/*.md`，目录特例改当地 `AGENTS.md`；不要改工具入口来增加独立规则。
- 最终报告修改点、实际验证结果和剩余风险。区分已有失败、新增回归、未运行和目标环境证据缺口；不能用静态或宿主测试替代真实 CLI/平台验收。

## 协作边界

- 主代理负责方案、跨模块协调、并发和安全边界及最终判断。子代理只做边界明确的分析、局部实现或验证，不直接向用户提问；结论必须由主代理核对。
- 默认最多 1 个实现子代理；阶段收口可增加 1 个只读审查代理。禁止多个实现代理同时改同一工作区，不为单文件检索、格式化或简单测试迁移创建子代理。
- 长期质量标准为 GPT-5.6 Sol + High；常规编码优先 Terra + Medium，机械检索和测试维护优先 Luna + Medium；架构、并发、状态机、安全边界和最终裁决用 Sol + High。实际接口不支持时继承当前模型，不伪称切换；`xhigh`/`max` 只在复杂裁决或最终安全审查中临时使用。

## 架构与安全底线

- CC Desk 是 Tauri 2 + Vue 3 + Rust + xterm.js 的多项目、多会话 CLI 宿主。真实 Claude Code / Codex CLI 拥有交互、slash 命令、模型和 Provider/API Key 配置语义；GUI 不重做这些能力。
- Native CLI v3 使用 authenticated document bridge、冻结的 launch snapshot 和 owned run。新双 CLI 功能不得 fallback 到 Legacy `ptySpawn`/`ptyInput`/`ptyKill`、默认配置根或历史删除接口；Legacy 仅经统一 adapter 保持兼容。
- 运行 owner 的状态/关注变化经现有 `catalog.refresh(projectPath)` 合并刷新受影响项目，跨项目移动同时刷新旧、新项目；替换、移除和卸载释放对应监听。历史与归档按投影签名刷新受影响项目，读取进度不触发刷新，行序列不变时不替换目录数组；项目请求版本继续拒绝迟到覆盖。这是 UI 投影优化，不能据此改变进程或授权所有权。回归与测量见 [CI 性能记录](docs/ci-performance.md)。
- 请求、profile revision、run、generation、document 与 stream 必须精确绑定。输入 partial/unknown 不重放；输出有界并使用 exact ACK；原生 workspace 写入遵守 revision/CAS。
- path、owner 或 opaque scope ID 本身不是授权。资源读取使用 backend-held scope 和 kind-specific safe DTO，不发布原始 env、argv、headers、凭证或异常载荷；路径只按对应 DTO 的授权与展示契约返回。Observer 是可选隔离侧通道，不拥有进程控制权。
- Windows ConPTY、完整 bracketed-paste 字节与一次逻辑 `write_all` 契约不得为测试变绿而降级。实际 `UserPromptSubmit` 正文相等、真实 CLI 和 WebView/平台验收是独立证据。
- 新 Rust DTO 返回前端使用 `#[serde(rename_all = "camelCase")]`。新 Tauri command 按 `commands.rs` → `lib.rs` 注册 → `src/api/tauri.ts` 封装；新 JS API 写操作须核对 [capabilities](docs/capabilities.md)，不能假设 `<plugin>:default` 含全部权限。
- 视觉系统唯一规范为 [DESIGN.md](DESIGN.md)。颜色、字体、圆角和阴影使用 `src/styles/global.css` token，light/dark 成对维护；终端 `--terminal-*` 独立。复用共享 UI 控件，不另建菜单、弹窗、提示或反馈实现。

## 测试与验证入口

所有命令默认从仓库根运行；Rust 原生命令进入 `src-tauri`。Windows 直接跑 Cargo 前，先从仓库根运行 `node scripts/prepare-conpty.mjs`。依赖安装用 `npm ci`，开发启动用 `npm run tauri:dev`。

| 场景 | 命令 | 证据边界 |
| --- | --- | --- |
| 修改迭代 | `npm test -- <测试文件或目录>` | 优先跑受影响行为，不默认重复全量 |
| 普通前端回归 | `npm run test:fast` | `fast-node` + `fast-dom`，不含进程与压力组 |
| 真实脚本/子进程/HTTP | `npm run test:process` | 保留真实执行，不替换为 mock |
| 压力行为 | `npm run test:stress` | 完整 5,000 会话、30 owner、120 次变化 |
| 完整 Vitest | `npm test` / `npm run test:all` / `npm run test:ci` | 四组全量；不包含独立 Node policy、Rust、视觉和平台验收 |
| 类型/构建 | `npm run typecheck` / `npm run build` | 按变更影响执行，不能替代运行验收 |
| 视觉逻辑/浏览器 | `npm run test:visual:unit` / `npm run test:visual` | 两套独立入口；浏览器成功不等于 Windows WebView 验收 |
| 生产启动/CSP | `npm run test:production` | 重新构建正常生产入口，在真实生产 CSP 下检查中英文首屏与异步 Settings；合成 IPC 不替代安装包 WebView2 验收 |
| Rust 迭代 | `cargo test --locked --lib <行为过滤名>` | 在 `src-tauri` 按域过滤，记录实际执行库存 |
| Rust library 全量 | `npm run test:rust:lib` | 等价 `cargo test --locked --lib`，包含真实进程集成，**不是快速组** |

- 前端测试在 `tests/`，Rust 在 `src-tauri/src/tests/`。新增功能和行为修改同步测试；bug 先复现 RED 再修到 GREEN。测试行为和错误边界，不测 getter、类型声明、简单 props 或第三方能力。纯文档改动检查内容与引用，不新增镜像式测试。
- 命名使用英文 `Feature_SubFeature_SeqNum`，中文说明目标。Pinia store 测试使用 `setActivePinia(createPinia())` 与 `mockIPC`。Schema、API、配置、生成文件修改后先生成，再跑相应验证。
- `vitest.config.ts` 的 Node/进程文件清单是显式分类；其余普通测试进入 DOM，视觉 fixture 用独立配置。保持测试发现全量、文件隔离、断言和压力工作量；需要浏览器全局的文件不能移入 Node。`tests/config/testProjects.test.ts` 核对真实发现库存与分组。
- 修改后先跑最小适用检查。涉及多模块时补充类型检查、受影响组与必要完整 gate；检查通过后仅在新变更、失败或证据缺口需要时扩大或重复。全量 Rust 本地可能很慢，不作为每次修改默认环。
- 普通 CI 仍必须执行原库存与 required checks：Windows Rust 编译四个原始 harness 后确定性分 16 片，`Rust checks` 聚合 producer/static/全片及名称、忽略集合、原始计数。18 条 Job-free 专项按原政策披露未验证，不增排除；timeout/部分输出不算通过，source SHA/run/attempt 证据不得混用。详见 [测试编写原则](docs/测试编写原则.md) 与 [CI 性能](docs/ci-performance.md)。
- Release 的 signed build 仍只依赖精确 main admission；preflight 等全部 build 成功后才等待普通 CI，publication 仍依赖 preflight 与 build。原 120 分钟 CI wait 从该 step 开始，异常 CI 诊断可能额外延迟构建耗时；来源、覆盖和签名门禁不变。
- Roundtrip debug 两场景仅在同一 job 首次完整构建成功且前端存在后复用前端；第二次只覆盖 `beforeBuildCommand`。两次 Rust 构建、严格 Clippy、disabled binding 和原始字节恢复不变，不跨 job/源码复用 `dist`。
- 独立 Node 粘贴验收编译真实生产源码；共享 AST 编译器仅将 Vite 的 `import.meta.env.DEV` 设为 `false`，不改正文或其他开关。宿主剪贴板隔离不能替代真实策略，详见 [粘贴契约](docs/paste-framing.md)。
- Rust 权重校准须使用本次编译识别的 executable、完整原始名称及源码/工具链记录；fixture 初始化失败、timeout 和并行输出观察时间均不能当作单例耗时。受限环境核对临时目录准入，保留失败记录，不改 ACL/token、Job 门禁或测试断言。持有 Windows 目录句柄的 fixture 先释放句柄、最后释放 `TempDir`，不得放宽生产共享或身份校验来清理测试树。
- 所有版本先将经审查和 CI 验证的变更合并 main，再从对应精确 main 提交发布；合并后清理已合并、冗余及临时本地/远程分支。有继续价值或长期用途的独有工作直接保留原分支；未整合独有改动的永久删除须逐项确认，不为清理自动创建备份。
- 发布只接受经授权的 protected-main promotion：同一精确源码与 workflow run/attempt 的完整 CI/coverage、产物字节和三平台实际签名。开始任何发布/恢复操作前必须阅读 [发布流程](docs/release-process.md) 及项目约束的版本/发布章节；不能靠本地摘要、普通套件成功或构建候选绕过门禁。D20 真实 CLI Layer-C 单独记录。

## 按任务阅读

先在 [完整项目约束](docs/project-contracts.md) 定位对应章节，再读下列细节；不要将完整约束自动导入每个工具入口，避免超过指令加载预算。

| 任务 | 必读文档/目录指南 |
| --- | --- |
| Native launch、profile、workspace、权限与 observer | [native-cli-v3](docs/native-cli-v3.md)、[执行账本](docs/superpowers/execution/)、[cli/AGENTS.md](src-tauri/src/cli/AGENTS.md) |
| Rust、ConPTY、PTY 与粘贴 | [src-tauri/AGENTS.md](src-tauri/AGENTS.md)、[terminal-integration](docs/terminal-integration.md)、[conpty-runtime](docs/conpty-runtime.md)、[paste-framing](docs/paste-framing.md) |
| 会话树、项目、历史、资源与交互 | [components](docs/components.md)、[interaction](docs/interaction.md)、[project-resources](docs/project-resources.md)、[统一工作区账本](docs/superpowers/execution/unified-workspace-progress.md) |
| UI、设置、布局与终端主题 | [DESIGN.md](DESIGN.md)、[layout-design](docs/layout-design.md)、[terminal-preferences](docs/terminal-preferences.md) |
| 持久化、迁移与配置 | [data-persistence](docs/data-persistence.md)、[env-injection](docs/env-injection.md)、[startup-checks](docs/startup-checks.md) |
| 测试或 CI 提速 | [测试编写原则](docs/测试编写原则.md)、[ci-performance](docs/ci-performance.md)、实际 `.github/workflows/` 与测试入口 |
| 版本、updater、打包、签名与发布恢复 | [release-process](docs/release-process.md)、完整项目约束的版本/发布及 signed installer 章节 |
| Agent 指南维护或工具接入 | [agent-guides](docs/agent-guides.md)，只在入口放原生导入或显式引用 |
