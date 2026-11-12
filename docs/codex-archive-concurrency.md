# Codex 归档并发边界（0.70 TODO2）

本记录区分已交付的生命周期修复、固定 CLI 的隔离实验，以及尚未完成的跨版本并发能力。它不是安装版验收，也不是放宽生产准入的授权。

## 已修复的独立生命周期问题

`038f4aca` 修正 Windows `AdmittedChild::try_wait`：便携 PTY 已缓存退出码，但保留的精确进程句柄尚未发出退出信号时，仍返回 `None`，不能让 command owner 提前离开并留下 Unknown admission。该修复有先失败后通过的定向回归，以及恢复后立即再归档的合成协议闭环。

这个竞态不能被当作现场唯一原因。现场只读诊断发现其他仍存活的 Codex owner；现有全局 Codex guard 在该状态下本来就会拒绝归档。没有失败瞬时快照，不能断言当时只有一个 blocker。

## 当前生产边界

`version_history/maintenance.rs` 的 Codex 归档准入只排除已识别的 Legacy Claude、Native Claude/OpenCode；Codex、Shell/raw、准备中未识别 owner、Unknown owner、未完成写入和维护冻结仍阻止归档。永久删除仍要求全部 owner 空闲。

下列已有证据不能提升为“与目标及其子树无关”的写权限证明：

- `SourceProvenance` 和 `read_scope_known` 只授权读取冻结环境所选目录；根句柄证明目录身份，不证明 CLI 最终使用该目录。它们也允许 Cmd shim。
- `RunPublicIdentity::effective_config_root` 仍是 Unknown。认证 Codex hook 只提供事件名和 session UUID，没有有效数据根或完整后代写入范围。
- 精确进程 HANDLE 证明生命周期，不证明文件访问范围。
- 新归档 helper 的 `initialize.codexHome` 只证明该 helper，不能替其他已运行 TUI owner 提供证明。

因此，本分支不按 profile 名、路径字符串、当前 UUID、不再输出内容或 Stop hook 放行其他 Codex owner。也不根据 0.167 的表现推断所有旧版、新版或自定义 launcher 都使用同一把原生锁。

## 固定版本隔离实验

入口为 `NativeArchiveLive_InstalledCodexLockMatrix_001`，默认忽略；必须显式设置 `CC_DESK_NATIVE_ARCHIVE_LIVE_GO=archive-lock-matrix`。测试复用已有 `native_cli_delete_live` 隔离设施：

- 固定已审核 Codex 0.167.0 二进制 hash，执行期间持有二进制及配置租约。
- 清空继承环境，使用全新 HOME、CODEX_HOME、SQLite、缓存、项目及日志目录；先后检查机器级配置不存在。
- 仅初始化、读取和归档合成会话，不登录、不请求模型，不启动或停止任何用户会话。
- 父子关系只写入本测试新建的 SQLite；原生 `thread-writer-locks/<id>.lock` 使用真实 Windows 排他文件锁。
- 无关锁、根锁、后代锁分别验证父子原文件及归档位置，控制会话必须逐字节不变；退出后核对自有进程组和 admission 已收口。
- 负例释放锁后，重建仅属于该临时实验的服务并重新授权同一合成来源，再做成功正对照。这不是生产 UNKNOWN 可重置或自动重试的实现。

实验观察的是 **真实 CLI + 合成写锁**，不是正在运行模型的 writer。生产错误会泛化为 `ARCHIVE_OUTCOME_UNKNOWN`，所以不能把错误码本身称为明确 writer-conflict 响应。释放锁正对照用于排除该 fixture 根本无法归档的解释。

2026-11-12 WSK-MSI 实测通过：1 个定向测试，3 个持锁场景和 2 个释放锁正对照，运行 124.64 秒。

- 无关锁：父会话与已知子会话成功归档，归档内容逐字节相同，控制会话未变化。
- 根锁：返回 `ARCHIVE_OUTCOME_UNKNOWN`，父子原文件未变化且归档目标不存在；释放锁后，同一合成父子归档成功。
- 后代锁：同样保留父子原文件；释放锁后，同一合成父子归档成功。
- 每个场景的自有进程组和 admission 均收口，锁持有期间独立句柄竞争验证均成立。

准备命令为仓库根的 `node scripts/prepare-conpty.mjs` 和 `node scripts/prepare-session-sdk.mjs`；隔离 worktree 还需可用的已安装 `node_modules`，不能省略 Tauri 资源或测试中的固定 SDK 内容检查。然后在 `src-tauri` 中设置上述 GO 环境变量并执行 `cargo test --locked --lib NativeArchiveLive_InstalledCodexLockMatrix_001 -- --ignored --nocapture --test-threads=1`。此次前两次重试因新 worktree 缺少本地资源而停在编译期，没有执行 CLI；准备完整后的矩阵只运行一次。

格式检查、`git diff --check` 与 `cargo clippy --locked --lib --tests -- -D warnings` 通过。未运行全量测试、安装版 WebView 验收或其他 CLI 版本。

## 后续设计门槛

TODO2 的“无关活跃 Codex owner 不阻挡”尚未完成。可选路径是运行实例提供持续有效的根和写入范围能力，或所有有关 writer 与归档 helper 参与同一互斥协议。0.167 的 initialize schema 没有协商归档互斥能力的字段。

按精确执行文件身份进行隔离并发能力验收可避免固定版本号白名单，但必须同时验证 writer 和 helper，绑定所有活跃 owner，并在映像、根或能力证据失效时安全拒绝。单独验证一个新 helper 或把本实验通过直接当作生产认证都不够。

固定源码参考：`openai/codex` commit `086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a` 的 `codex-rs/thread-store/src/local/archive_thread.rs`、`codex-rs/rollout/src/writer_lock.rs`、`codex-rs/app-server/src/request_processors/thread_processor.rs` 和 `codex-rs/app-server-protocol/src/protocol/v1.rs`。
