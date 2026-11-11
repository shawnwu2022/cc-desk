# 0.27 永久删除：当前实现与明确缺口

2026-11-06 用户明确要求永久删除来清理累计 session，并要求二次确认和风险提示。此要求替代此前优先隐藏/软归档的方向；隐藏、归档、备份都不作为永久删除交付。开发仅使用本任务合成数据，不删除真实用户数据，不连接用户正在使用的服务或申请凭据。

产品实现提交 `6de019a2ee22528d1c391e76c449c968c9846140`；最终命令、408项前端/7项Rust及独审与失败证据见 [阶段审计](../.audit/v027-stage8-permanent-delete.md)。

## 本轮实际完成

- 已整合 0.26 正式 main，未改 main、发布资源或版本号；整合证据见 [开发记录](development-0.27.md) 和 `.audit/v027-stage6-main-integration.md`。
- 停止扩展旧归档方案，完整保留可重放补丁，再仅撤出本任务的未提交改动；未 reset、stash、clean 或覆盖旧 todo。两个补丁见下表，旧归档测试结果不计作永久删除验收。
- 已实现 Windows 下 Codex 的窄永久删除链路：本地只读预览、独立二次确认和双勾选、后端一次性票据、同授权 profile 的受控原生 `thread/delete`、结果协调与只读 inspect。预览和取消不启动 CLI；执行不复用旧隐藏/归档接口。OpenCode/Claude 仍按各自具体缺口禁用，不猜路径或降级到旧删除 API。
- `NativeSessionDeletePanel.vue` 显示准确根 ID、profile/revision、来源标识和项目；确认范围是该 profile 原生根及执行时原生子树，完整后代数量不可得且明确为未枚举。确认前已观察到来源/配置/记录变化必须重新预览，不宣称冻结未来成员。
- API 固定 document bridge；Pinia 账本跨弹窗关闭保留原操作。重复点击只发一次并只刷新一次来源历史；发送前失权不发 mutation；请求发出后的异常保守标 unknown，只读 inspect 不重放。不同目标不能借用原确认或清掉别的结果。
- 直接调用固定 Claude SDK 的真实删除函数，对任务自建合成文件做隔离实验，实际证明硬链接适配不等于删除原数据，且 SDK 不拒绝已开放的 Node 写句柄。没有启动 Claude CLI、服务或模型请求。

**实际交付范围是上述 Windows/Codex 窄场景的产品实现、合成回归及真实 CLI 隔离后端验收。** 阶段10已用现有0.167.0原生 initialize/read/delete 真正删除任务合成原件，取消/缺ack、换源/修订、重复和只读结果链有证据；首次unknown原记录未重放，详情见 [阶段10审计](../.audit/v027-stage10-codex-live.md)。这不是安装版 WebView、真实可见双确认或 Unix 验收；没有完整枚举/冻结后代清单或独立逐后代结果。原生 ACK 与本地根是否仍存在分别展示，不能把根消失说成全部关联数据都已清除。该切片不等于原先所有 CLI、所有平台的完整 P0。

| 已保留片段 | SHA256 | 状态 |
| --- | --- | --- |
| `.audit/v027-stage7-superseded-desk-archive.patch` | `59ebd85c4f4df6fcf8359dfecbc8205a5c84c1d845044e40852d83ee9eb5a910` | 8 个前端/测试文件，含未完成标签测试，撤出后不在产品树 |
| `.audit/v027-stage7-superseded-native-archive.patch` | `d692097086409d01aa798caad2d37847d9be485ea398b87a2d7c0756320f754b` | 13 个 Rust 文件；补丁源 227 pass、Clippy/fmt 成功，撤出后不在产品树 |

## 三个提供方的真实接口与差异

### Codex：原生删除与外部 writer 保护，按根和动态子树确认

固定 `rust-v0.167.0` / `086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a`：

- [`ThreadDeleteParams`](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server-protocol/src/protocol/v2/thread.rs) 只有 `threadId`，成功 response 为空对象。没有 expected 子树或版本参数。
- [`thread_delete`](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server/src/request_processors/thread_delete.rs) 执行时重新计算 spawn 子树；检查 attached threads，准备删除，再以后代先、根最后的顺序调用 store。
- [`delete_threads`](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/thread-store/src/local/delete_thread.rs#L114) 在任何 cleanup 前获取候选生命周期锁和跨进程 writer 锁，再检查 attached/external-history 引用。因此不能声称它没有外部活动保护。随后做 host 关联数据 cleanup、逐项 rollout/历史删除，全部 rollout 完成后才删 state rows；I/O 错误可能留下部分结果。
- [`acquire_writer_locks`](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/thread-store/src/local/mod.rs) 逐项 acquire，任一失败退出；[`writer_lock`](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/rollout/src/writer_lock.rs#L44) 使用实际文件锁。短命 helper 不发送 resume/turn，可避免它自己加载活动线程；阶段10已做持合成原生锁的真实CLI未取得成功回执/原件保留观察，未启动真实模型writer，不等同完整busy演练。
- 协议源码确实提供实验性的 `parentThreadId`、`ancestorThreadId`、`attachedToThreadId`；生成 JSON schema 缺字段不证明接口不存在。可按实际能力校验做持久关系观察，但不能把两分区、多页查询当原子快照。预览后、请求重新求子树前发生的外部变化，仍可能扩大范围；writer 锁保证执行一致性，不是事前确认集合的 CAS。

当前不再把缺少 expected IDs 单独当作永久阻塞。窄契约按准确根及原生子树确认；普通 resume/unarchive 先获取同一 thread writer 锁，spawn 先持久化关系边再创建 child 存储，可为活动/引用保护提供实际依据。但这不是未来 spawn 图的租约：外部 unarchive 若在最后复核后先完成并释放锁，删除可能处理其后的原生子树；不能承诺只处理预览时固定成员或根始终保持 archived。界面对此明确二次确认，已观察到变化则拒旧票据。没有自行占用 native writer 锁再与 CLI 抢同锁。

同一精确 configured-profile 自己选择的原生 store 是本次授权语义，包含 CLI 自行配置的 graph DB；不要求 `sqlite_home` 物理等于 `CODEX_HOME`，也不写命令行覆盖它。[管理 requirements](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/core/src/config/requirements.rs#L13) 可覆盖普通配置，故不能声称 `-c` 或最新 config/read 已证明启动数据库身份。受控 helper 的 [initialize.codexHome](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server-protocol/src/protocol/v1.rs#L76) 来自实际启动 Config；当前将其物理 Root key 与 held authority 核对，再核对 thread/read 的实际 rollout 路径、首行身份和 cwd。缺失、不匹配或不能实核时不发 delete；没有版本白名单。

Windows 还持有非 reparse 的 sessions/archived_sessions 容器句柄至整棵 own Job 进程树实际退出，拒绝目录替换；直接 launcher reap 不等于 native 后代结束。新 helper suspended 创建后加入 Job 再恢复，实际释放必须 root 已 reap 且 Job.ActiveProcesses=0，未知继续保留 owner/freeze。当前还要求 Desk 全局无其他活动/准备/未知 CLI，执行期间冻结新 admission，不自行停止其他会话。Unix 缺等价已实现保护则 unsupported，无 legacy fallback。

原生 [host cleanup](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/core/src/thread_manager.rs#L565) 正常按候选 ID 清消息板；但 [board 数据库损坏恢复](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/message-board/src/local/lifecycle.rs#L42) 会备份并重建共享库，可能影响其他会话 board 可用性。启动还可运行 migration/compression；“没有 resume/turn”不表示没有后台维护写。二次确认明确关联维护风险，正常 ACK 后先 EOF 收尾，原时限后只终止本次 Job，未 settled 不释放保护。错误不等于无改动；没有保证对同用户恶意任意文件改名的强原子隔离。

早期阶段3安装版本探测为0.166.0；阶段10现有二进制实际探测为 **0.167.0**，SHA256 `edfdd48f8b458af0906e507b4cb05217b326c3d6e515e9f024e05e3b8d650f70`，本任务未安装或升级它。实际匹配上述086ccc来源，另核对关键配置/认证/初始化差异，未用旧0.166分析冒充当前行为。

阶段10修复了两个真实边界：初始化显式 `explicitGatewayOauth:true` 以避免元数据助手触发自动认证；按 [官方通知定义](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server-protocol/src/protocol/common.rs#L2081) 精确忽略无id、对象params的remoteControl/status/changed。其它工作通知和server request仍拒绝。初始化/读取或明确未开始delete写入的准备失败，收口后分别显示blocked和固定 `DELETE_INITIALIZE_FAILED` / `DELETE_READ_FAILED` / `DELETE_PREPARE_FAILED`；这只证明本helper未发送删除，不代表来源没有其他变化。实际删除写入开始后的错误/缺ACK仍unknown，只能核查，不自动重放。

### OpenCode：真实递归删除会先中断，不能套成单记录无副作用操作

固定 `9bbb8ce18c80f099e0e448775f59d965ead40a21` 与 `47935cae0d96703dad8b096ab09ee7a07655c7ad`：

- [公开 DELETE](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/protocol/src/groups/session.ts#L245) 仅收 `sessionID`；[handler](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/handlers/session.ts#L186) 直接调用 remove，没有 noncascade、expected children、维护租约或条件版本。
- 两个快照的 [`remove`](https://github.com/anomalyco/opencode/blob/47935cae0d96703dad8b096ab09ee7a07655c7ad/packages/core/src/session.ts#L391) 顺序一致：interrupt、awaitIdle、取消 jobs、关闭 transport，再动态读 children、顺序递归，最后清环境并发布 Deleted/remove bus。活动视图限该进程；不能自行停止外部会话。
- [`store.list`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/store.ts#L145) 只传 parentID 时不排除已归档 children。[projector](https://github.com/anomalyco/opencode/blob/47935cae0d96703dad8b096ab09ee7a07655c7ad/packages/core/src/session/projector.ts#L963) 释放 session blob 并删除 session_v2；[schema](https://github.com/anomalyco/opencode/blob/47935cae0d96703dad8b096ab09ee7a07655c7ad/packages/core/src/session/sql.ts#L28) 的 parent_id 无父会话 FK。推论：递归过程中新增对象可能被纳入，也可能遗漏为孤儿，不能只凭根消失宣布整棵树成功。

最小解阻条件是已有受控服务/DB 同源授权、可绑定确认范围及不擅自中断外部工作的执行机制。现有 private serve 启动可能 migration/resume 工作，不能为删除预览假装只读启动；相关来源证据见 [同源分析](opencode-service-source-proof-0.27.md)。不采用直接 SQL DELETE、项目 purge 或删库代替单会话接口。

### Claude：官方 SDK 确有删除，现有 rename namespace 不可复用

已安装且 lockfile 固定 `@anthropic-ai/claude-agent-sdk@0.3.301`，官方 tag [`v0.3.301`](https://github.com/anthropics/claude-agent-sdk-typescript/tree/b0fc766997199a49f90c37f5ec056936c80c9491)，[changelog](https://github.com/anthropics/claude-agent-sdk-typescript/blob/b0fc766997199a49f90c37f5ec056936c80c9491/CHANGELOG.md) 在 0.2.113 记录新增 `deleteSession`。这是本地 SDK 删除，不是云 Managed Agents session API，也不是项目 purge。

本地类型 `sdk.d.ts:688/6637` 和实际 `sdk.mjs:166` 的 `nYe → q4/XZe` 证明：

- UUID 校验后，无 sessionStore 时查找 transcript，先 rm 主 JSONL，再 recursive rm 同名子会话目录；第二步失败可能在主记录已删后报错。没有活动、writer 锁或 expected manifest 参数。
- `dir` 是项目路径，实际搜索还可能纳入关联 git worktree；省略时搜索全部 projects。不能把 display cwd 或 UUID 当唯一真实源证明。
- sessionStore 缺 delete 时可成功 no-op，不能据此宣称磁盘已删除。
- 既有 `rename_fs.rs::private_transcript` 在私有 namespace 建真实 transcript 的 hardlink；rename 的追加会落到同 inode，但 delete 只 unlink 私有名字，原始名字和未映射的真实子目录仍在。改 import/call 并不能交付删除。

本地包摘要：`sdk.mjs` SHA256 `6b963b3eb871949a14392bb35ff970a55e91e2edbe218c61cd9eed60e46cbe45`；`sdk.d.ts` SHA256 `26f9a0c91ad64e2020f0d3734932302e3cc09685bddf2b2f9ef60839ce278773`。SDK 的闭源部分与包实现不混称公开完整 CLI 源码。

后续真实 Windows/SDK 实验已证明，private project junction 和受限 Node FS 权限可以映射并删除合成原件，因此来源适配本身有可行方向；尚未接入产品。当前最小缺口是保护主记录和动态子目录在确认至删除期间不受活动/新子会话写入，并准确核查原件而非仅看名字消失。仅列出进程、确认时看似 idle 或用户风险确认本身不建立该保证；不能靠硬链接、全 projects 搜索或猜路径绕过。

## 合成隔离实验与范围

`node .audit/v027-stage8-sdk-delete-fixture.mjs` 固定 SDK 哈希，先在任务 `.audit` 下 mkdtemp，核对物理父目录，再造只有合成元数据的 JSONL/子目录。子进程环境重新白名单构建，HOME/USERPROFILE/APPDATA/XDG/CLAUDE_CONFIG_DIR/TEMP 全指任务新目录，不继承认证变量；只 import SDK 和调用 deleteSession，没有 query/startup/CLI/server。

Windows 实际三个观察均通过：

1. 原生 SDK 真删除合成主记录和子记录目录，任务自建项目 marker 保留；不外推为所有外部附件/项目内容都保留或清除。
2. 私有 hardlink 被删后，任务自建“原始”名字和子目录仍在。
3. 合成主记录的 Node 写句柄仍开放时，SDK 仍删掉路径，已有句柄随后仍能写。这是缺少 OS writer 拒绝的反例，不是实际 Claude CLI 活动验收。

fixture `v027-stage8-sdk-delete-AMYBUm` 和日志保留原状；脚本额外校验 `.audit` 物理路径没有跳出 repository，最终脚本检查另行记录。未做真实 CLI、WebView、Unix 或安装验收。

## 实现接口、结果与剩余

- `cli_preview_session_delete` 只读绑定 caller/document、profile/revision、held root、sessionKey/UUID 与当前观察；120 秒票据不等于冻结图。scope 明确 native-subtree，knownDescendantCount=null，不编造列表。
- `cli_confirm_session_delete` 必须原 previewId、operationId 和两个 true ack；确认前 fresh 复核，变化消费旧票据。同 operationId/参数只返回原结果，不扩大范围；未知物理 Root+UUID 还跨 profile/caller/UUID 大小写保护，不可换配置盲重试。
- 尚未 dispatch 时，来源、容器、观察或票据失效消费原预览并保存 `blocked / DELETE_PREVIEW_CHANGED` 回执，只读 inspect 可返回该结果。已经 claim 的操作不会被迟到失效改写为 blocked；必须保留已有回执或 unknown，不能重新发出删除。
- `cli_inspect_session_delete` 只读查原操作，不调用删除。区分 acknowledged、partial、unknown、blocked、unsupported；native ACK 单独保存，owner settled 前仍不报完成。没有 ACK、根缺失仍 unknown；owner 已 settled 且根仍在可报告 partial，但只有只读检查后、用户主动新预览及再次双确认才可重试，不自动重放。
- inspect 首次协调为 acknowledged 后，界面刷新操作最初保存的准确历史来源和项目；切换复核目标不改变刷新对象。历史读取失败保留最后完整缓存并显示错误，仍刷新对应目录，不把刷新失败改写为删除失败。
- Desk 标签/覆盖元数据可能保留；没有宣称清掉全局日志、外部附件、项目文件或远端副本。原生不返回失败时逐项清单，后代与关联数据仍标 unverified。

OpenCode/Claude 的真实源/活动约束继续分别解阻，不将其误称无永久删除 API。当前不是等待产品意图确认或工具审批；用户已授权产品实现与隔离测试。真实 CLI / WebView、三平台签名候选及隔离升级恢复仍是尚未跑的交付证据，不新发布、不改用户安装。

## 后续提供方适配：已完成的有界实验

从 `3bdc9674` 继续，保留 Codex 和共同双确认/账本，增加 `claude_delete_guards.rs` 合成 OS/固定 SDK 实验及 OpenCode 官方 remove 原样函数体的合成 SQLite 观察。完整固定源码、MIT 许可、精确命令、原失败和最小条件见 [阶段9审计](../.audit/v027-stage9-provider-delete.md)。这轮不把观察用例通过说成两家产品能力已交付。

Claude 同文件 READ/share READ|DELETE guard 确实拒绝旧/新 writer，但目录 sharing 不能冻结新 child；SDK unlink 后同名可重建。仅在新 fixture 上做的 deny WD/AD 能拒绝路径及旧 READ 目录句柄相对创建，SDK仍能真删；预先取得的 DELETE-only 句柄仍能经 classic/Ex rename 将原件迁出。受限 private junction 的来源映射可行，真实活动保护仍缺。若未来采用权限维护模式，必须另行说明并实现范围、旧能力、精确恢复及崩溃恢复；本轮没有产品 ACL 变更，不把它变成未说明的删除副作用。迁出反例也不意味着永远无法用 held-object 身份/删除状态核查结果。

OpenCode 持久 `time_suspended` 是 write-ahead intent，claim 的零行 no-op 不建立排他权；本进程 active/jobs/transport 不涵盖其他实例。5项算法观察包括已提交外部 claim 仍删根、晚到 child 留存、已归档 child 纳入、不相关根保留、元数据相同的不同DB以及晚启动 execution 被 interrupt。替身 Set/服务不是另一真实 CLI 实例。官方 CLI delete 仍走 ServerConnection，默认/standalone 可能启动服务并触发调度；Desk 现有 private server 在操作后销毁，没有可直接借用的持久 HTTP owner。实际可行下一步是在正常授权生命周期保留该服务来源，并具备覆盖启动/恢复/调度的拒忙/维护准入；不是单纯再加一次 idle 查询或要求理想 expected-ID 集合。

结论分别为：Codex Windows 窄能力保持；Claude 来源映射可行、活动/namespace 保护未交付；OpenCode 原生删除真实存在、现有服务来源及执行期间活动准入未接通。两家执行仍关闭，隐藏/软归档未作为替代。无新增服务、真实用户数据操作、push 或发布。

Claude/OpenCode 后续的具体来源、活动准入、正常生命周期、崩溃/部分失败、所需协议或用户动作及验证入口，单列在 [维护设计](provider-delete-maintenance-design-0.27.md)。这是待实现设计，不是两家永久删除交付，也不授权生产 ACL 或新服务启动。

阶段10最终收口：受影响Rust域18 pass / 5按规定ignored，另显式A/101/B002/102均通过；前端3文件24项、类型检查、最终严格all-target Clippy/fmt和独审通过。仅B002是单根真实永久删除正向结果；102是持续占用合成原生锁的未取得成功回执/原件保留观察，无解锁重放。真实document IPC、可见安装版WebView、Unix、引用/多后代部分失败矩阵仍未验收，旧完整套件不冒称当前源全量结果。完整命令、源码摘要和保留失败见阶段10审计及索引。


## 2026-11-11：批量删除交互

归档抽屉的“永久删除能力”提供“批量删除”：勾选多项、按名称/CLI/项目路径筛选、全选当前可删除项、清空选择；新出现的行不会自动加入。选择可跨当前抽屉中的项目及配置，同一物理来源根＋原生 UUID 只发送一次。活动、准备中、不支持的 CLI、来源未知、重复根及未解决操作不能作为新删除目标。正常界面不再通过单选下拉逐个执行删除；下拉仅用于说明各 CLI 能力边界。

批次统一展示根清单、准确数量、项目、profile/revision、来源标识与原生 ID，然后经过一次独立二次确认和两个风险勾选。确认后自动顺序调用既有原生删除接口，不要求用户逐个再点击。批次不是原子事务，不自动回滚已完成项；首个 blocked/partial/unknown、来源失效或取消都停止尚未发送项。父根删除可能包含另一所选根；后一根已缺失时保留阻止结果，不能伪造第二份 ACK。

120 秒票据时限保持不变，不自动延期、续签或重放。很大的选择集或等待较久可能在预览/执行队列中超时；界面明确提示将剩余项重新选择并预览确认，可缩小每批数量。不能把“全选”理解为无期限授权或承诺全部一次完成。

关闭、切换来源或卸载只停止未发送请求；已发送请求继续由原操作账本收口。排队项一旦被观察到失权，即使随后恢复也不沿用旧确认。每批次保存独立操作实例，重新预览同一目标不改写旧 partial/unknown/根是否存在/原因；旧批次不会借用新操作客户端检查。ACK 后按最初来源合并刷新，每个来源一次；失败保留历史读取诊断，不把刷新失败改成删除失败。

### 同源连续执行的后端观察

旧票据对整个 history catalog 求摘要，首个根被删除会让同目录其它根的票据失效。本轮将 Codex 删除观察改为精确目标的显示记录、原 config.toml 内容以及该 UUID 全部 rollout 的路径、物理文件身份和完整原始内容 SHA-256。通过实际 session_meta UUID 归集，不从文件名猜身份，不受自动会话、项目过滤或显示折叠影响；混合活动/归档状态、跨项目同 ID、读取失败或预算不足仍拒绝。其它根被删除不会刷新或重签本目标票据。

全内容哈希分块读取并遵守既有总字节/条目/时间预算，不将原始转录返回前端。确认时及原生 thread/read 后、真正发出 thread/delete 前分别复核该目标原件和配置。该观察仍不是原子文件快照或未来子树租约，检查与原生删除间仍存在外部变化窗口；原生 writer/attached/external-reference 检查、Desk admission、held root/容器和动态子树确认都保留。助手每次写协议帧前均复核同一 30 秒截止时刻；慢内容复核耗尽时限时，在标记删除已发送之前停止，不因延迟再发 delete。

### 验证范围

新增前端用例覆盖多选/筛选/跨项目、统一双确认、顺序请求、重复点击、迟到预览、关闭/卸载、短暂失权、未知/partial/过期停止与旧回执保存。Windows Chromium 合成桥测试覆盖中英文、明暗主题及 100%/150% 缩放；截图是未批准的新交互证据，不改既有批准基线。

真实合成文件回归先复现“删除 A 使 B 票据变化”，再验证同源连续根、同长度同 mtime 改写、同字节原件替换、额外同 ID rollout、显示外/自动 rollout、混合归档和配置变化。生产服务的合成子进程测试验证同源两根先预览再连续执行。它们不等于真实 Codex CLI/安装版 WebView2 或真实父子/外部 writer 的批量验收；用户真实数据未被删除，Claude/OpenCode 与 Unix 能力范围未扩大。

本轮已验证：`npm run typecheck`、`npm run build`、`npm run test:fast`（196 文件 / 3138 项）、批量组件专项（17 项）、`cargo test --locked --lib NativeDelete_`（16 项）、隔离 `scope-core` 全部 265 项、`cargo fmt -- --check` 和 `cargo clippy --locked --all-targets -- -D warnings`。Chromium 指定批量及既有可用性场景共 6 项通过；新截图位于本地 `test-results/visual/`，未替换批准基线。未运行完整 Rust library、真实 Codex CLI / WebView2、其他平台或发布验收。先由真实合成文件复现旧整目录观察导致第二项票据失效，修复后对应回归通过。


## 2026-11-11：逐行删除入口调整

用户进一步要求每条存档会话一个独立删除图标，并完整移除列表下方永久删除模块。归档抽屉改为行末图标入口，复用单条删除预览、独立二次确认与两个风险勾选；批量组件不再挂载到产品归档入口，原后端与合成批量回归保留。关闭、目标移除/恢复和来源变化继续撤销未发送预览；已发出的操作由原账本收口，行消失后当前结果弹窗仍保留。回执详情在删除弹窗内展示。Claude/OpenCode、Desk-only 归档和失效来源的图标禁用，并说明原能力限制；没有扩大删除范围或删除用户真实数据。

仅当已有删除执行/结果记录时，抽屉标题旁显示“查看删除结果”的历史图标，独立于当前行和删除资格打开只读回执与核查。它不提供新的删除或重试入口，也不恢复列表下方模块。

本次入口调整验证：新增逐行入口/回执恢复用例先失败后通过；最终 `npm run test:fast` 为 196 文件 / 3144 项通过，`npm run typecheck` 与 `npm run build` 通过。六项 Windows Chromium 合成场景通过，覆盖中英文、浅暗主题、100%/150% 缩放、键盘打开/取消回焦、单条双确认和只读结果重开；新截图位于 `test-results/visual-row-delete-final/`，未替换批准像素基线。首轮两项浏览器场景在与全量快速回归并行时导航超时，保留 `test-results/visual/` 原失败证据；未放宽超时或断言，随后单独运行全部六项通过。审查发现并修复未知回执在行消失/来源失效后不可达的问题，复审无剩余代码缺陷。本轮无 Rust 改动，未执行完整进程/压力/Rust、真实 CLI、安装版 WebView2 或发布验收。构建仍有既有大 chunk 提示及本机 npm 配置告警。


## 2026-11-11：修复有界历史元数据误拦截

历史列表的 `truncated` 同时表示标题截断和正常有界读取的 `metadataIncomplete`，不表示来源身份或原生归档证据失效。删除入口不再以此展示标志拒绝已知原生归档的 Codex 记录；只允许进入既有只读预览，不会自动删除或绕过二次确认。来源、profile revision、项目、根和精确历史键仍须匹配，读取失败、来源失效、活动/准备中和仅 Desk 归档仍阻止预览，并按实际原因给出中英文提示。

后端代码和权限边界未更改。完整目标观察、原生归档核验、原生 writer/attached/external-reference 检查、Desk 全局 admission、一次性票据与执行前复核继续生效。测试只使用合成历史和合成 document bridge，不操作用户真实会话。

本次验证：新增 13 项组件回归先失败后通过；删除相关 4 文件 / 65 项通过，完整 `npm test` 为 211 文件 / 3287 项通过、2 项跳过，`npm run typecheck` 与 `npm run build` 通过。四项 Windows Chromium 合成场景通过，覆盖有界元数据下的可用入口、仅 Desk 归档的禁用提示、中英文、明暗主题、100%/150% DPR、取消与双确认；截图保留于 `test-results/visual-delete-eligibility/`，未替换批准基线。只读审查未发现剩余问题。无 Rust 改动，未重跑真实 CLI、安装版 WebView2 或 Rust 验收；未删除真实会话、修改用户安装、push 或发布。构建仍有既有大 chunk 提示，本机 npm 配置和浏览器颜色环境有告警。
