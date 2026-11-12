# Claude 主记录可恢复移除

这是独立于原生永久删除的新功能。归档抽屉的 **Claude 恢复区** 提供明确选择、范围确认、只读刷新和原件恢复；原有永久删除按钮、原生子树契约与 Claude/OpenCode 的永久删除能力门保持不变。不能把本功能计作 OpenCode 删除或 Claude 原生永久删除完成。

## 适用范围

- Windows 本地 NTFS；来源须为已认证文档持有的 Claude 配置 profile scope、准确 revision、物理配置根及已注册项目。Run scope、其他提供方、未知来源或其他平台不执行。
- UI 仅选择已关闭、Desk 已归档、无 resume/preparation 中间状态的 Native CLI 行。后端重新核对原生 UUID、首个 cwd、唯一 session key 与实际转录身份；后续进入其他工作目录不改变首次归属，不用 Desk 归档标记冒充原生归档证明。
- 只移动 `projects/<project>/<uuid>.jsonl` **原件**。同名子任务目录、附件、索引及其他会话全部保留原地；不改原生索引、不写 SQL、不调用 SDK deleteSession、不清空恢复区，也不启动 CLI/helper。
- 主转录必须是单链接 regular file；重解析点、祖先替换、身份变化和已有主转录 writer/DELETE 句柄导致拒绝。所有 Desk 运行时须空闲；操作结束显式释放 metadata freeze，不遗留虚假活动 owner。
- 不声称未知外部 CLI 已停止。子任务仍可写入原地目录；主转录原路径也可能被外部重建。移除不会接着删除这些后继对象，恢复不会覆盖它们。

## 持久恢复与故障边界

同卷恢复区位于已授权配置根 `.ccdesk-recovery-v1/<operationId>/`，不在 `projects` 扫描树中。单个源最多保留 128 个操作目录，达到预算会拒绝新的移除；不自动淘汰旧记录。

1. 保留根、祖先和项目目录能力句柄。先只读固定目标文件身份，再独占重新打开同一对象，重新验证 UUID/cwd。
2. `intent.tmp` 以 create-new 写入有界日志并同步，再通过持有句柄无覆盖 rename 发布 `record.json`。只在完整日志发布后移动原始 transcript 为 `transcript.jsonl`。任何阶段都不覆盖已有文件。
3. 日志固定版本、源身份、相对路径、UUID/cwd 和原始文件 ID。日志 cwd 只与 backend 已准入的项目路径拼写比较，不为判定归属打开、规范化或探测日志提供的路径；原 Grant 保留已核实项目能力。状态由实际源/恢复位置及文件 ID 只读推导，不依赖前端内存或“上次返回成功”。正常返回前，当前授权仍须有效。
4. 日志写到一半不会出现在可读取的最终记录名下；原件尚未移动。日志完整但尚未移动时显示“原件仍在原处或已恢复”；移动后回执丢失时，重开恢复区可发现已保留的原件。缺少最终日志的孤立目录保留不删。
5. 恢复区采用独立容错名字枚举，每个子项仍通过 no-follow 能力打开，单个 reparse 不阻断健康记录；版本历史的完整库存枚举继续严格拒绝 reparse。单条损坏或不安全的记录以 `unavailableCount` 报告，不隐藏其他健康恢复记录；不尝试修补、覆盖或删除损坏文件。读取其他未授权项目的有效记录不会对当前 scope 发布。
6. 恢复再次独占打开保留的原件，核对文件 ID、内容身份和项目授权，并用内核 `ReplaceIfExists=false` 无覆盖 rename。最终时刻竞争重建也不覆盖，双方都保留。只有执行前已观察到原路径占用才报告确定冲突；实际 rename 发出后的失败统一保守报告 unknown，随后仅只读定位。

普通错误后只读检查，不自动回滚或重复移动。未知结果意图按 backend instance、物理根、原生 UUID 和 operationId 在 Pinia 中保留，跨组件关闭/重开仍禁止重新发出同一未决意图；只有同 operation 的 `stored`/`source` 观察能解除。无日志的未知结果不能凭一次空列表认定未执行；提示重启后重新核对。新后端实例不继承旧执行身份，恢复日志仍可被新授权来源发现。明确的前置拒绝允许用户处理原因后重新选择、重新确认。

回收区不是不可变快照，也不防止其他拥有同等系统权限的软件自行修改数据。这里验证的是正常进程中断、句柄/路径竞争及明确故障点，不宣称已证明任意硬件断电、损坏磁盘或恶意同权限进程下的数据完整性。源根、项目目录或已记录 cwd 不再可验证时停止自动恢复，保留文件供检查。

## 接口与显示

`cli_recycle_session` 为独立 authenticated IPC，严格请求包含 source、action、operationId、sessionKey 和 acknowledgeRecoverable；动作仅 list/remove/restore。返回只含原生 ID、授权项目路径、操作 ID、固定状态、原路径占用标记、固定 `claude-root-transcript` 范围和不可读记录数量，不返回原生异常、任意路径或日志内容。

- list 不修改任何来源或恢复数据。
- remove 明确消费一次选择/确认；未知或协议错误不会重发。
- restore 只处理该日志指向的确切原件，原路径有后继记录则保留双方并要求用户处理冲突。
- `stored` 表示原件身份在恢复区，`source` 表示原件仍在原处或已恢复，`unknown` 不等于成功，也不触发补删。
- UI 刷新失败保留结果/错误，不能把显示行消失当作执行证明。全局视图按实际授权 scope 查询并按物理根+operationId 去重；恢复重新取得当前来源。

## 验证入口和范围

`NativeRecycle_` 覆盖原件往返、现有 writer 拒绝、晚到子任务保留、原路径重建和最后时刻竞争、journal/移动后故障、半写日志不可发布、损坏记录隔离、撤销/错误项目/硬链接拒绝、真实服务 caller/source/admission、重复操作拒绝及冻结释放。

其中 `OfficialSdkDiscoveryOmitsRecoveryAndRestores_006` 使用项目固定 Claude Agent SDK 0.3.301、限定只读文件权限和全新合成配置，实测 `listSessions()` 从可见到移除后不可见、恢复后再可见。索引字节和子任务数据不变。这是官方 SDK 发现验收，不冒充真实 Claude TUI `/resume` 或安装版 WebView 验收。

前端 `nativeSessionRecycleApi.test.ts` / `nativeSessionRecycle.test.ts` 覆盖独立范围与确认、取消、单次发送、未知结果跨重开保护、持久日志重新发现、恢复冲突及来源变化。永久删除、批量删除、capabilities 与类型检查继续回归。

所有操作验证只针对本任务创建的临时假数据，未操作真实会话、未修改生产 ACL/凭据，未发布。


### 本轮本地结果（2026-11-12，WSK-MSI）

- 首轮 6 项 OS/固定 SDK 用例通过。新增服务往返用例随后复现 metadata freeze 未释放：10 项通过、1 项以 `SOURCE_BUSY` 失败；显式收口后 11 项全部通过。
- `cargo clippy --locked --all-targets -- -D warnings`、`cargo fmt --all -- --check`、Vue/TypeScript 类型检查通过。Cargo 现有全局 Windows `nul` 缓存清理警告未通过清缓存绕过。
- 7 个受影响前端测试文件共 75 项通过，含新 API/UI 8 项和既有永久/批量删除、store、capabilities 回归。
- 独立首次审查提出未知意图重发和半写日志遮蔽问题，均已增加实现与回归；修改后的独立复审仍待安排。本记录不代表完整 RC gate 或三平台安装验收。
- 未运行全量 Rust/前端、真实 Claude TUI picker 或本功能安装版 WebView 操作。没有 OpenCode 可恢复移除实现，也没有开放 Claude/OpenCode 永久原生删除。


### 独立复审增量（2026-11-12，WSK-MSI）

固定基线 `e5f681ac` 的第二轮审查发现后续 cwd 误拒、单 reparse 子项遮蔽健康记录、未准入 journal 路径先被打开、恢复 rename 的未知结果误报冲突四项。4 项均用合成数据复现为 RED（14 项中 10 通过、4 失败），没有执行 UNC/网络探测。

修复后 `NativeRecycle_` 16 项全通过，新增反向验证错误首 cwd 不会被后续正确 cwd 覆盖，以及恢复已生效但回执丢失只读显示 source。既有严格 `HistoryWindows_Enumerate_024`、`HistoryWindows_RejectReparse_015` 各 1 项通过；严格 all-target Clippy、fmt 通过。前端与 IPC DTO 未变化，本增量不重复计算此前 75 项前端结果。当前增量已交独立审查员复核，尚未用安装验收替代此结论。
