# Claude / OpenCode 永久删除的后续维护设计

本文件的维护执行合同仍为下一阶段设计，**没有开放删除执行**。0.70 已接入下述只读来源诊断切片；两家原生删除继续关闭；不重复已被阶段9反例推翻的分享锁/最后一次idle查询方案，不用隐藏或软归档替代永久删除。已证事实和固定源码见 [阶段9审计](../.audit/v027-stage9-provider-delete.md)。当前开发只允许任务新建合成源，不操作真实用户会话/服务、权限或凭据。

## Claude：分离原件来源与维护准入

已证来源切片：固定 SDK `0.3.301` 可以通过任务私有 project junction 和受限 Node FS 权限，删除合成原始 JSONL 及同名目录。现有 rename hardlink 只删除别名，不复用。未来适配必须继续绑定 authenticated caller、profile/revision、held source root、注册项目和原始 UUID；锁定 alias/原容器身份，拒绝非受控或意外reparse；任务私有project junction单独校验，同时拒绝换源、worktree 搜索扩展和 sessionStore 成功no-op。

活动保护需要另一个真实控制，建议先定义维护合同，再选实现：

1. **优先的协作式合同**：CLI/SDK 提供可撤销维护租约，覆盖主文件与选定原生子树，排除已有 writer、创建子项和恢复；所有正常 CLI writer 都必须遵守。当前没有该公开租约，Desk 的普通 mutex 不会让外部 CLI 遵守。只有所有实际 writer 均已纳入受控来源且可验证无未知外部 owner 时，才可开放这个窄来源；用户点“已关闭窗口”本身不是证明。
2. **可独立研究的 Windows 维护模式**：在明确产品范围和用户操作后，才考虑对准确目标项目/子树做短暂 creation/write barrier，并逐个持有文件身份。现有实验只证明新 fixture 的 WD/AD deny 能挡创建，未交付生产权限方案。它会改变目录访问行为，可能使其他 CLI 操作失败，不能悄悄变成删除副作用；当前没有真实权限变更授权，不执行。

第二条若获准开发，最小实现顺序是：

- 先持有来源与容器，快照每个存活对象的原 DACL 和物理身份；拒绝不具备正常 READ_CONTROL/WRITE_DAC 的对象，不提权、不改owner/SACL。覆盖创建主JSONL的项目目录和动态子目录；只锁已知文件不足。
- 以可恢复持久日志记录“原状态/精确对象/是否已施加”，再施加限定屏障；崩溃恢复必须按原对象身份恢复，不能在同路径新目录上重放旧ACL。取消和施加失败先恢复本次已修改项，未发SDK删除则明确本次未调用SDK删除；恢复不确定要阻断后续动作并显示需要人工处理。
- 屏障成立后重新枚举、核对身份、活动/恢复状态和范围摘要。已变化使旧确认失效，恢复权限后重新预览。两次确认不可合并；用户最后确认前不改ACL、不调用SDK。
- SDK 只调用一次，不给其全projects或任意外部路径权限。旧 DELETE 句柄仍可能把对象迁出，故必须结合 held-object 身份、链接/删除状态和原 namespace 事后核查；不能仅凭SDK success或名字消失宣布原件全删。发现迁出不追着外部路径再删。
- 主JSONL先删、child失败时按 native SDK 的实际部分结果呈现；每根独立账本，未知先只读核查，禁止自动重放。权限恢复结果与删除结果分别记录，不能以恢复成功掩盖部分删除。

这不是承诺 ACL 足以解决全部外部活动，也不是要求所有平台共用Windows机制。若屏障不能涵盖普通外部 writer，就仅禁用该来源；不宣称全平台不可能。

验证入口：现有 `ClaudeDelete_` 10项 OS/SDK反例继续作为边界；新合同另加 task-owned writer/late child/旧ADD与DELETE能力、reparse/目录替换、取消、进程崩溃、权限施加/恢复部分失败、SDK EBUSY与同名重建矩阵。必须验证恢复到原DACL、外部合成marker保留和SDK原件结果；不再重跑相同假设作为新能力证明。正常用户流程只能在合同真实成立后显示可执行，二次确认不是越过保护的许可。

## OpenCode：保留正常服务来源，再增加真正的拒忙准入

当前 private preparation server 在操作后销毁，Native TUI owner 不等于持久HTTP服务。`session delete` CLI 仍走 ServerConnection，默认/standalone可能启动或替换服务；启动可能调度/恢复。后续不能把创建服务藏进删除动作。

可做的第一个切片是**正常生命周期的受控服务 owner**，不是先接DELETE：

- 在明确的正常启动/连接流程中取得授权，保留服务generation、实际进程handle/Job、限定endpoint与短期授权、冻结profile/source/cwd和实际配置来源。选择已有服务时必须验证对应来源，loopback、PID、路径自报、版本或相同session元数据不够。
- 应用重启、服务替换、端口复用、profile/root变更立即撤销旧票据。不要扫描真实注册/口令来伪造来源；当前没有该链路就维持不可用。确需保留现有private helper或改变TUI连接生命周期时，应独立审阅启动副作用和所有权，不算本轮已完成适配。
- 如果采用“该精确profile自己的原生store”语义，需由实际受控原生连接核验其配置/数据集；不一律要求任意HTTP服务给出完美的物理DB证明，也不能拿仅Desk一端held DB替它作证。

第二个切片是**服务内部的 delete-if-idle / maintenance admission**：

- 准入必须同 run/wake、恢复、队列和schedule启动共用实际互斥边界，能拒绝busy/unknown，而不是调用现有remove先interrupt。跨实例共享同DB时，还需所有writer遵循的原生lease；只控制本进程Map或写time_suspended不够。
- 可以按明确根及执行时原生子树确认，不强制任意expected-ID集合API。执行前发现成员/来源变化仍使旧确认失效；动态范围风险需如实告知，不能把项目根列表说成完整树。
- 现有remove会取消job/transport、逐项递归和清环境。新的准入若只是包一层HTTP查询，没有覆盖执行期间的启动窗口，不能宣称“不停止活动”。外部活动不明则拒绝，不能替用户停止它。
- 按根记录operation ID和原生ACK/失败；恢复中的服务或连接断开一律unknown，先只读核查原根、已观察children和原生状态，绝不盲重放。晚到child可能留为孤儿，根消失不是全树清理成功；逐项观察和无法证明的范围分别展示。

第三个切片才是产品双确认→该受控服务→原生删除适配。首次选择/只读预览不启动服务、不调用remove；二次确认核对不可恢复、子会话及关联blob/事件影响、未覆盖的项目文件/外部副本。关闭窗口不清空账本或停止未知owner。服务崩溃后旧generation失效，连接重建只为只读核查，不自动重发删除。

需要新增的协议或用户操作：受控正常服务的明确连接/启动步骤、可验证来源契约，以及真正服务内部维护/拒忙机制。它们当前均未交付，不能用风险确认代替。修改上游或另建平台不在当前阶段授权内。

验证入口：现有固定 remove 函数体的5项SQLite模型观察只做反例基线；新合同须另用任务合成的真实受控服务，覆盖A/B同元数据DB、generation/端口替换、外部claim/晚启动、递归中的新增child、活动恢复与schedule、断连/部分失败/重复请求及源切换。正常生命周期、只读预览和删除分别统计副作用；没有实际授权实例时不跑，也不把模型GREEN写成原生服务验收。

## 两家共同交付门槛

来源、活动/维护、双确认、逐根结果和恢复失败各自有证据才开放相应能力。新增危险能力先合成RED、最小实现、独立安全审查和固定工具链受影响验证；真实数据操作另有授权。维护执行仍仅保留设计；只读诊断不执行权限改动、启动真实服务、调用模型或删除真实用户数据。


## 0.70：只读目标诊断切片

`cli_preview_session_delete` 现在对 Claude/OpenCode 也重新核对 authenticated caller、精确 profile/revision、held root、注册项目和当前 sessionKey。它使用既有只读投影授权，Claude 额外核对原始 transcript 的 UUID/cwd，OpenCode 使用实际选定 SQLite 的受限只读快照；不启动 CLI/服务，不调用 SDK、不执行 SQL 写入，也不修改 ACL。

- 成功观察到目标后，Claude 返回 `unsupported / DELETE_MAINTENANCE_UNAVAILABLE`，OpenCode 返回 `unsupported / DELETE_SERVICE_UNVERIFIED`，同时提供已核验的原生 ID。两个原因分别指出维护排他能力、受控服务来源/准入仍未接通，并不表示提供方没有原生删除 API。
- 响应没有 previewId，不进入执行账本；rootArchived 和 knownDescendantCount 保持 null。Desk 的显示归档或当前只读投影的默认字段不作为两家的原生归档证明，也不编造子树范围。
- Desk 存在活动/准备/未知 owner 时返回 SOURCE_BUSY；目标无法唯一归属时返回 DELETE_SOURCE_UNAVAILABLE 且不发布原生 ID。来源或文档失效继续使用既有安全错误。
- 前端诊断协议按提供方校验 ID：Claude/Codex 仍是 UUID，OpenCode 是有界 `ses_` locator，保留大小写。任何 unsupported/blocked 响应携带执行票据都会拒绝；两家伪造 ready 响应也拒绝。后端账本另有 Codex-only 发票据保护，不能通过直接调用账本绕过尚未实现的适配。
- 用户界面现有两家删除禁用入口保持不变。此切片是安全适配基础，不是永久删除功能交付，也没有将 TODO 3 标为完成。

回归入口：`ProviderDelete_` 三条 Rust 测试使用任务临时 Claude JSONL/OpenCode SQLite，逐字节比较来源不变，覆盖已核验身份、虚假目标、活动 owner、错误物理来源、失效文档、伪造确认/核查和账本拒绝发票据。`providerSessionDeleteApi.test.ts` 覆盖两种 ID、非法 locator/原始错误载荷及不可执行诊断。既有 Codex 删除、单项/批量 UI 和未知结果账本测试继续独立执行；它们不能替代两家真实删除、外部 writer 或崩溃恢复的验收。

2026-11-12 WSK-MSI 本地验证：新增前端诊断先 RED 后 GREEN；5 个删除相关前端文件共 68 项通过，TypeScript/Vue 类型检查通过。新增 `ProviderDelete_` 3 项先全部 RED 后全部 GREEN；既有 `NativeDelete_` 16 项、`native_cli_projection_service` 19 项通过（后者含 2 项新增重复）。`cargo fmt --all -- --check` 和 `cargo clippy --locked --all-targets -- -D warnings` 通过，独立只读审查无阻断发现。Cargo 提示既有全局 cache 的 Windows `nul` 元数据清理失败，但命令最终成功；未改缓存清理策略。未运行全量 Rust/前端、真实提供方删除或安装包 WebView 验收，不把本切片测试冒充上述证据。

## 0.70：独立可恢复产品切片

后续实现了 [Claude 主记录可恢复移除](recoverable-claude-removal-0.70.md)：不要求未知外部子任务已完全静止，因为它们保留原地且不会 purge；主 transcript 原件以精确身份移入持久恢复区，冲突恢复不覆盖。该独立功能不能替代本文件的永久原生删除合同，OpenCode 的整库/跨实例维护边界也未因此解决。
