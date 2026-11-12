# Codex 归档并发边界（0.27 TODO2）

本记录区分生产防护、固定 CLI 的隔离实验和剩余验收边界，不把测试夹具视为已安装应用验收，也不宣称任意 CLI 版本都有相同锁实现。

## 现场问题及独立修复

SXY 的只读诊断发现两个其他 Codex owner 仍活着，目标没有匹配进程。rc5 的全局 Codex guard 在该状态必然拒绝归档；没有失败瞬时快照，不能断言它是唯一 blocker。Stop hook 表示回合结束，不表示进程退出。

`038f4aca` 独立修复 Windows `AdmittedChild::try_wait`：portable PTY 已缓存退出码但精确保留的 Windows 进程句柄尚未 signalled 时仍返回 None，防止 command owner 提前离开并留下 Unknown admission。已有 RED/GREEN 和恢复后立即归档的合成协议闭环；不能将退出竞态误报为已确证的现场唯一根因。

## 生产责任边界

选定 CLI 的原生归档语义是产品边界。Desk 用独立 app-server 初始化、只读检查并发送一次 `thread/archive`，不连接已有 app-server、不发送 resume、Shutdown 或 stop，不为了归档成功接管其他 owner。外部或未观测到的原生 writer 冲突由该 CLI 自己处理，不宣称 Desk 在操作系统层隔离了任意外部进程。

Desk 追加以下本地拒绝保护：

- 结构化 Codex run 绑定独立 backend identity。请求中的 resume UUID 只是保守提示，单独不能放行；必须从该 run 的有效认证 hook 收到真实 UUID。raw/Shell、准备期未识别、无 hook/未知 owner、未收口写入与维护冻结仍拒绝。Legacy Claude 和已识别 Native Claude/OpenCode 的原有豁免不变。
- ID 只追加、不因 Stop 或新 SessionStart 覆盖。每 owner 最多 64 个 UUID，溢出永久降为未知直到精确进程回收；快照合计最多 128 个 UUID。迟到或重放 hook 不重建票据。
- 已知 owner 含目标 UUID 直接 Busy。对其余已知 owner，独立 helper 通过 `thread/read` 的精确 `thread.id` 和显式 `parentThreadId` 读取双方祖先链。目标、祖先或后代关系拒绝；兄弟和独立会话允许。缺字段不等于无父关系，错误 ID、查询失败、环、深度超过 64 或总读取超过 128 均拒绝。
- 不解析 CLI 版本号、命令字符串或 profile 路径来伪造有效根/写入范围。其他数据根的 UUID 若在目标 helper 中无法核对，仍拒绝而不假定无关。hook 不是完整 writer 清单，parentThreadId 也不是未来关系变化的事务锁；这部分仍依赖选定 CLI 原生语义。
- 归档期间冻结新 Desk admission 和元数据写入。开始时固定 owner/epoch，读取阶段、发送前后和 helper 回收后反复检查；身份、owner 或 epoch 改变便拒绝或记 Unknown。只在短锁内标记发送阶段，管道 I/O 不持 admission/identity 锁。
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
