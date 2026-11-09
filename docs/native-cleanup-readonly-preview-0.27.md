# 原生清理：只读预览能力的窄范围复核

2026-11-06 对当前实现 `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3` 做增量只读复核。未重做删除接口研究，未启动 CLI/server、读取真实会话、调用 archive/delete 或改变用户数据。独立审查和集成负责人分别读取下列固定官方源码。结论是公开关系读取确实存在；执行不安全本身不阻止只读观察，但当前链路还不能发布“完整原生级联预览”。

## Codex：持久关系可读，不能当作完整原生级联

固定 `rust-v0.166.0` / `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`：

- [`ThreadListParams`](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L1483) 公开 `parentThreadId`、`ancestorThreadId` 和 `attachedToThreadId`；ancestor 可列任意深度 spawn 后代、不含根。接口标为 experimental，必须做实际能力/形状校验，不能用版本字符串白名单。
- [关系请求默认过滤处理](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_processor.rs#L6017) 会在未传 sourceKinds 时取消默认交互来源/provider 过滤，不能主动加 project/cwd/search 等过滤后仍声称完整后代。
- [关系读取路径](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/thread-store/src/local/list_threads.rs#L244) 直接走 state DB。[单页查询](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/state/src/runtime/threads.rs#L533) 取 page_size+1，没有溢出时可证明这次过滤查询到尾。不能笼统说“没有 snapshot token，所以连单页都不完整”。
- 该结果仍只覆盖所选归档分区与持久数据，[关系 SQL](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/state/src/runtime/threads.rs#L1294) JOIN threads 并排除 attached；跨归档分区或多页查询没有共同快照。
- 原生级联使用 [ThreadManager 的集合](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/core/src/thread_manager.rs#L1146)：**根 + 持久 spawn graph + 当前 runtime 的 live 子树**。公开 ancestor 列表没有这个 live 并集；即使一页到尾也不能代替该集合。
- [`thread/loaded/list`](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_processor.rs#L3018) 和状态 enrichment 属于该服务 manager 的观察，不能把未出现解释为全部 CLI 实例不活动。

因此可以另做“某服务/归档分区的持久关系观察”，但必须显式标注范围，不能命名为完整清理级联。本轮没有为此新增容易误解的部分子列表。完整预览最小缺口是可绑定同一来源、覆盖原生持久图和 live 并集的完整读取契约；这与 mutation 的原子 expected 集合比较是两项独立要求。

## OpenCode：单次完整投影读取有可行边界

固定 `v2.2.5` / `9bbb8ce18c80f099e0e448775f59d965ead40a21`：

- [`GET /api/session`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/protocol/src/groups/session.ts#L112) 支持 limit、cursor 和关系过滤。底层 [`SessionStore.list`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/store.ts#L145) 只有显式传入才按 parentID、archived、search、directory、project 等过滤；省略 parentID 会包含子会话，省略 archived 包含两种状态。
- 可证明的窄例：**已授权的整个查询域、已绑定的同一数据库服务、首次无 cursor、无会漏后代的过滤、明确硬上限且返回数小于上限**。此调用是一次 SQL SELECT，可在这一次结果内按 parentID 求闭包，展示“该次查询所见的完整已投影关系”；达到上限、来源变动、关系缺项/循环或校验失败时拒绝完整性声明。
- [`Session.list`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session.ts#L403) 对任意非空结果生成 next cursor，即使已到尾，不能把有 next cursor 一概当作实际还有数据。反之，跨页或逐 parent 递归遍历不是单次快照，time_updated 排序也可在读取期间变化，不能仅去重后称完整。
- [`session.active`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/protocol/src/groups/session.ts#L220) 只覆盖当前 OpenCode 进程。预览可显示“该服务观察的活动”和“其他实例未知”；未知本身不禁止无副作用观察，也不能授权执行。

当前没有证明服务与 Desk held DB 的读取绑定：[`ServerInfo.paths`](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/server-info.ts#L5) 只有 tmp，不提供 DB 身份。PID、hostname、同一个 session ID 或相同文件名均不足以替代来源证明。现有私有 serve bootstrap 也不能视为纯读取，[数据库初始化](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/database/database.ts#L33) 会做 WAL/checkpoint 和 migration；本轮不为了预览启动它。

OpenCode 只读预览不必等待上游安全 delete：可以通过已有服务的已验证同源只读通道，或另经证明的 backend-held 同 DB/授权域只读快照能力解决。当前未证实这些条件，不把已有根会话 SQL 直接扩成全库数据展示，也不猜字段/路径或暴露未注册项目数据。本轮明确拒绝声称完整的预览，未新增不完整子会话列表。

## 当前 Desk 已有能力与真正缺口

当前已有 `SourceRef`、Root、profile revision、逐注册项目 admission；并非完全没有 OpenCode v2 来源适配：

- `native_projection/service.rs:125` 已探测准确 DB，失败不回退旧默认镜像。
- `launch_service.rs:465` 已区分 v2 `debug paths db` 和 v1 `db path`。
- `native_projection/selection.rs:65` 已验证同 held root、显式 DB 与 channel 文件名。
- `native_projection/opencode_db.rs:333` 已优先 `session_v2`；但 `:436/:469` 仍是 `parent_id IS NULL` 的项目历史列表，DTO 没有完整关系契约。
- Codex 元数据操作的短命 app-server、OpenCode `opencode_prepare.rs::with_server` 私有 serve，不是保留原运行服务的完整关系连接；不能继承其他服务内存状态的完整性。

现有 project scope 不自动授权读取整个数据库；子树完整读取必须同时证明所有对象在授权域内或在越界时整次拒绝。不能隐藏越界子项后仍显示完整。无完整同源契约之前不新增 mutation、服务发现扫描或真实用户数据读取。Claude 本轮没有新的安全关系接口证明，继续明确不支持。

## 接通预览时的独立验收要求

先证明上述读取契约，再以合成隔离 fixture 覆盖根/多层子项、归档、同 ID 不同来源、跨项目越界、上限/分页、关系变化、循环/缺项、profile revision、取消/迟到与活动观察的服务边界。结果明确标注观察时间/覆盖范围/可能变化/非执行授权，执行按钮继续禁用。取消必须无副作用。没有满足这些前提，本轮不编造 GREEN 测试或用演示列表冒充完整预览。

执行端仍须另行解决 [原子条件阻塞](native-cleanup-blocker-0.27.md)：预览完成不会解除它。此次只是能力分解和证据文档，没有代码变化或新测试运行。

后续 [现有服务同源证明切片](opencode-service-source-proof-0.27.md) 已进一步核对真实 instanceID/私有注册/认证、现有 Root/快照与短命 helper。服务身份确实存在，但尚无已接通的可信链把实际 HTTP 进程生命周期和实际 DB 对象绑定到 held 来源；未据此编造纯 Proof 模型或启动真实服务。读取完整性、服务同源和 mutation 原子条件继续分别陈述。
