# 会话能力对照与本轮问题证据

本轮对照范围是会话管理、状态和提醒。已确认原生更名被显示别名替代，以及 Native 新建会话重启可能另开对话；两者不是旧功能的等价实现。恢复、单次关闭、归档恢复和任务栏提醒仍存在。不能把未证实的系统通知、全局重置或旧版没有的 Codex 能力算作回归。

## 基线与证据边界

- 最近正式 pre-0.17 tag：`v0.16.0`，`4e7b3e9cddfe219b198e9b3a3d9541bc2fb30515`。
- 最后 pre-0.17 源码：`v0.17.0^`，`8cdc32ee33c175cde220e2945374c6e77cf4f4f2`，2026-08-29，版本仍为 0.16.0。所核对六项会话核心文件与该 tag 无差异。
- 当前安装版本对应基线：0.20.2，`f3f0b9e7c495a0e9bc7aa0b008887b3b75094b95`。本轮分支 `fix/session-observation-refresh-index` 未提交、未推送、未发布。
- 下表的“当前”指上述基线；本轮修复另列。旧文件行号须在对应 SHA 下读取，不能用当前行号替代。
- WSK-MSI：Claude 2.1.296、Codex 0.162.1。事故机 Claude 2.1.301 与 WSK 不同，尚未访问事故机，也未证明版本差异是故障原因。

## 会话工作流

| 工作流 | 旧版证据 | 当前路径及结论 | 最小处理和验收边界 |
| --- | --- | --- | --- |
| 新建 | `XTermTerminal.startNewSession:849`、`TerminalView.handleNewSessionIn:499` 创建并启动 Claude PTY | `unifiedSessions.createSession → createNativeTab → NativeCliTerminal`；配置、项目与 attempt 冻结。入口改变，安全性改进 | 保留 Native 默认入口；组件请求不替代目标 CLI 登录、launcher 和 WebView 实测。统一入口变更见 `7264481`、`b1309bd` |
| 历史发现与完整性 | `session.fetchHistory:555`、分页 getSessions；默认 Claude 根，展开懒加载 | Native 按 profile 根、注册项目和精确 cwd 发现；Legacy 保留默认根。新增 Codex、压缩历史及部分失败提示 | 不按 UUID/cwd 猜授权。本轮修正人工 code review 主线程过滤；父目录 cwd 与子项目仍不自动归并。压缩恢复见 `f3f0b9e` |
| 首次名称更新 | `useStatusMonitor.userPromptSubmit:61–73` 直接从首问更新默认标题 | Legacy 保留；Native 依赖唯一历史身份，历史重读只由首次身份或回复结束触发，可能延迟到回复结束。无身份则保留占位名 | P2：认证首问 occurrence 可触发受限重读，不读正文、不猜 Codex 身份。当前 `NativeObservedIdentity_WithoutIdentityKeepsLocalTitle_031b` 证明保守占位，不能证明旧时效等价 |
| 历史恢复 | `TerminalView.startResumeSession:131`、`handleSwitchToProjectSession:471`，以项目和 ID 恢复 | Native `resumeSession → verify → createTab(resume-id)`；Legacy 按旧来源恢复。行点击、恢复对话框、直接 ID 入口仍在 | 保留。测试覆盖来源/revision/重复请求；仍需真实 CLI 同 ID 上下文续接。入口变更见 `181c6f5`、`4e82596` |
| 单次关闭 | `session.closeTab:293` 先移除 tab、异步 kill 后读历史 | Native `closeSession` 先确认停止，再移除并读历史；失败不要求再关一次。Legacy 也 await 停止 | 保留并改进。修复链 `c69427b`、`ed61ad1`；本轮分离关闭和归档坐标。真实进程停止/WebView 点击仍独立验收 |
| 项目内全部关闭 | `ProjectNode.closeAll → TerminalView.handleCloseAllSessionsIn:507 → session.closeAllTabs:327` | 当前项目菜单无等价入口，只能逐行关闭。P2 入口丢失 | 恢复时按项目快照逐项派发 canonical close，统一确认与部分失败反馈，不调用全局 ptyKillAll。统一菜单见 `f2764aa` |
| 工作区重置 | `XTermTerminal.cleanup:862 → session.cleanupAll:669` 是内部端口，未找到旧可见入口调用证据 | Legacy 内部清理仍在；统一 host 不暴露跨 runtime reset | 未证实旧用户能力，不据此新增全局杀进程操作 |
| 重启并继续对话 | `XTermTerminal.restartTab:729 → buildClaudeArgs:809`，已有 sessionId 就 --resume | Legacy 保留；Native 新建 tab 重启仍为 new 且删除观察 ID，可能另开对话。P1 连续性缺口 | 本轮以冻结 run 根、强制历史正向确认、精确 owner 生成 resume-id；未知身份不得静默 new。原 action 保留见 `545c8cf`，清观察 ID 见 `603c503` |
| 原生更名 | `TerminalView.handleRenameSession:399–405` 向运行 Claude 发送 /rename，再更新 tab 名 | 两 adapter 后来只 saveSessionDisplayName，终端端口拒绝注入。P1 原生操作被别名替代 | 本轮接官方元数据 API 和读回；未知结果不重放、不假报别名成功。打开 run 没有后台确认 ID 时有明确支持边界 |
| 归档与恢复 | `session.archiveSession:951`、`restoreSession:960`、项目归档弹层 | Desk 归档/恢复、独立按钮、菜单及项目/全局抽屉仍在；Codex 原生归档走 CLI unarchive | 保留、入口变化。本轮固定动作槽；原永久删除入口移除是额外能力变化，不能叫归档失效，也不自动恢复破坏性操作。Native 控制见 `7b2c8ef` |
| 排序 | 会话按 lastActiveAt，项目置顶/字母序/孤儿置底 | 当前会话按持久 lastOpenedAt，后台活动与选择不调序，项目稳定 tie-break | 有意改进，不恢复活动导致的跳动。见 `c69427b`、`7faec9b`、`92b9457` |

## 状态与提醒

| 能力 | 旧版证据 | 当前 Native 与 Legacy | 处理原则 |
| --- | --- | --- | --- |
| 进程运行 | session.setTabPty、sessionDot.computeDotClass | Native 精确启动回执；Legacy PTY 状态；进程与活动分开 | 正确改进，fixture 不证明实机生命周期 |
| 正在工作 | prompt/tool/subagent 事件设置 working | Native 粗粒度 working；Legacy 保留并加鉴权/因果限制 | 依赖观察通道，不用终端输出猜状态；见 `2beb44d`、`198c3f8` |
| 空闲 | Stop/idle_prompt 清 working | Native Stop 或有界无活动回落；Legacy 对应回合处理 | 超时只是最佳努力，off/unavailable 不是已知空闲；见 `1ae8558` |
| 等待输入 | 旧为一般 pending/通知，未证明独立 waiting_input | Native 一般 waiting；类型和 fixture 能表达不等于生产来源已提供 | 旧独立能力未证实；需要明确原因来源 |
| 等待权限 | attentionFromEvent 把 permission_prompt/worker_permission_prompt 分类 permission | Legacy 保留；Native 与 idle_prompt 同为 waiting | P2 具体原因降级。需要认证 occurrence 与当前状态分开；见 claudeObserver、`198c3f8` |
| 子代理 | 旧只是一般 working，无独立状态 | 当前有带 agent ID 的有界 subagent_running 提示 | 新增改进；不把 agent ID 当完整 invocation epoch，见 `603c503` |
| 错误 | attentionFromEvent 把 StopFailure 分类 error；不是所有 tool failure 都算会话错误 | Legacy 保留；Native StopFailure 当前退为 unknown，可保留失败 occurrence notice | P2 原因辨识降级。补明确生产来源契约，不把失败 occurrence 伪装当前状态，见 `2beb44d` |
| 未知与观察不可用 | 旧没有明确活动未知图形，缺事件容易仍为运行/空闲 | 当前区分进程 unknown、观察 connecting/off/unavailable 和活动 unknown | 正确改进。本轮增加冻结安装原因；事故机通道是否健康仍未知 |
| 进程结束 | handlePtyExit 保留 stopped tab，清 working/pending/attention | Native exited/stopped 与回合活动分离；Legacy 保留终端 | 保留。退出、静默不等于回答完成 |
| 前后台一致性 | 旧 active tab 隐藏 permission/completed，选择即 ACK | 当前选择不改真实活动；focus+visible+精确 active owner 才清未读 | 正确改进；见 `198c3f8`、`f36fd14`，OS 焦点仍需平台验收 |
| 未读完成/结束 | 旧 idle_prompt 产生 completed，Stop 本身不产生 completed | Legacy 保留；Native 用独立 reply-ended 未读回执、角标、Tooltip | 替代语义，不能称明确当前 completed。见 `f36fd14`、`99f7616` |
| 折叠项目汇总 | ProjectNode:44 显示 completed 计数 | Native 不产 attentionKind:completed，现汇总也不计 unreadReplyEnd | P2 发现能力缺口。应增加“未读回复结束”汇总，不伪造当前完成 |
| 任务栏提醒 | setPendingWithAttention 在失焦时 requestUserAttention(Critical) | useUnifiedWindowAttention 统一处理 Legacy 原因、Native waiting 与新结束回执 | 保留和去重改进；mock 不等于实际任务栏验证 |
| 系统 toast | 所核对旧链路只有 requestUserAttention，未证明 OS toast | 当前任务栏及应用内反馈 | 未证实旧能力，不当作回归，也不新增通知权限 |

## 更名变更的准确时间线

1. `545c8cf`（2026-09-30）：最初 Native adapter 只改 tab 内存名。
2. `26b8f76`（2026-10-01）：加入持久显示别名，并替换统一 Legacy 的运行更名；首次进入 v0.18.1。
3. `92b9457`（#51，2026-10-14）：短暂恢复运行 Claude 的 /rename；Codex 仍用别名。
4. `fa7b0b1`（#53，同日）：PTY 回执不能证明 CLI 编辑器为空，撤回注入以免拼接并提交草稿；首次进入 v0.20.0。

保护草稿的理由成立，但不能证明原生更名需求已完成。原 sessionRenameDelivery 与独立 Node 测试断言 display-name-saved 和零终端写入，只证明新别名契约自洽；本轮改为原生成功读回与不支持/失败不落别名的断言。

## 本轮问题与验收矩阵

| 问题 | 根因证据 | 本轮主要文件 | 验证和剩余条件 |
| --- | --- | --- | --- |
| 列表闪烁 | 原每次输出时间变更会失效 catalog，后台使用通用 loading | useUnifiedWorkspaceRuntime、unifiedSessions、SessionsPanel | 定向和完整 Vitest、隔离浏览器 DOM 身份通过；WebView2 未验 |
| 非主动归档 | 关闭变历史后原位变归档，已在隔离浏览器复现 | SessionItem、交互文档与组件/浏览器测试 | 固定槽验证通过；没有操作审计，不能据此证明事故根因或用户误操作 |
| 历史漏显 | code_review 无条件过滤；精确 cwd 为另一独立归属规则 | native_projection/history、cache、Rust fixture | 人工主线程过滤修复；原“不完整”横幅具体错误码未知，不宣称已解决 |
| 观察无角标 | 静态设计把 off/unavailable 显示为中性运行；机器差异未定位 | observer_status、observer_host、hook、nativeTabs、SessionAppIcon | 安装原因与初始连接态可观察；新建和恢复皆受现场影响，不能只归因恢复配置 |
| 索引重建 | 原无统一重建；新增事务边界由独立审查复现并修正 | session_index_maintenance、session_name_index、native cache、维护 store/UI | 同 ID 恢复、迟到 begin 取消保护、显式读取失败、旧代隔离通过；超 1KiB 异常 manifest 保守拒绝，旧代保留 |
| 原生更名 | 上述原生到别名替换已证实 | native_rename、nativeProjection、两 adapter、官方 SDK helper | Codex 0.162.1 私有 API 读回成功；Claude 官方 SDK 已接入，2.1.296 空闲 TUI 私有问答恢复成功。忙碌 writer、事故版本和安装包未认证 |
| 重启续接 | new action 跨 generation 保留已独立复现 | nativeCliAdapter、nativeTabs、useUnifiedWorkspaceRuntime | 新建→精确历史→resume 同 ID、错根/替换/未知身份回归；真实 CLI 上下文仍需目标平台 |

现有五项修复检查点：完整 Vitest 181 文件、2635 通过、2 原有跳过；Rust 130 项定向通过、2 原 benchmark 忽略，严格 Clippy/格式通过。该数字早于后续更名/重启集成，不能冒充最终源码全套结果。视觉逻辑 66 通过、生产 CSP 中英文 2 通过；浏览器 13 个不可变截图基线不匹配，本机 HEAD 空屏对照与当前输出字节一致，其他像素仍未认证，未重写基线。

本轮继续实现上述缺口，最终修复状态和未完成边界见下一节。保留兼容路径，不改写用户真实历史来修补界面。

## 本轮增补的交付状态

下表是修复进度；上文“当前路径”保留 0.20.2 基线证据，不代表缺口仍全部存在。

| 项目 | 本轮代码与验证 | 尚需目标环境证明 |
| --- | --- | --- |
| 首问标题及时同步 | 已认证 prompt-submitted 独立回执驱动现有去抖历史重读；同 tick tool 事件不覆盖证据，重复回执不重复 IO；只读历史，不采集正文 | 真实 CLI 写入历史与 hook 时序、WebView 显示时延 |
| 折叠项目完成未读 | 独立 unreadReplyCount 聚合 Native 未读回复回执，文案明确“未读回复”；不伪造当前 completed；原焦点/可见/active owner ACK 保持 | 真窗口焦点与任务栏提醒 |
| 项目内全部关闭 | 项目树及项目页恢复入口、共享确认；冻结本项目当前 owner，逐个 canonical close；新会话及替换代次不在授权集合；部分失败继续处理其他原会话并明示结果，不归档 | 真实进程停止及部分失败 UX |
| 重启续接 | new 和 resume-id 均核对冻结 run 来源及新鲜正向历史，raw 保持显式重跑；独立只读来源证明在 PTY 退役后仍可核对，旧控制句柄继续失效 | 真实 CLI 同 ID 恢复及配置根变化 |
| 精确权限/异常原因 | 已实现并复审：后端固定枚举、前端 kind-specific 白名单及 ledger 校验；Tooltip 与未读并列显示最近权限请求/12 类回复失败原因，不输出异常正文、不改写当前 Activity | 无可靠源顺序的 hook 不能证明当前仍在等待权限/失败；事故机事件来源、排序与活动一致性仍待证 |
| Codex 原生更名 | 关闭且来源已核实的历史，以及打开 `resume-id` 标签明确选定的恢复来源，经官方 app-server thread/name/set 并读回。打开标签保存前核对冻结 run 根与最新 profile revision、项目、root、ID、key，实际使用 profile SourceRef | 恢复来源不等于当前终端会话；new/raw/picker 不猜目标，Rust Run scope 继续拒绝。真实运行并发和其他 TUI 即时刷新未验收 |
| Claude 原生更名 | 固定官方 Agent SDK 0.3.301，本地 metadata 单次写入后官方读回；精确 scope/profile/project/root/ID/key/file identity，私有同卷硬链接。保留完整原生 TUI | 需要既有 Node ≥22.13；不自动安装。空闲真实 2.1.296 TUI 恢复通过；忙碌 writer、事故 2.1.301 与安装包 WebView2 未验收 |

### Claude 更名的证据与未验证边界

当前实现已按用户授权接入官方 SDK `renameSession`，未改为 stream-json 宿主或注入按键。helper 清空继承凭据和 `NODE_OPTIONS`，只读写精确文件的私有 namespace，不访问默认配置根、不调用模型 API。Node 文件权限限制不等于全面网络封禁。写入未知时只允许读回协调，不能重试写入；确认成功后才清除仍匹配的历史/终端别名，同根 Legacy alias 另须新鲜后端根证明。多 profile 同一物理记录的显示折叠与缓存刷新不转移授权或进程 owner。

真实 Claude **2.1.296** 在私有 ConPTY 内已加载合成历史问答；保留该进程时调用 SDK 更名，读回成功，原两条 JSONL 字节前缀完整且仅追加一条 custom-title。停止并恢复后，终端仍显示原问答及新 OSC 标题。只处理了该临时目录的 trust、明确假 key 本地确认和终端协议应答，未发送 prompt/query/turn，没有模型费用。原始日志的简单字符串检测曾因 ANSI 光标转义误报 prior-answer=false，去除 CSI 后的首开/恢复文本均能核实原问答。证据：Temp/ccdesk-claude-real-tui/target/debug/fixture-34540/reviewed-evidence.json、首开/恢复 ANSI 文本。

这项实测只覆盖空闲 TUI；未证明真实忙碌模型输出与 SDK 并发追加安全，也未认证另一台事故机 CLI 2.1.301。测试为强制终止后的恢复，Claude 出现 classic renderer fallback，不能把它写成无异常完整生命周期认证。

#### 接入之前的调查记录（保留失败及风险证据）

不是缺少官方 API：SDK 0.3.301 定义同一 stream 内的 rename_session 控制请求（title、source、session_id），也提供 renameSession 本地历史元数据函数。前者服务于 stream-json/print 宿主，尚未找到能透明附着当前普通交互 PTY 的公开入口；Remote Control 需要用户启用和远端授权，claude attach 面向后台会话，不能据此自动接管现有终端。[CLI reference](https://code.claude.com/docs/en/cli-reference)、[Remote Control](https://code.claude.com/docs/en/remote-control)、[Agent view](https://code.claude.com/docs/en/agent-view)

官方 SDK `renameSession` 没有声明禁止对活跃会话更名，也没有给出与真实 CLI 并发写入的保证；不能把文档未承诺当成禁止。其已检查实现以 append 方式写入完整 Buffer 并处理短写，未取得实际 CLI 2.1.301 writer 的单记录写入粒度、短写与跨写入者协调证据。[SDK changelog](https://github.com/anthropics/claude-agent-sdk-typescript/blob/main/CHANGELOG.md#0274)

文件 API 的**条件反例**在独立 Temp 夹具复现：先写有效用户行，再由已持有 append fd 的**合成写入者**写半条 assistant JSON；真实 SDK helper 追加 custom-title 并返回成功；合成写入者补齐尾部。最终三行 JSON.parse 结果为 [true,false,false]。它只证明该人为拆写条件下会发生交错，尚未证明真实 CLI 使用这种写入方式，不能写成真实 CLI 已损坏、必然损坏或官方禁止活跃会话。硬链接和 EOF 预检不能修复该条件反例，但不据此断言所有实际 writer 不安全。

该早期候选当时已撤回，未修改用户历史；随后经用户授权重新按上述固定 SDK 与精确文件隔离方案接入。条件反例脚本仍保留在：C:/Users/wusha/AppData/Local/Temp/ccdesk-sdk-inspect/sdk-partial-line-probe.mjs、同目录 .log；早期候选保存在 rejected-candidate/。官方 /rename 名称也支持 -p，但另起 -p --resume 不等于附着当前写入者，不能跳过并发边界。[Commands](https://code.claude.com/docs/en/commands)

人工兜底仍是定位原会话终端，由用户处理现有草稿并手动提交 /rename 名称；关闭会话先按原来源恢复。此流程不等于 SDK 自动路径验收，未读回成功不得落别名或宣称更名成功。宿主不会自动清空草稿、停止用户进程、注册远端会话或启动额外模型 query。

当时的编辑器核对发现 Ctrl+S 确有草稿保存/恢复能力，但空输入且已有 stash 会恢复旧草稿，Agents/搜索上下文与重绑定也会改变动作；权限上下文 Enter 会确认权限。因此不能把固定按键序列当成更名事务。同进程 `rename_session` 可以绑定 session_id，但普通 PTY 附着入口仍未知。早期私有 Claude 2.1.296 print/stdin 探针只发 initialize 后超时，未执行更名；该失败记录与后来的 SDK/真实 TUI 成功证据分别保留。[Interactive mode](https://code.claude.com/docs/en/interactive-mode)、[Keybindings](https://code.claude.com/docs/en/keybindings)

## Native 与 Legacy 的区分

这区分的是 Desk runtime，不是“官方 CLI 创建”与否。Native 行用 runtime=native-cli、profile/revision 和 request/run/generation；Legacy Claude 行用 runtime=legacy-claude、旧 tab/PTY 身份。两者可能读取同一默认 Claude 历史。unifiedSessions.refresh 仅在 backend 物理根证明、CLI、项目路径、原生 ID 全匹配且来源唯一时去重；开着的旧终端保留原 owner。外部会话按被授权的 profile/root 进入历史，不因为“外部”就自动变为兼容会话。

当前默认新建走 Native；Legacy 用于保留已有终端、历史与旧配置兼容，不要求用户选择一种新的 CLI 类型。同一物理会话经不同 runtime 打开确有不同实现。渐进统一应先用上述工作流矩阵做等价验收，再迁移历史投影/显示元数据，最后退役无活跃 owner 的兼容路径；不能按路径或 UUID 猜测后直接删旧路径。

事故机最小安全补证是：CLI 版本、observer 配置布尔值/固定原因码、服务是否就绪、插件准备结果、事件是否到达与新建/恢复各一次结果。不要导出正文、完整环境、argv、headers 或凭据。WSK 与隔离夹具无法证明另一台机器的服务、插件、策略和 WebView 实际行为。


## 最终验证与交付边界

### 官方 Claude SDK、重复显示与生命周期收口（2026-10-16）

当前在同一未提交分支与基线 `f3f0b9e7c495a0e9bc7aa0b008887b3b75094b95`，package 仍为 0.20.2。固定 SDK 0.3.301 已接入；未提交、推送、合并、安装或发布，未更改真实历史、真实配置或全局信任状态。

隔离回归先复现更名/并发刷新后的跨 profile 同一物理记录重复显示（预期 1、实际 2）。修复按 CLI/root/注册项目/path/native ID/session key 完整身份折叠闭合行，保留多个打开 owner、不同归档组及不同来源。成功后刷新每个精确物理来源缓存，清理仍属于该记录的打开标签别名；同根 Legacy alias 另须后端新鲜根证明。未用标题匹配或裸 UUID 扫描。Claude/Codex 均覆盖更名→并发刷新→关闭→恢复→应用 store 重启，以及未知失败保留旧 metadata、不自动重试写入。此复现并不能证明用户现场重复显示或误归档的事故根因。

最终增量验证如下（日志位于 `C:/Users/wusha/AppData/Local/Temp/`）：

| 验证 | 实际结果 | 日志 |
| --- | --- | --- |
| 完整 Vitest 四组 | 183 文件，2720 通过、2 原有跳过；含真实子进程及完整 5000 会话/30 owner/120 次变化 | ccdesk-claude-rename-final-all.log |
| Node 更名和 CI 策略 | 15/15 | ccdesk-claude-rename-final-node.log |
| 类型与生产构建 | 均通过；原 chunk size 提示仍在 | ccdesk-claude-rename-types3.log、ccdesk-claude-rename-build-final.log |
| Rust 更名/registry/service | 14/26/30 项通过，实际 SDK 文件写入与回读 | ccdesk-claude-rename-final2.log 及 backend delivery manifest |
| 独立 scope-core | Windows 正常宿主完整 183/183，无忽略/过滤；fmt 与严格 all-target Clippy 通过 | ccdesk-scope-core-windows-host.log、ccdesk-scope-core-fmt.log、ccdesk-scope-core-clippy-final.log |
| 独立只读审查 | 当前生产实现无剩余阻断发现；独立两文件 63/63 通过 | 审查代理最终报告 |

scope-core 旧接线先出现编译错误；现整文件引用真实模块，对跨入 Tauri 大模块的少量生产项按 Rust AST 原样选择，不改函数体、不替换权限/锁/SDK。三平台矩阵继续完整测试和严格 Clippy，并准备同一 SDK bundle。沙箱内首次 181 通过、2 个 Windows 私有 namespace 准入失败；独立临时探针证实打开随机 Temp 父目录返回 NTSTATUS `0xC0000022`。随后通过正式 `exec_command` 的 `sandbox_permissions=require_escalated`，附临时测试用途说明并获自动审批允许，完整原命令重跑 183/183。没有明确拒绝后继续执行，没有修改 token、ACL、目录准入或断言。失败原日志保留：ccdesk-scope-core-rewired2.log、ccdesk-scope-core-namespace-diagnostic.log。

当前尚无本轮 Linux/macOS 实际运行、安装包 WebView2、真实忙碌模型输出并发写入或事故 CLI 2.1.301 证据。此前视觉/CSP 等检查点在下文单独记录，不能当作本轮 SDK 或目标平台认证。主 crate Rust 本轮只运行上述定向库存；scope-core 183 项也不能代替主 crate Rust 全量。官方 SDK 追加的合成拆写条件反例仍有效，未证明实际 CLI 使用该模式，不抹除也不升级为真实损坏结论。

### Codex 恢复来源更名增量（2026-10-16）

本次小范围实现只扩展已有原生元数据路径：菜单和编辑器明确说明“恢复来源会话”，保留来源 ID；保存前重新核对冻结 run 来源与最新 profile revision、项目、物理根、ID、key，调用 profile SourceRef 的 `cli_rename_session`，由原有官方 app-server read/name-set/read 确认。即使终端切换会话，目标仍是原恢复来源；new/raw/picker 不猜，Rust Run scope 拒绝保持。原生成功后才清历史、已知 ID tab、早期 null-ID tab 的匹配显示覆盖，失败和未知结果不保存新别名、不重放。

- RED：新增 adapter 场景 10 项失败、双语组件 2 项失败，均因旧入口拒绝或目标文案缺失。GREEN：受影响 61 文件 857 通过、2 原有跳过；之后追加的两条审查建议由更名文件 27/27 单独验证。日志：ccdesk-codex-source-rename-vitest2.log、ccdesk-codex-source-rename-review-regressions.log。
- 类型检查和生产构建通过，原有 chunk size 提示保留。日志：ccdesk-codex-source-rename-typecheck2.log、ccdesk-codex-source-rename-build.log。
- 独立 Node 更名策略 9/9。旧提取执行器初次遇到新增局部 TS 类型报解析错误，已使用现有共享 AST 编译器编译真实函数体，不改正文/分支。日志：ccdesk-codex-source-rename-node.log（失败保留）、ccdesk-codex-source-rename-node2.log。
- 已安装 Chrome 的 tooltip/menu 两项 2/2，命令最终 exit 0；自建视觉服务器退出等待较长，核实 PID/创建时间后仅结束该 owned fixture server 才完成收尾。不是完整视觉基线认证。日志：ccdesk-codex-source-rename-browser.log。
- Codex 0.162.1 全新私有夹具 initialize/read/name-set/read 成功，前后 notLoaded；rollout 564 bytes 完全相同，SHA-256 为 5a1d460c1fe5d274b06b3ef44d2b9a4aec08e4d7618657e91b07ac32d5bf4b7c。未 start/resume/turn，无模型请求；不宣称活动并发或另一 TUI 即时同步已验收。日志：ccdesk-sdk-inspect/codex-name-probe-resume-source-final.log、codex-name-probe-resume-source-bytecheck.log。
- 独立只读复审未发现阻断，git diff --check 通过。本增量没有改 Rust、提交、推送、安装或发布；以下完整 Vitest/Rust/生产 CSP 结果属于上一阶段检查点，本次未重复全量。

### 上一阶段完整检查点

代码冻结于分支 fix/session-observation-refresh-index，基线 f3f0b9e7c495a0e9bc7aa0b008887b3b75094b95。package 仍为 0.20.2；未提交、推送、合并、安装或发布。原始事故机数据、归档键、配置均未由本轮修改。

| 验证 | 实际结果 | 日志（C:/Users/wusha/AppData/Local/Temp/ 下） |
| --- | --- | --- |
| 完整 Vitest 四组 | 183 文件、2671 通过、2 原有跳过，0 失败；保留真实进程与 5000 会话/30 owner/120 变化工作量 | ccdesk-final-all2.log |
| 类型 / 生产构建 | 均通过；原构建 chunk size 提示保留 | ccdesk-final-types3.log、ccdesk-final-build2.log |
| 独立 Node 更名策略 | 9/9 | ccdesk-final-rename-node.log |
| 视觉逻辑 | 66/66 | ccdesk-final-visual-unit.log |
| 本机 Chrome 受影响用例 | 10/10；不含不可变全套截图认证 | ccdesk-final-browser2.log |
| 正常生产入口严格 CSP | 英文、中文 2/2；实际重新构建，含异步 Settings | ccdesk-final-production2.log |
| Rust 12 个过滤组 | 288 个互不重复通过、2 原 benchmark 忽略；不是 Rust 全量 | ccdesk-final-rust-inventory.json（含各组命令、完整名称与日志） |
| Rust 静态检查 | fmt、严格 all-target Clippy 通过 | ccdesk-final-rust-clippy-all-targets.log |
| 独立复审 | 最终代码未发现新增阻断；不包含 Claude 未实现自动路径的准入 | 审查结论保留在本任务记录 |

失败记录没有抹掉：第一次最终全量有 14 项旧“GUI 保存别名”预期失败，分属 4 个文件；已迁移原生成功/显式拒绝及旧 metadata writer 兼容性，相关 116 项和后续全量通过。两处迁移测试未使用局部变量导致的类型错误已清理，最终类型检查通过。首轮浏览器因默认 Playwright headless shell 不存在而未启动；设置已安装 Chrome 的绝对路径后 10+2 项通过，未下载或安装浏览器。

原两个 Rust 忽略项为 BenchmarkIndexParseCost_002、BenchmarkIndexSizes_001。严格 Clippy 首轮新增两项 style/constant-assertion 问题已等价修正；最终 all-targets 通过，首轮日志仍保留。

Windows 实测涵盖真实 child terminate/reap/retire、旧控制资源拒绝、只读来源继续核对，以及真实 junction 重指向拒绝。观察安装的 4 项 PTY 测试使用 ConPTY+Node fixture，不能冒充真实 Claude/Codex。原生 Codex 隔离验收仅涉及 read/name-set/read，没有 start/resume/turn。Claude SDK 交错写入实验的另一写入者为合成夹具。未运行 Rust 全库存、Unix/macOS 平台、安装包 WebView2、真实 CLI 完整 Layer-C 或事故机 observer 补证。

若首次启动时原生配置根尚不存在，冻结来源能力可能未知；CLI 启动不因此失败，但自动续接必须显式拒绝缺失来源证明，不能退回默认根或悄悄新建对话。历史不完整横幅的现场具体错误码仍未知。没有归档操作审计，不能把已修复的交互风险认定为本次事故根因，也不能归因于用户操作或 PR56。


## 13 项历史像素差异逐项收口

HEAD `f3f0b9e7c495a0e9bc7aa0b008887b3b75094b95`；当前未提交工作树；Chrome `155.0.8059.39`；UTC固定夹具日期 `2026-09-28T12:00:00Z`；每项按原spec的viewport/locale/theme/DPR及准备动作，两个只读Vite server分别4188/4189。沿用历史测试UA151字符串，实际引擎155，两侧一致。未触碰生产源码或既有PNG。

结果：8项HEAD/current字节一致，5项为预期会话行操作布局变化，未发现新的阻断像素回归。旧批准基线与同环境HEAD的13项全部仍不匹配，具体生成环境原因未认证；不得据此称13项历史像素gate通过。26张PNG及两侧DOM快照在本目录。所有页面无pageerror、blockedHostCalls=0；13张批准PNG均与git HEAD字节相同。

|场景|当前/HEAD不同像素|旧基线/HEAD不同像素|判定与现象|
|---|---:|---:|---|
|workspace-empty-1024-zh|0|9354|当前与HEAD字节一致；空项目、中文空态及导航没有本补丁像素变化。|
|workspace-mixed-1366-zh|2145|31651|预期：会话行改为五列；标题更早省略，时间区左移约26px；opened失败行主操作由重试改为关闭。侧栏/项目/终端外框不变。|
|workspace-hover-action-1366-en|4493|33820|预期：hover行时间继续显示，关闭在固定独立列显示；标题与完整名称Tooltip位置因可用宽度调整；其他行五列变化同mixed。|
|workspace-resources-overlay-1024|0|443756|当前与HEAD字节一致；资源Drawer、遮罩、只读资源卡片没有本补丁像素变化。|
|projects-150-percent|0|42975|当前与HEAD字节一致；DPR1.5下项目列表布局和操作区没有本补丁像素变化；新增关闭入口藏在未展开菜单，不在本图。|
|new-session-dialog|0|381824|当前与HEAD字节一致；新建对话框、表单及遮罩没有本补丁像素变化。|
|archived-sessions|534|731155|预期：统一五列预留尾槽，归档抽屉中两行的时间和恢复按钮约左移26px；名称、Drawer、关闭按钮及项目未变。恢复按钮仍完整可见。|
|settings-terminal-light-gui-dark-terminal|0|53552|当前与HEAD字节一致；终端设置页、预览及light GUI没有本补丁像素变化。|
|settings-launch-configurations|0|26261|当前与HEAD字节一致；dark GUI配置列表/编辑器没有本补丁像素变化；该图未展开的控件不据此认证。|
|confirm-stop-and-archive|0|557326|当前与HEAD字节一致；停止并归档确认框及背景没有本补丁像素变化。|
|workspace-menu-1024-en|1367|43516|预期：背景会话行五列排布与opened失败行主操作变化；已展开的会话菜单文字、位置、尺寸和焦点项无变化。|
|workspace-dark-gui-light-terminal|1971|32910|预期：dark GUI会话行五列排布与opened失败行主操作变化；light终端内容及外框不变。|
|tooltip-transformed-1024|0|24740|当前与HEAD字节一致；变换容器下Tooltip位置、内容、焦点没有本补丁像素变化。|

证据边界：这是合成视觉夹具与Chrome宿主证据，不代表真实CLI、Windows WebView2、实际安装包或全部241视觉用例通过。
