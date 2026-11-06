# 原生清理的能力阻塞与拒绝路径

此结论由独立只读安全审查交叉核对官方源码。没有执行真实归档、删除、purge、worktree rm 或目录删除，也没有读取用户认证或会话正文。当前归档抽屉只展示已观察目标及明确拒绝原因，原生归档和永久删除没有执行入口；这不是完整级联预览或清理功能交付。

## 固定源码快照

| 能力 | 官方来源 | 已证明的语义 |
| --- | --- | --- |
| Codex archive / delete 参数 | `rust-v0.166.0` → [`6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`](https://github.com/openai/codex/commit/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856)，[参数定义](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L716) | 只有 thread ID，没有确认集合、revision 或预约令牌。 |
| Codex 执行范围 | [thread_delete](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_delete.rs#L28) | 执行时重新计算 spawn 子树，之后准备及删除 IDs；没有比较 Desk 预览集合。 |
| Codex 原生锁 | [WriterLockCoordinator](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/rollout/src/writer_lock.rs#L44)、[delete_threads](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/thread-store/src/local/delete_thread.rs#L123) | 确有跨进程 writer 锁和生命周期/引用复核；不能错误地说“没有锁”。这些锁维护存储一致性，没有绑定事前确认的集合。 |
| Codex 已加载线程 | [prepare_thread_for_removal](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_processor.rs#L1093) | 对服务中已加载线程可主动 shutdown，不是活动时必定无副作用拒绝。 |
| OpenCode 删除协议 | `v2.2.5` → [`9bbb8ce18c80f099e0e448775f59d965ead40a21`](https://github.com/anomalyco/opencode/commit/9bbb8ce18c80f099e0e448775f59d965ead40a21)，[协议](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/protocol/src/groups/session.ts#L235)、[handler](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/handlers/session.ts#L181) | 仅 sessionID，没有确认范围/状态的条件参数，handler 直接调用 remove。 |
| OpenCode 级联与活动 | [Session.remove](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session.ts#L382)、[SessionExecution](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session/execution.ts#L14) | 先中断、等待 idle、取消任务、关闭 transport，再动态枚举后代并递归删除；活动观察和中断仅限当前进程。 |
| Claude 单会话删除 | [CLI reference](https://code.claude.com/docs/en/cli-reference)（2026-11-06 查阅） | 未找到已验证的安全历史单会话接口。项目 purge 和后台/worktree rm 不是替代。闭源部分不能声称已做源码证明。 |

## 具体反例和未证明项

Desk 最后一次读取的确认集合为 S；外部 CLI 随后创建一个子会话；原生请求重新计算关系后包含新对象。内部锁即使正确，原生参数里没有 S，无法比较其是否改变。多次复查、Desk mutex、进程扫描或“目标已归档”都不能把观察转成外部租约。

OpenCode 最后一次显示 idle 后可以开始活动，然后 remove 主动 interrupt。另一进程的活动还可能不在当前进程的 active 视图内。没有证据证明当前删除接口会在原子保护区间拒绝这些状态；不能把“先停止再删”伪装成“排除活动对象”。

Desk 现有 ScopeRegistry 可验证文档、profile/revision、held Root 和准确身份；AdmissionGate 只约束 Desk 自身。可见历史刻意隐藏 Codex 自动子会话、OpenCode `parent_id IS NOT NULL` 记录，不能充当完整级联目录。当前尚未扩展该完整目录；不能因此在界面显示猜测的后代数量。

当前未找到公开可调用的条件 mutation 或覆盖预览至执行的原生租约。这里没有断言未来版本永远不支持，也没有按版本号封锁 CLI 的其他功能。自行占用原生 writer 文件锁再调用原生 delete 会与其内部锁冲突，不是经过证明的锁交接方案；不采用注入/修改原生库、直接 SQL DELETE、猜路径删除或终止外部用户进程来弥补。

## 已实现与剩余

已实现只读拒绝说明：选择归档对象显示 title/CLI/project/native ID/profile/revision/观察到的 root key；缺失字段明确未知。显示身份不是执行授权，明确尚未核验 held source 和完整后代集合。活动、恢复中或状态不确定的目标另行保护提示；原生按钮始终禁用，无 IPC mutation、确认副作用、部分成功或超时成功声明。关闭/移除目标清空选择。Desk 自身既有归档/恢复保持原语义。

完整清理仍未实现：完整关系枚举、后端持有的一次性范围预约、逐项执行及结果协调。仅有这些 Desk 结构仍无法解决上述 native 条件接口缺口，所以没有构建会制造安全错觉的执行框架。

接通前至少需要证明：原生执行点比较 expected 完整集合；在同一保护区间拒绝活动、恢复和新 admission；保护持续到 mutation 完成。之后才能实现范围确认、一次提交、部分失败、timeout 后只读查询且不盲重试，并用隔离合成 fixture 验证。该阻塞不通过扩大用户确认范围或降低活动保护要求解决。
