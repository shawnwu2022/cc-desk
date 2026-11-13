# Native Host approval component

Implement a native-only, one-shot approval owner. A frozen backend description is moved through approval, never reconstructed from JavaScript. Pending lasts 30 seconds and approval at most five minutes, capped by the supplied policy/profile deadline. These are implementation defaults. No restart persistence, unattended activation, replenishment or automatic retry.

Plan: add the isolated state machine and native dialog callback; test refusal, expiry, cancellation, single consumption and late callbacks; compile the remote-host feature. The production description/material supplier and application trigger remain separate gates. No placeholder supplier is added. Windows Hello is a future stronger-presence adapter, not implemented or certified.

The dialog plugin returns no join or close handle. Its callback retains only a weak owner. Timeout invalidates authority but does not close the dialog or prove its detached worker retired. Callback completion must never count as key, run or network retirement. The component must not own those resources.

Implementation status: the concrete native description is rendered internally and moved once into registration. A shared sticky revoke lease is checked by the original source-clock chain and closes a late registration. Tokens remain private to the owner; no refill method exists. Grant duration remains distinct from the Host approval cap, which is bounded by expires_host_ms and profile_cap. The caller must poll_expiry to discard expired description memory; no unowned timer is spawned and no strong run references are retained. The dialog callback holds only Weak state. Production UI trigger and key/profile supplier are not connected. Focused tests and the existing registered-source fixture are required; passing the state-machine tests alone is not production approval-chain evidence.

Only one native dialog may be outstanding process-wide. Expiry/cancellation does not release this quota until the original callback returns; plugin scheduling failure therefore stays fail-closed. A Yes response is not Windows Hello proof or protection against OS-level automation. Frozen local endpoint role/identity and relay key-id/role are compared with actual supplied materials. Relay origin is the frozen HostSnapshot origin used by the original transport; RelayAdmissionIdentity itself has no origin field. Canonical-grant presence remains a separate gate.

Pending now retains only weak NativeRunNetworkOrigin, not network_checks or NativeRunAdmission. Registration calls the supplied admission's network_origin (which performs the same original access and authority validation), compares original identity, and then calls the original service registration, which validates again. Yes records intent only; it is not current run authority. No pending callback holds the original run alive.

## App-owned controller boundary

The remote-host feature now manages one application controller and exposes current/request/cancel only. Each command authenticates the original document and run through the existing raw-body admission path. JavaScript supplies only runId/generation; unknown fields, duplicate fields and wrong types are rejected by the shared DTO. Feature-disabled commands return REMOTE_HOST_UNAVAILABLE without decoding or accessing controller state. Frontend wrappers require the native document bridge and never fall back to raw invoke.

Production has no installed profile configuration or material factory, so authenticated calls remain Locked. There is no start IPC, new UI button, production key adapter, unattended mode or readiness claim. Backend configuration is not IPC-installable. Once an approval slot is created, it remains occupied for the controller session; cancellation does not automatically replenish configuration or open another dialog.

Wrong document/run requests cannot consume another owner's configuration. A pending request retains its original identity and cancellation flag while the dialog is scheduled outside the controller lock. Cancellation before publication invalidates the returned owner. Closing begins controller cancellation at the existing Host exit entry; the existing document handoff refusal still runs first. It does not replace original exit/Unknown/join rules.

ActivationTicket is private, move-only and consumed before the fixture factory runs. Consumed reports this intermediate state without claiming a network start. A revoked lease reports Cancelled even if the state-machine object has not independently closed. Frozen supplier preparation now uses the original production-capable custody and operation code, but its only start caller remains under cfg(test). The anonymous fixture verifies actual original-thread joins and post-join registration; it does not qualify a production existing-key loader or GUI activation.

Verification separates 32 synchronous reentrant interleavings from 16 actual two-thread cancellation cases at clock return / token publication. The fixture checks material release, zero new network attempts, original owner retirement and no reuse. A distinct actual-thread test cancels a pending show before publication. Real native dialog interaction, Windows Hello, production profile/key acquisition and late production worker retirement remain separate acceptance gates.

Registered is only a historical native-source registration fact, not connection availability, current approval validity or permission to consume again. It must not be mapped to Ready. Current does not call an external GrantClock; later network actions retain the original source-clock authorization checks. Local deadline and sticky lease revocation take precedence in reported status.

## 生命周期首片边界

[单次启动保管链设计与测试计划](native-host-activation-lifecycle-plan.md) 记录首片及后续独立分片的审查、RED/GREEN 证据边界。生产代码新增 service 唯一 activation 槽，预留先于 ticket 内外部 clock，原准备线程真 join 后才允许显式完成登记。查询没有推进副作用；未消费结果仍 Pending。退出由 operation 保管独立材料释放线程，原唯一 waiter 持续观察，不同步析构阻塞材料。后续 2B 增加可选双 key custody、冻结两用途 supplier 契约和 operation 共推进，只有公开匿名 fixture 驱动。后续批准绑定片已将原 ApprovalLease 的同一 native 撤销 token 接入冻结供应、operation、两 key 与原网络停止门禁，并用匿名 fixture 验证严格准入和显式登记。生产供应安装、start IPC 与真实配置仍未实现；当前生产无配置仍 Locked，不能称为真实密钥供应者已实现，也不扩大 native dialog 的批准或退休证明。

## 原批准与冻结供应绑定

批准 lease 创建时唯一生成只可置位的 NativeRevocation。cancel 先纯撤销，再执行原 watch/source 通知；clock 失败、过期、panic 与已取得 claim/ticket 的失败 Drop 只撤销资格，不在准备或释放线程中升级 service。外层 SourceClock 对回退、host-ms 到期和其他失败也纯撤销同 token；panic 的原 Unknown 分类保持。

顺序为严格 document/run/config owner 门禁、原 service reservation、无外部回调的一次性 claim、安装同 token custody、锁外验证 clock、准备原线程。claim 移动原冻结 bundle 和原 approved Instant end；外国请求及重复 Consumed 在 claim 前拒绝，不消耗或撤销合法批准。原准备线程真正 join 后才显式登记，current/snapshot 不推进登记或采外部 clock。

FrozenHostApproval 将公开描述和两个私有一次性 opener 的 FrozenHostSupplier 绑定为同一个移动对象。HostKeyProfileIdentity 是原生签名配置域的 [u8;16]/u64 身份与 revision，不是 CLI Profile 的字符串/WireU64，也不是 WebAuthn BrowserProfile。Endpoint/Relay 的不同 SPKI、Host role/id、origin、原 run/resource 与 grant scope 在批准前核对；展示来自同一冻结对象，Yes 后不读取可变 profile。Issuer 只有验证公钥，供应接口没有 Issuer opener。

私有 RegisteredHostSource 精确保留原 token/end 的 NativeApprovalStop。源检查、注册后准入、补充材料和最终 commit 门禁均读取这个无回调事实；原 setup/established select 用有界 timer 等待，不另建线程，不采 GrantClock。纯撤销也会沿原 stop+join 收口空闲连接。资格关闭不等于物理退休，Unknown 与原 handle outstanding 继续分别记录。

完整同源码证据和验收边界见计划中的“批准与冻结供应绑定片”。012 实测两匿名 CNG key 各打开一次并立即完成 Relay/TLS 握手，Registered 时资源仍 outstanding；没有真实用户凭据或生产入口。当前 facade 仍最多五秒，晚到的 ClientHello 可能已失效，准备/登记不代表持续握手可用。分阶段一次性签发另行设计，不延长五秒、不重开 key、不自动重试或续期。
