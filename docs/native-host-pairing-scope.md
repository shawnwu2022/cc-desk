# Native Host 公开会话范围

`remote_host_pairing_scope` 是 Windows `remote-host` 的只读出口。它复用原 authenticated document bridge、严格原始 `NativeRunQuery`（仅 runId / u32 generation，原 1024-byte 上限）、`admit_host_control` 和 `NativeRunAdmission.scope()`。返回前再次核对原 document/caller。feature-off 或非 Windows 直接固定返回 `REMOTE_HOST_UNAVAILABLE`。

返回仅含 `session`、`run`、`generation` 三个字符串，与 Browser `grant.scope` 的形状一致。`session` 来自实际 native producer 的随机会话标识，不是 CLI 历史 ID，也不能由前端 runId 推导。此投影不是当前在线状态、配对成功、批准或可调用权限。原 run 退出、document 撤销或不匹配 caller 后不能重新读取。

该入口不安装候选、不打开密钥、不生成凭据、不联网、不持久化、不取走 producer，也不访问 controller 或改变批准、消费、原截止时间。返回字段不包含路径、key name、credential ID、principal、token、终端内容或原始错误。

生产 RemoteHostPanel 仅在用户点击“读取公开会话范围”时调用，不自动复制或发送。原完整 NativeAttempt（requestId/runId/generation）、面板 epoch 和 open 状态拒绝晚结果；关闭、卸载、原 attempt 变化或停止后清空。只读失败不启动授权清理，也不重试。面板显示该公开 scope，不为 Browser 建立信任。

## 当前端到端剩余步骤

| 环节 | 当前代码已有 | 尚需完成或单独验收 |
| --- | --- | --- |
| Host 本机入口 | 严格公开候选冻结、实际 run 归属、本机一次批准、显式 start、owned driver/network，见 remote_host_configuration.rs / remote_host_ipc.rs | 真实 named-key 引用与公开元数据的可信供应；实际安装应用面板和原生批准对话框验收 |
| Browser 目标 | browserEntry.ts 严格验证 grant.scope；本片提供原生公开 scope | 用户明确选择并绑定 scope；不把展示结果当批准或 enrollment |
| Browser 身份与批准 | browserApproval.ts / browserGrant.ts / browserTlsSigner.ts 使用分离的 non-exportable CryptoKey、WebAuthn assertion 与 TLS 签名；browserEntry.ts 接收已批准 inputs 和 presenter | 真实独立 endpoint / issuer key handles、credential、relay signer 和公开 pins 的可信 enrollment、管理、撤销/恢复；真实设备验证 |
| Browser 配置供应 | browserProfileSource.ts 从同源受控 /browser-profile.js 读取受信模块，每页只读一次并保留原 source；[公开配置核对 helper](browser-pairing-profile.md) 严格解析候选，与独立可信 provider 完整快照匹配后组合原 handles | 可信模块与 presenter 的实际供应/部署。换 run/generation 后需新的显式页面/连接流程，旧 source 不会自动更新 |
| Relay | 同级 cc-desk-relay 的 rendezvous opt-in 与显式 policy / TLS chain / TLS key / TLS CA WSS 启动器已实现 | operator 可信配置、真实 TLS/Origin 信任、授权部署与真实网络验收 |
| 组合验收 | 原已有匿名 TEST_ONLY keys、原生 owned 子进程、relay、TLS/AppBind/grant/数据流隔离证据 | 实际手机/浏览器到安装 Host 的授权全链，不能由本片单元/DOM/原生 fixture 代替 |

本片不生成、读取或存储真实用户凭据，不修改安装应用或系统权限，不部署或发布。历史文档里“未接线”的结论应按上述当前代码与其历史阶段分别理解。
