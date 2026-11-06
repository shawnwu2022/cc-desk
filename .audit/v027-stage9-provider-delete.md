# 0.27 Claude / OpenCode 永久删除：实际反例与准入结论

2026-11-06，独立 clone `E:\Codex\2026-11-06\task-2\cc-desk`、`feat/v0.27.0`，开工 HEAD `3bdc96744df14727efd613322d749fa349c230b4`。用户要求真正永久删除、二次确认和风险提示；不是本次授权清理真实数据。此轮只创建任务合成文件/SQLite，直接调用固定 SDK 或注入服务运行官方算法；没有启动 Claude/OpenCode CLI、HTTP 服务、模型请求，也未扫描真实会话、注册或凭据。

## 实际交付与未交付

- 保留已验证的 Windows Codex 窄产品实现 `6de019a2ee22528d1c391e76c449c968c9846140`，不改共同预览、独立二次确认、一次性账本和 unknown 只读核查协议。
- 新增真实 Windows 文件/目录/旧句柄/权限及固定 Claude SDK 的合成实验，以及固定 OpenCode remove 函数体在合成 SQLite 上的算法反例。它们验证边界，**不是新增两家产品删除能力或完整真实 CLI 验收**。
- Claude 的原始源映射已证实可行，不再把 hardlink rename 局限当作不可解决的来源问题；执行仍缺覆盖动态目录及活动写者的已交付保护。OpenCode 仍缺可复用的已授权现有服务链和拒绝活动、阻止并发启动的实际准入。
- 两家 mutation 保持关闭；这是已复现技术缺口，不是工具审批拒绝、待用户确认意图，也不是缺少 delete API。不会为制造进度添加永远无法签发的 Proof、直接删真实 DB、项目 purge 或隐藏替代。

## Claude：来源能适配，当前保护不能直接用于产品

固定 `@anthropic-ai/claude-agent-sdk@0.3.301`，官方 tag commit [`b0fc766997199a49f90c37f5ec056936c80c9491`](https://github.com/anthropics/claude-agent-sdk-typescript/tree/b0fc766997199a49f90c37f5ec056936c80c9491)，具体包实现 `sdk.mjs` SHA256 `6b963b3eb871949a14392bb35ff970a55e91e2edbe218c61cd9eed60e46cbe45`，同一本地 npm 包类型文件 `sdk.d.ts` SHA256 `26f9a0c91ad64e2020f0d3734932302e3cc09685bddf2b2f9ef60839ce278773`。包内实现、包内类型和公开仓库分别标注，不冒称公开完整 CLI 源码；SDK 许可与下文 OpenCode MIT 不混同。原生先删主 JSONL 再递归删同名目录；无活动锁，错误可能已部分删除。

`src-tauri/src/tests/claude_delete_guards.rs` 仅 Windows 测试模块。所有源是 `TempDir` 新建合成根；固定 SDK 文件先核哈希再复制到任务 helper。Node 开启 permission，不给 child_process/worker 权限，FS 权限只授 helper、新配置根和显式任务内目标；清空环境后 HOME/APPDATA/XDG/CLAUDE_CONFIG_DIR 等全部指新目录。SDK 从 private projects/junction 命名空间调用真实原件；不是只 unlink 私有 hardlink。没有读取真实配置或凭据。

| 实验 | 实际观察和保证限度 |
| --- | --- |
| 001 同文件 writer | READ 句柄、share READ\|DELETE 确实拒绝已有写句柄，并阻止同对象的新写句柄；正常只读可用。不能外推为子目录冻结。 |
| 002 SDK 真删除 | 持有允许 DELETE 的文件 guard 时，SDK 能删合成主记录和子目录。 |
| 003 目录分享 | 即使目录 share_mode(0)，仍能创建新的 child 文件/目录。分享约束的是目录对象，不是整个 namespace 的后代。 |
| 004 同名替换 | SDK unlink 后同名新文件/新 writer 可以出现，旧 guard 仍读旧对象；再次调用 SDK 会删除新名字。旧句柄不能自动继承到替换对象。 |
| 005 部分失败 | child 的 deny-DELETE guard 导致 SDK EBUSY，但主 JSONL 已删除，child 保留。不能把 error 当作无副作用。 |
| 006 原件来源 | task private project junction 加受限 Node FS 权限可以删除真实合成原件；容器身份 guard 能阻止容器替换。不是沿用 hardlink alias。 |
| 007 晚到 child | 已知文件逐个 guarded，仍能创建晚到 child/writer；SDK 删除路径后该 writer 仍可写。只是合成 Node/OS writer 反例，不冒充真实 Claude turn。 |
| 008 child junction | 固定 SDK 递归删除移除合成 child junction，其任务外层合成 marker 保留；不泛化为所有平台/链接类型。 |
| 009 fixture DACL | 新 fixture 目录 deny WD/AD 后，路径创建及旧 READ-only 目录句柄相对创建都被拒绝；旧 ADD_FILE 句柄会使 no-share-WRITE guard 失败。固定 SDK 仍可删除。说明 OS 创建保护有可行局部，不等于外部 CLI 已静止。 |
| 010 旧 DELETE 能力 | 主文件预先打开 DELETE-only 句柄后，即使 project no-share-WRITE/no-share-DELETE、创建 DACL 和文件 writer guard 都在，classic FileRenameInfo 与 FileRenameInfoEx 仍能将原件移到另一个任务目录。同名创建则被 DACL 阻止。 |

Microsoft [CreateFile sharing 文档](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)说明 DELETE sharing 同时允许 rename；[文件安全文档](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights)区分对象 DACL 与目录继承。这里以 OS 实验核实实际组合，没有仅凭文档推断所有权限效果。

**最小后续控制：**需要实际参与 CLI 运行的排他/维护机制，或另行设计并验证覆盖目标及其父 namespace 的 writer/creation 保护和可靠权限恢复。若采用 ACL，不仅要限定原源，还要处理旧能力、动态子目录、进程崩溃/取消后恢复、原 ACL 保真；不能把测试的 Everyone deny/remove 直接迁到用户项目。旧 DELETE 移出反例不证明 Windows 永远不可能实现：未来可用 held-object 身份及 disposition 事后核查避免虚假全删成功，但这本身不能让正在运行的外部会话静止，也不是当前已实现的产品合同。此轮不新增持久权限/特权机制。

## OpenCode：持久 claim 不是排他，CLI 命令仍走服务

固定安装对应 `v2.2.5` / `9bbb8ce18c80f099e0e448775f59d965ead40a21`，并对比后续 `47935cae0d96703dad8b096ab09ee7a07655c7ad` 的同样 remove 顺序：

- [`session.ts:391`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session.ts#L391)：get、interrupt、awaitIdle、cancel jobs、close transport、查询 children、逐项递归、清环境、Deleted/remove。不是 delete-if-idle。
- [`execution.ts:73`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/execution.ts#L73) 确实有 durable write-ahead claim；[`store.ts:218`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/store.ts#L218) 只在 time_suspended 为 null 时 UPDATE，零行是 no-op，没有排他 owner token 或冲突拒绝。
- [`run-coordinator.ts:66`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/run-coordinator.ts#L66) executions 是进程内 Map；[`job.ts:491`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/job.ts#L491) 和 [`model-transport.ts:537`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/model-transport.ts#L537) 只取消该进程拥有的 job/channel。没有据此宣称另一个真实 OpenCode 实例已被实验运行或停止。
- [`CLI delete`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/cli/src/commands/handlers/session/delete.ts#L13) 调用 ServerConnection.resolve 后发送原生 remove；[`连接解析`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/cli/src/services/server-connection.ts#L23) 默认 Service.ensure、standalone 路径会 start。显式既有 --server 是另一条路径，不因此获得来源或活动保护。
- [`server/process.ts:109`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/process.ts#L109) lifecycle 可恢复 suspended sessions，所有服务种类均启动 schedules。不能为清理假装只读启动服务。

Desk 现有 `src-tauri/src/cli/opencode_prepare.rs::with_server` 在操作后 `drop(child)` 并检查端口收尾；`ServerChild` own Job 关闭。正常 NativeRun 持有 TUI，不持有可复用的 managed HTTP endpoint/auth lease；SourceProvenance 不暴露环境或进程控制。因此当前没有可直接复用的长期服务链。不是因为 loopback 或 URL 字段格式不够美观，而是那一实际 owner 已结束，且没有覆盖并发启动的服务准入。

`.audit/v027-stage9-opencode-remove-source.mjs` 保留官方 MIT license，原样抽出 12 行 generator；独立审查逐字对照通过，函数文本 SHA256 `2acba27882f6a9682a6e87675c6a1ae05acc2dc8cffd25a82aa94bcdf88e76b0`。实验以 Node 内置 SQLite 两连接和最小 session_v2 schema，注入 Effect、execution/jobs/transport/bus；实际 SDK/HTTP/完整 OpenCode runtime **没有运行**。Deleted projector SQL 被建模，blob 与完整事件存储没有模拟成实测。五项观察：

1. 最后一次 null claim 读取后，第二 SQLite 连接提交 claim，算法仍删根；foreignExecutionModelRetained 是替身 Set，不是真实外部 CLI 活态证据。
2. children 查询后另一连接创建 child，根删了但 child 留为孤儿；根不存在不能当全树清理成功。
3. 查询包含 archived child，先子后根删除；不相关根保留。
4. 两个物理 DB 的元数据完全一样时，算法删实际 served DB，held chosen DB 原根仍在。是来源契约反例，不是生产 issuer 验收。
5. get 后替身本地 execution 启动，算法会 interrupt 它再删，不能包装成“确认前 idle 所以整个执行不会中断活动”。

**最小后续控制：**在正常、另有授权的生命周期中保留实际服务 generation、选定 profile/source 和限定请求授权；再取得服务中真正覆盖 run/wake/schedule/恢复 admission 的 delete-if-idle 或受控维护机制。可以按用户确认的根及执行时原生子树工作，不单独要求任意 expected-ID 集合协议；来源和活动约束仍不能用确认文案、路径自报或最后一次查询代替。此轮没有为删除新建/保留服务、读取真实注册口令或改上游。

## 验证与失败证据

固定工具链为 `rustc 1.98.1 (48a229cea 2026-09-01)`、`x86_64-pc-windows-msvc`，CI dev/test debug=0、offline，实际 Node `v24.18.0`。从仓库根执行的最终命令如下（直接 Cargo 前本 clone 已按指南准备 ConPTY）：

```powershell
$env:RUSTUP_TOOLCHAIN='1.98.1-x86_64-pc-windows-msvc'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_NET_OFFLINE='true'
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib ClaudeDelete_ -- --test-threads=1 --nocapture
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
node .audit/v027-stage9-opencode-remove-experiments.mjs
node --check .audit/v027-stage9-opencode-remove-experiments.mjs
node --check .audit/v027-stage9-opencode-remove-source.mjs
git diff --check
```

全部最终命令 exit0。Rust 最终 `v027-stage9-claude-guards-final-010.log` 为 **10 pass、0 fail、0 ignore、1787 filtered，3.72s**；strict all-target `clippy-final2.log` 退出0（13.07s），`fmt-final2.log` 明记 exitCode0。测试补强或 SDK 编译期嵌入之后按变更重验；不重复旧全套。完整原始失败仍保留：

- OpenCode `node .audit/v027-stage9-opencode-remove-experiments.mjs --expect-protection`：初轮 4 项 1 pass/3 fail，新增本地晚启动后第二轮 5 项 1 pass/4 fail，均 exit 1。是被反证的安全假设，不能称产品 TDD 修复。
- `node .audit/v027-stage9-opencode-remove-experiments.mjs`：5 pass/0 fail，exit 0，是算法观察成立；日志 `v027-stage9-opencode-remove-observations.log`。fixture 均保留 `.audit/v027-stage9-opencode-*`，无真实 DB。
- `node --check` 两个 stage9 OpenCode `.mjs`：均 exit 0。
- Claude `hypothesis-001`：3 pass/3 fail，目录保护假设失败，另有普通 rename sharing 和 cmd junction 参数的 fixture plumbing 失败；`observations-002` 6 pass/1 fail，普通 rename 仍 sharing conflict；修正实验适配后 `observations-003` 8/8。这些不是生产修复通过。
- Claude `dacl-004`、`dacl-005` 各 9 pass/1 fail；`acl-restore-diagnostic-006` 单项失败。观察已得到 classic 迁出，但 icacls /restore 返回 1300（ERROR_NOT_ALL_ASSIGNED），未到 Ex；没有申请提权。这是测试恢复方法失败，不能写成“产品 ACL 不可恢复”或工具 auto-review 拒绝。
- `dacl-007` 10/10 实际观察包括 classic 与 Ex；改为只移除全新 fixture 本轮新增的 deny，并精确核对存活合成对象原始 DACL 保存结果（SDDL）。已被 SDK 删除的 fixture 目录没有存活 ACL 需要恢复。没有验证任意既有 ACE、owner/SACL 或崩溃恢复，不是通用用户权限恢复方案。
- `final-008` 10/10，补强009创建前成功/后精确 STATUS_ACCESS_DENIED 和010两种 rename 必须成功。根复核再发现004/006/008等条件分支允许实验未发生仍通过；进一步要求实际 SDK 成功、原名及 children 消失、同名重建成功和005精确 EBUSY。`final-009` 10 pass/0 fail/0 ignore，1787 filtered，3.59s；这些日志保留而非覆盖。
- 首次 Clippy 为 `chunks_exact_to_as_chunks` 两项失败、exit101；改标准 slice as_chunks，不添加 allow；最终结果见上段。
- 首次 staged diff 检查 exit2：新抽取 `.mjs` 末尾多一个空行（未跟踪时普通 diff 未覆盖）。只删除该空行并更新文件摘要，重新语法检查、核 generator 原文本摘要不变、staged check 通过；原失败在工具 stdout，未冒称有独立磁盘日志。

Rust CI 分片只有 checkout/setup-node22 和已编译测试包，没有 node_modules；根静态复核发现最初运行时读 SDK 的新测试会失败。最终测试改为编译期 include_bytes 固定包并在运行时核哈希，producer 本来执行 npm ci，不改 CI、依赖或生产包。只增加 test executable 内容；当前宿主实际 Node24，Node22/hosted分片尚未运行，不能冒称已验收。

## 来源核对、独立审查与提交边界

`.audit/v027-stage9-source.json` 绑定两份 Rust 测试源、两份 OpenCode 实验源及19份原始日志的大小/SHA256。两 Rust 文件按排序 `path + NUL + raw bytes + NUL` 摘要为 `2ea0632eadf7d1fa340359f23ceecef2fc36434ffd8062b7dc0b1e11fbd5953e`，根实算与 worker 报告相同。root 核对 production/CI/policy/依赖 diff 为空；SDK 与类型文件原字节摘要重新核对相同。

独立只读审查核对固定 OpenCode 函数体/许可证逐字相同、Claude 最终硬断言及日志、fixture 恢复和原件来源边界；根另查软断言和 CI runtime 依赖并要求修正。没有以独审口头意见替代命令证据。实际应用 superpowers 计划执行/TDD、系统调试、请求审查、完成前验证，以及 pstack foundational-thinking 的并发 actor/数据结构检查；仅使用已存在技能，不安装。

文档一致性检查6份文档、42个本地链接通过；旧 todo 和两份 superseded 归档补丁的3个保护摘要一致。`.audit/v027-stage9-doc-check.json` 保留结果。阶段9只本地提交该新实验、测试注册与审阅材料，不加入旧 untracked fixture、补丁、todo 或其他树，不 push/main/PR/tag/Release/安装。

没有修改生产 Rust/TS/UI；不需要将旧全量重新跑一遍制造证据。Codex 7、前端 408 和构建/CSP 属上一阶段原源，当前不冒称新 Rust 测试树全量通过。未跑真实 Claude/OpenCode/Codex 删除、WebView、Unix、hosted CI、新源完整四 harness/16 片或真实签名 0.27 往返。此轮不能宣布整版 P0 完成或发布。
