# 原生清理的历史阻塞与当前窄场景

**最新范围见 [永久删除实现与证据](permanent-delete-0.27.md)。** 用户已明确要求真正永久删除及二次确认，后来允许按准确根及原生子树语义确认；不再以缺少任意 ID 集合参数单独阻塞 Codex。当前开发实现 Windows 下同授权 profile、已原生归档根的窄删除，并明确动态成员/关联维护风险。下面保留的是早期更强“冻结成员集合”设计的分析，不是当前所有 CLI 一律关闭的结论。OpenCode、Claude 的具体来源/活动约束仍分别处理，不用隐藏或软归档代替。

以下是早期独立只读审查的历史记录：当时没有执行真实归档、删除、purge、worktree rm 或目录删除，也没有读取用户认证或会话正文；当时抽屉只有目标和拒绝原因，没有执行入口。这些事实不表示后来实现的Codex窄删除仍全局关闭，当前行为以页首新文档为准。

## 固定源码快照

| 能力 | 官方来源 | 已证明的语义 |
| --- | --- | --- |
| Codex archive / delete 参数 | `rust-v0.166.0` → [`6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`](https://github.com/openai/codex/commit/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856)，[参数定义](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L716) | 只有 thread ID，没有确认集合、revision 或预约令牌。 |
| Codex 执行范围 | [thread_delete](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_delete.rs#L28) | 执行时重新计算 spawn 子树，之后准备及删除 IDs；没有比较 Desk 预览集合。 |
| Codex 原生锁 | [WriterLockCoordinator](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/rollout/src/writer_lock.rs#L44)、[delete_threads](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/thread-store/src/local/delete_thread.rs#L123) | 确有跨进程 writer 锁和生命周期/引用复核；不能错误地说“没有锁”。这些锁维护存储一致性，没有绑定事前确认的集合。 |
| Codex 已加载线程 | [prepare_thread_for_removal](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_processor.rs#L1165) | 对服务中已加载线程可主动 shutdown，不是活动时必定无副作用拒绝。 |
| OpenCode 删除协议 | `v2.2.5` → [`9bbb8ce18c80f099e0e448775f59d965ead40a21`](https://github.com/anomalyco/opencode/commit/9bbb8ce18c80f099e0e448775f59d965ead40a21)，[协议](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/protocol/src/groups/session.ts#L245)、[handler](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/handlers/session.ts#L186) | 仅 sessionID，没有确认范围/状态的条件参数，handler 直接调用 remove。 |
| OpenCode 级联与活动 | [Session.remove](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session.ts#L391)、[SessionExecution](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/execution.ts#L14) | 先中断、等待 idle、取消任务、关闭 transport，再动态枚举后代并递归删除；活动观察和中断仅限当前进程。 |
| Claude 单会话删除 | 已安装官方 Agent SDK `0.3.301` 的 `deleteSession`，固定包及官方 tag 见 [最新核对](permanent-delete-0.27.md) | 确有删除 transcript/子会话目录的接口；现有 rename 私有 hardlink namespace 不适用于删除原始历史。SDK 缺活动/预览集合参数，不能直接复用。项目 purge 和后台/worktree rm 不是替代。 |

## 具体反例和未证明项

Desk 最后一次读取的确认集合为 S；外部 CLI 随后创建一个子会话；原生请求重新计算关系后包含新对象。内部锁即使正确，原生参数里没有 S，无法比较其是否改变。多次复查、Desk mutex、进程扫描或“目标已归档”都不能把观察转成外部租约。

OpenCode 最后一次显示 idle 后可以开始活动，然后 remove 主动 interrupt。另一进程的活动还可能不在当前进程的 active 视图内。没有证据证明当前删除接口会在原子保护区间拒绝这些状态；不能把“先停止再删”伪装成“排除活动对象”。

Desk 现有 ScopeRegistry 可验证文档、profile/revision、held Root 和准确身份；AdmissionGate 只约束 Desk 自身。可见历史刻意隐藏 Codex 自动子会话、OpenCode `parent_id IS NOT NULL` 记录，不能充当完整级联目录。当前尚未扩展该完整目录；不能因此在界面显示猜测的后代数量。

只读读取与执行安全分别核对。公开的 Codex ancestor 关系查询和 OpenCode session list 确实存在，单次查询也有可证明的完整过滤集合边界；但 Codex 该集合不含原生级联另合并的 live 子树，OpenCode 当前缺 Desk held DB 与无启动副作用的同源只读服务绑定。详见 [只读预览窄范围复核](native-cleanup-readonly-preview-0.27.md)。不因 mutation 缺口一概否定读取，也不把持久关系或项目根列表当成完整原生子树。

当前未找到公开可调用的条件 mutation 或覆盖预览至执行的原生租约。这里没有断言未来版本永远不支持，也没有按版本号封锁 CLI 的其他功能。自行占用原生 writer 文件锁再调用原生 delete 会与其内部锁冲突，不是经过证明的锁交接方案；不采用注入/修改原生库、直接 SQL DELETE、猜路径删除或终止外部用户进程来弥补。

## 已实现与剩余

已实现只读拒绝说明：选择归档对象显示 title/CLI/project/native ID/profile/revision/观察到的 root key；缺失字段明确未知。显示身份不是执行授权，明确尚未核验 held source 和完整后代集合。活动、恢复中或状态不确定的目标另行保护提示；原生按钮始终禁用，无 IPC mutation、确认副作用、部分成功或超时成功声明。关闭/移除目标清空选择。Desk 自身既有归档/恢复保持原语义。

完整清理仍未实现：完整关系枚举、后端持有的一次性范围预约、逐项执行及结果协调。仅有这些 Desk 结构仍无法解决上述 native 条件接口缺口，所以没有构建会制造安全错觉的执行框架。

早期设计要求原生比较 expected 完整成员集合；这项强保证没有被证明。当前经授权的窄契约改为确认准确根及其原生子树，明确执行时动态成员风险，保留原生 writer/reference 保护与本机 owned-run admission。确认前已观察到来源/配置/根记录变化必须重新预览，未知结果不重放。具体实现、没有承诺的保证与测试结果以最新文档为准，不能把此历史要求当成唯一可接受的上游协议。
