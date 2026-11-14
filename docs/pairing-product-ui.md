# 本机记住设备与锁定恢复 UI

## 交互与授权边界

入口仍是当前 Native CLI 运行会话的“单次远程连接”。保留既有公开候选冻结、首次登记、独立 Host 本机批准与单次启动流程。

- 首次登记新增“在本机记住公开设备（Remember，可选）”。默认不选，每次关闭、切换运行身份或卸载后重置。候选 JSON 的 `remember` 不能替代勾选；前端先拒绝重复字段，再只替换顶层布尔字段，保留其他字段的原始数字词法。注入后的公开 JSON 仍受 16 KiB 限制。
- 勾选不等于保存成功，也不批准 Host 控制。原生流程仍须独立询问是否记住；No、失败和 Unknown 不自动重放登记。`NativeStoredRegistrationIncomplete` 明确保留本机已保存的事实，但不声称浏览器已安装。
- “刷新已记住设备”是显式只读操作。面板打开不自动读 store，不创建、修复或初始化 store。列表只展示公开摘要，固定标为 Locked，不展示或恢复旧批准、密钥、凭据或网络连接状态。
- “用于当前会话”固定列表的 storeId、revision、deviceId，只请求后端创建当前 owned run 的新锁定候选。仍需另行本机批准及启动，不自动打开密钥、不连接、不自动重试。已有候选或登记时禁止覆盖。
- “忘记设备（本机确认）”独立触发原生确认，只移除本机记住记录。界面明确说明：不终止独立已批准的连接，不等同于“取消权限”。Forgotten 后只读刷新列表；刷新失败保留忘记成功事实，不重放 mutation。Declined 不重试、不冒充成功。异常不显示原始载荷，并清除旧列表操作目标，要求用户只读核对。

## IPC 契约

全部经过当前 authenticated document bridge，不回退 raw invoke。

- `remote_registration_request`：外层 `{runId, generation, candidateJson}` 不变；公开 JSON 顶层注入 checkbox 决定的 `remember`。
- 新登记状态：`PendingNativePersistence`、`NativeStoreDeclined`、`NativeStoreFailed`、`NativeStoreUnknown`、`NativeStoredRegistrationIncomplete`。InstalledLocked / DeliveryUnconfirmed / RetirementUnknown 返回 `remembered` 历史事实。
- `remote_pairing_list({runId,generation})`：`{state:'Locked',storeId,revision,devices:[{deviceId,browserOrigin,relayOrigin,profileId}]}`。空 store 的 ID 和 revision 为 null。
- `remote_pairing_select`：同 configuration envelope；candidateJson 为 `{storeId,revision,deviceId}`。返回 `{state:'Configured',receipt}`，UI仍显示尚无批准。
- `remote_pairing_forget`：同 configuration envelope；candidateJson 为 `{storeId,revision,deviceId,requestId}`。每次点击生成新 UUID，返回 Forgotten / Declined。
- `remote_pairing_forget_cancel`：同 configuration envelope；candidateJson 仅含 `{requestId}`，返回 void。UUID只是精确取消标识，不是权限。后端需要在同 document/run 内记录提前取消，避免 cancel 比 request 先到时丢失取消。

## 生命周期与竞态

面板复用既有 attempt（requestId/runId/generation）与 epoch 栅栏，所有 await 后重新核对。`UnifiedTerminalHost` 在隐藏或切换 active native tab 时卸载本面板，因此执行同一 invalidate 清理。

- 同一面板共享 busy 单飞，重复点击不重复发出写操作。关闭始终可用。
- select 的迟到成功 receipt 按原 run 精确 discard，不安装到新 UI；失败不显示原始错误。
- Forget 发送前记录 UUID。关闭、卸载、运行身份失效时立即对原 run + UUID 发取消，不等待原确认返回。“取消忘记确认”只取消这个操作，不调用 Host cancel。取消时维持单飞直到原 await 完成，之后要求只读核对。
- 同 run 重开生成新 UUID，旧 cancel 不得撤销新 Forget。迟到 Forgotten / Declined 不刷新新面板，也不清除新操作的 busy。前端 epoch 只保护 UI；迟到 Yes 不提交存储依赖后端精确取消与同步 commit gate。
- Forget 不调用 Host cancel/discard。面板整体关闭仍按原有约定清理本面板拥有的 Host 候选/请求，该行为与 Forget 操作本身不同。

## 验证与证据边界

`tests/native-cli/remotePairingProduct.test.ts` 用合成 IPC 覆盖：默认不保存与 JSON 不能代替勾选、重复键/非对象拒绝、数字词法保留、存储 No/失败/Unknown/保存后浏览器失败、锁定选择、重复点击、迟到 receipt 丢弃、运行身份变化、独立 Forget、列表刷新、旧 UI 回包隔离、精确 Forget 取消。

`tests/native-cli/launchApi.test.ts` 覆盖新增命令的精确 bridge 参数、原错误传播与 bridge 缺失不降级。既有 RemoteHostPanel 测试继续覆盖登记和单次 Host 批准边界。

这些测试不打开真实 GUI、原生确认、密钥或用户 store，不创建真实凭据，不建立 relay 连接。实际原生存储、迟到 Yes 写屏障、平台弹窗和安装包 WebView2 的端到端验证仍须后端与目标平台独立证据，不能由 jsdom 结果代替。
