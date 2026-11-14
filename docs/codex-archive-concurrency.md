# Codex 归档并发边界（0.27 TODO2）

本记录区分生产防护、固定 CLI 的隔离实验和剩余验收边界，不把测试夹具视为已安装应用验收，也不宣称任意 CLI 版本都有相同锁实现。

## 现场问题及独立修复

SXY 的只读诊断发现两个其他 Codex owner 仍活着，目标没有匹配进程。rc5 的全局 Codex guard 在该状态必然拒绝归档；没有失败瞬时快照，不能断言它是唯一 blocker。Stop hook 表示回合结束，不表示进程退出。

`038f4aca` 独立修复 Windows `AdmittedChild::try_wait`：portable PTY 已缓存退出码但精确保留的 Windows 进程句柄尚未 signalled 时仍返回 None，防止 command owner 提前离开并留下 Unknown admission。已有 RED/GREEN 和恢复后立即归档的合成协议闭环；不能将退出竞态误报为已确证的现场唯一根因。

## 生产责任边界

选定 CLI 的原生归档语义是产品边界。Desk 用独立 app-server 初始化、只读检查并发送一次 `thread/archive`，不连接已有 app-server、不发送 resume、Shutdown 或 stop，不为了归档成功接管其他 owner。外部或未观测到的原生 writer 冲突由该 CLI 自己处理，不宣称 Desk 在操作系统层隔离了任意外部进程。

Desk 追加以下本地拒绝保护：

- 结构化 Codex run 绑定独立 backend identity。请求中的 resume UUID 与认证身份分离，只能保守拒绝同 UUID 目标，不能参与关系查询或证明实际身份。等待认证 hook 的结构化 owner 不再全局阻断归档；它与外部 writer 一样由独立官方 helper 的目标/原生子树 writer 锁处理，不被宣称为无关、空闲或无写入。raw/Shell、准备期未识别、未知进程生命周期、身份溢出/不一致、未收口写入与维护冻结仍拒绝。Legacy Claude 和已识别 Native Claude/OpenCode 的原有豁免不变。
- ID 只追加、不因 Stop 或新 SessionStart 覆盖。每 owner 最多 64 个 UUID，溢出永久降为未知直到精确进程回收；快照合计最多 128 个 UUID。迟到或重放 hook 不重建票据。
- 已知 owner 含目标 UUID 直接 Busy。对其余已知 owner，独立 helper 通过 `thread/read` 的精确 `thread.id` 和显式 `parentThreadId` 读取双方祖先链。目标、祖先或后代关系拒绝；兄弟和独立会话允许。缺字段不等于无父关系，错误 ID、查询失败、环、深度超过 64 或总读取超过 128 均拒绝。
- 不解析 CLI 版本号、命令字符串或 profile 路径来伪造有效根/写入范围。其他数据根的 UUID 若在目标 helper 中无法核对，仍拒绝而不假定无关。hook 不是完整 writer 清单，parentThreadId 也不是未来关系变化的事务锁；这部分仍依赖选定 CLI 原生语义。
- 归档期间冻结新 Desk admission 和元数据写入。开始时固定 owner/epoch 及逐 owner 的完整身份状态，读取阶段、发送前后和 helper 回收后反复检查；等待身份变为已认证也会撤销快照，即使合并 UUID 集合未变。身份、owner 或 epoch 改变便拒绝或记 Unknown。只在短锁内标记发送阶段，管道 I/O 不持 admission/identity 锁。
- 每次只允许一条归档帧； bounded writer channel 与读取共用 30 秒截止时间。确定发送前拒绝才返回 Busy/`ARCHIVE_RELATION_UNAVAILABLE`。可能发送、原生错误、丢 ACK、超时或写后证据失效都保留 `ARCHIVE_OUTCOME_UNKNOWN`，不能说成“未执行”，不能自动重发。
- 成功需 native ACK 加同源根的物理归档读回。仍只证明 root，不能声称逐个后代都已验证。Unknown 按原有只读核对机制处理；已归档快路径、恢复前收口和 history 收口保留全 Codex 冻结，不绕过关系检查清除未知屏障。
- 只回收本次独立 helper 的精确 Child/Job。永久删除仍要求所有运行时空闲，本次没有放宽删除。

## 固定版本实验

### 合成 writer 锁

`NativeArchiveLive_InstalledCodexLockMatrix_001` 默认 ignored，需显式 `CC_DESK_NATIVE_ARCHIVE_LIVE_GO=archive-lock-matrix`。固定 Codex 0.167.0 映像 SHA-256，完全合成 HOME/CODEX_HOME/SQLite/项目/日志，env_clear、file 凭据存储、插件和遥测关闭、不登录、不发模型 turn。只对测试新建 SQLite 写入父子边，用真实 Windows `File::try_lock` 持有该版本原生 writer-lock 文件。

2026-11-12 WSK-MSI 实测：1 项矩阵通过，3 个锁案例及 2 个释放后正对照，124.64 秒。无关锁下 root 与 child 归档，根锁/子锁下返回 Unknown 且原字节保持，释放后同一合成目标成功。重建 service 仅为隔离实验排除测试残留，不是生产 Unknown 重试方案。这是“真实 CLI + 合成文件锁”，不是已加载 native writer。

### 已加载原生 writer

`NativeArchiveLive_LoadedWriterScopedMatrix_002` 默认 ignored，需显式 `CC_DESK_NATIVE_ARCHIVE_LIVE_GO=archive-loaded-writer-matrix`。另一个独立、精确 Job 持有的真实 app-server 仅 `thread/resume` 新建合成会话、不发 turn，并独立确认 native writer lease 确实争用。归档后检查 writer 仍活且仍持锁，最终 EOF 后回收自己的 Child/Job。

Windows KnownFolder 不受 USERPROFILE 覆盖，因此启动前后必须确认真实 KnownFolder Profile/.agents/skills 不存在；若存在、reparse 或状态未知则拒绝，不能删除/改名用户目录，也不能关闭该门槛。固定源码中的其他 skills 根来自隔离 CODEX_HOME 或已检查缺失的 ProgramData/OpenAI/Codex；file 凭据模式不进入 keyring。这不是任意文件访问的 OS 沙箱。

本次管理员身份的前置门槛失败，未启动 CLI。改用默认受限测试身份后原门槛通过。首轮 known-target 得到 Busy；known-descendant 因夹具仅写 DB 边、header 仍声明独立 cli，原生 read 给 null 父关系，最后原生锁产生 Unknown。该失败如实保留，夹具随后补齐官方 parent_thread_id/subagent header，不降低预期。第二轮前三例通过；外部目标在发送前暴露 Windows canonical rollout 路径差异（SOURCE_PATH_REJECTED）。复用已独立验证的路径修复 d4ed8031（本分支等价 cherry-pick 87cfeaa4），只对授权根 canonicalize 并重核目录 identity，不规范化不可信文件路径。最终矩阵结果见本节后续验证记录。

已知 owner 案例用合成 Desk admission 加真实 loaded writer，不是实际 TUI hook 到 Desk 的端到端安装验收。固定 CLI 的结果不能泛化为所有旧版、新版或自定义 launcher。

## 验证记录

- 最初 scoped admission RED：3 项应允许的已识别无关 owner 被旧 guard 拒绝，2 项未知边界通过。
- 前端新增错误码 RED：2 项失败；修复后 API/用户错误定向 36 项通过，typecheck 通过。
- 第一轮 Rust NativeArchive：27 项通过、2 个显式 live 实验 ignored；包含关系链、认证 hook、身份变化、精确退出、原有恢复后立即归档和丢 ACK 屏障。
- 只读复审发现并修复已归档快路径绕过完整关系检查的 Unknown 收口问题，恢复严格冻结。另补 blocked writer 通道可响应/超时与测试 Job 分配失败精确回收。
- 最终 scoped 增量：NativeArchive 28 项通过/2 ignored，认证 observer 24 项通过，HistoryRuntime 17 项通过/2 ignored；blocked writer 与 Unknown 保留断言通过。前端归档/投影/API/错误 51 项通过、typecheck 通过；fmt 和严格 all-target Clippy 通过。复制测试 binary 的 lifecycle 首跑因缺少 ConPTY license 被资源门槛拒绝，补齐官方固定资源后复跑通过，未降低资源校验。
- 集成路径依赖后再次 NativeArchive 28 项通过/2 ignored，前端 52 项通过，严格 all-target Clippy 通过。按仓库规范将门禁与关系单元测试移入 src/tests 后，又确认同一 28 项与 fmt/Clippy 通过；生产逻辑和 live 夹具不变。
- 最终真实 loaded-writer 矩阵 1 项、5 个案例全部通过，208.38 秒。known-target / known-descendant 为 ARCHIVE_BUSY；known-unrelated 允许 root+child 归档且无关 writer 继续持锁；external-target / external-descendant 为 ARCHIVE_OUTCOME_UNKNOWN，root/child 原字节保持且原 writer 继续持锁。每例精确 root 已回收、Job 已空，主机来源缺失门槛前后均通过。
- live 矩阵使用固定测试 binary SHA-256 70868066c83cac6210462e14102f21c1bce11931ac1536ac96edf9c24f6acabf；该二进制在纯测试文件位置整理前复制，生产代码与 live 夹具和最终提交一致。仍未运行安装版 UI、实际 TUI hook 端到端或全部 CLI 版本。

## 固定来源与将来的更强保证

源码依据：`openai/codex` commit `086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a`：`thread-store/src/local/archive_thread.rs`、`rollout/src/writer_lock.rs`、`app-server/src/request_processors/thread_processor.rs`、`app-server-protocol/src/protocol/v2.rs`、`protocol/src/protocol.rs` 以及 skills host roots。

该版本共享 app-server 的归档会先对其加载的 root/descendants 准备 Shutdown，故 Desk 明确不采用共享实例路径。`ThreadArchiveParams` 只有 threadId，loaded-list 不是稳定快照，initialize 没有持久 writer 范围租约。

将来若需跨版本、跨 owner 的原子 idle 保证，应由原生存储协调者增加显式能力及 `archiveIfIdle`、持续 writer lease、目标树 epoch、无副作用 Busy 和可核对 receipt。这是更强保证的上游方案，不是当前独立原生语义方案的伪造能力或版本白名单，也不是本次生产交付的阻塞前提。

## 归档后目录回读修复（2026-11-13）

归档成功/Unknown 后的同根回读原先逐项目调用独立历史扫描，并发数不受控。后端 registry 仅允许两个扫描；真实 history store 加隔离 IPC 的八项目回归在修复前得到两个正常读取、六个 SOURCE_BUSY，复现来源警告。现在同配置/修订项目使用已有 loadGrouped，八项目一次观察完成；三个同根配置仍分别授权，以两组一批收口。不同根不读取，目标必须保持精确根/上下文且读回 archived 才成功。

进行中横幅原先对已发出的归档 RPC 也声称“等待工作区连接”，现改为中性处理中。该横幅不证明连接失败。上述回归证明重复扫描和自触发来源忙碌已消除；不等于复现用户现场的约 30 秒总耗时，未操作真实用户会话，也未替换安装版或证明端到端耗时。

本次验证：针对性 79 项通过；完整 Vitest 219 文件、3344 项通过、2 项 Windows 原条件跳过（D28_D20_RunRootSymlinkEscapeIsRejected_04、D20_Runner_ExecutionRejectsSymlinkEscape_11），91.77 秒；typecheck 与前端 build 通过。npm 旧配置与 bundle 大小警告仍存在。没有修改 Rust，不将上述结果视为安装版或真实 CLI 耗时验收。

独立审查补充无关项目在合并观察中撤销的边界：整组拒绝时，只对准确目标作一次新的只读回读，不重放归档；目标原有来源/所有权检查仍必需。新增回归先复现 ARCHIVE_DISPLAY_SYNC_FAILED，再验证成功目标不被无关撤销误报，撤销项目不复活。

## 结构化 Codex 等待身份修复（2026-11-14）

rc.8 的 scoped admission 仍把任意未收到认证 hook 的结构化 Codex run 当成全局障碍。现场只读 UI 中的三个 Codex 终端均显示空闲但等待身份；精确 rc.8 门禁隔离复现确认，无关 resume 意图在收到认证前拒绝所有目标，认证后才允许无关目标。这是额外 admission 限制，不是目标历史损坏的证据；未重放现场真实归档，也未关闭任何用户终端。

本次只移除 awaiting-identity 的全局否决，不把它升级为认证身份。请求 ID 仍可保守拒绝自身，实际已认证身份才进入祖先/后代预检；每个 owner 的状态逐一比较，防止未知 owner 认证为已存在于合并集合的 UUID 时漏掉撤销。身份溢出、未知进程退出状态、raw/Shell、维护/写入屏障保持原约束。已归档快路径、恢复前 Unknown 收口和永久删除仍保留全 Codex 冻结。

本机已安装 Codex 0.169.0，原 0.167.0 的测试 pin 不能用于声明当前版本通过。共享显式 live 夹具现固定为 0.169.0 映像（382529840 字节，SHA-256 `ce1945eb440b5481ed1233ebaf32b7056ee7e4e452fa62ffe1cbfb26103cdee0`）。本次只复核归档，不据此声明其他 ignored 删除/改名矩阵完成。

官方 0.169.0 的 [thread processor](https://raw.githubusercontent.com/openai/codex/rust-v0.169.0/codex-rs/app-server/src/request_processors/thread_processor.rs) 将原生完整子树传入 writer-lock 清单；[归档实现](https://raw.githubusercontent.com/openai/codex/rust-v0.169.0/codex-rs/thread-store/src/local/archive_thread.rs) 在任何移动前持齐整组锁；[writer lock](https://raw.githubusercontent.com/openai/codex/rust-v0.169.0/codex-rs/rollout/src/writer_lock.rs) 是同根跨进程独占。独立 helper 不恢复/加载用户会话，也不主动接管或 Shutdown 其他 writer。这不构成未来子树关系变化的事务锁，不泛化为任意旧版本或自定义 launcher；成功仍只证明实际物理读回的 root。

当前版本修复前真实 loaded-writer 基线矩阵：5 案例通过，236.92 秒。known-target / known-descendant 返回 Busy；known-unrelated 成功且无关 writer 继续持锁；external-target / external-descendant 返回 Unknown，root/child 原字节不变。全部使用新建合成数据、不发模型 turn，精确 root/Job 已回收。修复后同一 pinned CLI 的 8 案例全部通过，412.47 秒：新增 awaiting-target / awaiting-descendant 同样返回 Unknown 且 root/child 原字节不变；awaiting-unrelated 成功，原无关 writer 继续存活并持锁。所有案例均在请求结束后验证 writer 持锁、原无关数据不变和精确 Job 回收。

修复回归：归档组 34 项通过，3 项按原规则显式忽略；strict `cargo clippy --offline --locked --all-targets -- -D warnings` 和 `cargo fmt --check` 通过。前端归档/adapter/workspace 三文件 113 项通过；`Pointer_ArchiveRestore_002` 补充恢复后关闭抽屉、直接点精确历史行归档，核对第二次 native archive、未打开、未创建/切换 tab、没有普通归档二次确认。Rust `_009` 原有合成 ConPTY 恢复后直接归档链也通过。这些覆盖不等于安装版 UI 或真实 CLI 恢复循环；SXY 历史原因仍未证实，不与本次问题直接合并归因。现场缺少错误发生时逐次诊断快照，本次确认可复现的阻断机制，不宣称是全部历史错误的唯一原因。

全库边界：本轮启动的 1932 项 Rust library 库存仍在执行长耗时 Windows 恢复夹具，尚未得到最终汇总；已观察到 `D11_Channel_Native_011`、`D11_Webview_Live_001`、`D11_Launch_Native_001`、`D12_Webview_FormalCommands_001` 和 `HistoryClassesAlias_HeldDrift_003` 失败。没有放宽断言、忽略规则或 OS 安全设置，也未把这些失败归为已证实的旧失败。本修复按上述独立完成的归档验收交付，不宣称全库通过；完整日志保留供后续收口。
