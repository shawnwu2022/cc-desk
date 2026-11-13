# Native Host approval component

Implement a native-only, one-shot approval owner. A frozen backend description is moved through approval, never reconstructed from JavaScript. Pending lasts 30 seconds and approval at most five minutes, capped by the supplied policy/profile deadline. These are implementation defaults. No restart persistence, unattended activation, replenishment or automatic retry.

Plan: add the isolated state machine and native dialog callback; test refusal, expiry, cancellation, single consumption and late callbacks; compile the remote-host feature. The production description/material supplier and application trigger remain separate gates. No placeholder supplier is added. Windows Hello is a future stronger-presence adapter, not implemented or certified.

The dialog plugin returns no join or close handle. Its callback retains only a weak owner. Timeout invalidates authority but does not close the dialog or prove its detached worker retired. Callback completion must never count as key, run or network retirement. The component must not own those resources.

Implementation status: the concrete native description is rendered internally and moved once into registration. A shared sticky revoke lease is checked by the original source-clock chain and closes a late registration. Tokens remain private to the owner; no refill method exists. Grant duration remains distinct from the Host approval cap, which is bounded by expires_host_ms and profile_cap. The caller must poll_expiry to discard expired description memory; no unowned timer is spawned and no strong run references are retained. The dialog callback holds only Weak state. Production UI trigger and key/profile supplier are not connected. Focused tests and the existing registered-source fixture are required; passing the state-machine tests alone is not production approval-chain evidence.

Only one native dialog may be outstanding process-wide. Expiry/cancellation does not release this quota until the original callback returns; plugin scheduling failure therefore stays fail-closed. A Yes response is not Windows Hello proof or protection against OS-level automation. Frozen local endpoint role/identity and relay key-id/role are compared with actual supplied materials. Relay origin is the frozen HostSnapshot origin used by the original transport; RelayAdmissionIdentity itself has no origin field. Canonical-grant presence remains a separate gate.

Pending now retains only weak NativeRunNetworkOrigin, not network_checks or NativeRunAdmission. Registration calls the supplied admission's network_origin (which performs the same original access and authority validation), compares original identity, and then calls the original service registration, which validates again. Yes records intent only; it is not current run authority. No pending callback holds the original run alive.

## App-owned controller boundary

The remote-host feature now manages one application controller and exposes current/request/cancel only. Each command authenticates the original document and run through the existing raw-body admission path. JavaScript supplies only runId/generation; unknown fields, duplicate fields and wrong types are rejected by the shared DTO. Feature-disabled commands return REMOTE_HOST_UNAVAILABLE without decoding or accessing controller state. Frontend wrappers require the native document bridge and never fall back to raw invoke.

Production has no installed profile configuration or material factory, so authenticated calls remain Locked. There is no start IPC, new UI button, unattended mode or readiness claim. The private open-only key adapter described below has no production configuration source or caller. Backend configuration is not IPC-installable. Once an approval slot is created, it remains occupied for the controller session; cancellation does not automatically replenish configuration or open another dialog.

Wrong document/run requests cannot consume another owner's configuration. A pending request retains its original identity and cancellation flag while the dialog is scheduled outside the controller lock. Cancellation before publication invalidates the returned owner. Closing begins controller cancellation at the existing Host exit entry; the existing document handoff refusal still runs first. It does not replace original exit/Unknown/join rules.

ActivationTicket is private, move-only and consumed before the fixture factory runs. Consumed reports this intermediate state without claiming a network start. A revoked lease reports Cancelled even if the state-machine object has not independently closed. Frozen supplier preparation uses the original production-capable custody and operation code. The internal prepare_owned driver is now compiled outside cfg(test), but has no production configuration or application trigger; only anonymous fixtures invoke it. It owns autonomous preparation and post-join registration without query/IPC observer progress. This does not qualify actual user existing-key access or GUI activation.

Verification separates 32 synchronous reentrant interleavings from 16 actual two-thread cancellation cases at clock return / token publication. The fixture checks material release, zero new network attempts, original owner retirement and no reuse. A distinct actual-thread test cancels a pending show before publication. Real native dialog interaction, Windows Hello, production profile/key acquisition and late production worker retirement remain separate acceptance gates.

Registered is only a historical native-source registration fact, not connection availability, current approval validity or permission to consume again. It must not be mapped to Ready. Current does not call an external GrantClock; later network actions retain the original source-clock authorization checks. Local deadline and sticky lease revocation take precedence in reported status.

## 生命周期首片边界

[单次启动保管链设计与测试计划](native-host-activation-lifecycle-plan.md) 记录首片及后续独立分片的审查、RED/GREEN 证据边界。生产代码新增 service 唯一 activation 槽，预留先于 ticket 内外部 clock，原准备线程真 join 后才允许显式完成登记。查询没有推进副作用；未消费结果仍 Pending。退出由 operation 保管独立材料释放线程，原唯一 waiter 持续观察，不同步析构阻塞材料。后续 2B 增加可选双 key custody、冻结两用途 supplier 契约和 operation 共推进，只有公开匿名 fixture 驱动。后续批准绑定片已将原 ApprovalLease 的同一 native 撤销 token 接入冻结供应、operation、两 key 与原网络停止门禁，并用匿名 fixture 验证严格准入和显式登记。生产供应安装、start IPC 与真实配置仍未实现；当前生产无配置仍 Locked，不能将隔离供应适配验收称为真实用户密钥供应链已验收，也不扩大 native dialog 的批准或退休证明。

## 原批准与冻结供应绑定

批准 lease 创建时唯一生成只可置位的 NativeRevocation。cancel 先纯撤销，再执行原 watch/source 通知；clock 失败、过期、panic 与已取得 claim/ticket 的失败 Drop 只撤销资格，不在准备或释放线程中升级 service。外层 SourceClock 对回退、host-ms 到期和其他失败也纯撤销同 token；panic 的原 Unknown 分类保持。

顺序为严格 document/run/config owner 门禁、原 service reservation、无外部回调的一次性 claim、安装同 token custody、锁外验证 clock、准备原线程。claim 移动原冻结 bundle 和原 approved Instant end；异源请求及重复 Consumed 在 claim 前拒绝，不消耗或撤销合法批准。原准备线程真正 join 后才显式登记，current/snapshot 不推进登记或采外部 clock。

FrozenHostApproval 将公开描述和两个私有一次性 opener 的 FrozenHostSupplier 绑定为同一个移动对象。HostKeyProfileIdentity 是原生签名配置域的 [u8;16]/u64 身份与 revision，不是 CLI Profile 的字符串/WireU64，也不是 WebAuthn BrowserProfile。Endpoint/Relay 的不同 SPKI、Host role/id、origin、原 run/resource 与 grant scope 在批准前核对；展示来自同一冻结对象，Yes 后不读取可变 profile。Issuer 只有验证公钥，供应接口没有 Issuer opener。

私有 RegisteredHostSource 精确保留原 token/end 的 NativeApprovalStop。源检查、注册后准入、补充材料和最终 commit 门禁均读取这个无回调事实；原 setup/established select 用有界 timer 等待，不另建线程，不采 GrantClock。纯撤销也会沿原 stop+join 收口空闲连接。资格关闭不等于物理退休，Unknown 与原 handle outstanding 继续分别记录。

完整同源码证据和验收边界见计划中的“批准与冻结供应绑定片”。012 实测两匿名 CNG key 各打开一次并立即完成 Relay/TLS 握手，Registered 时资源仍 outstanding；没有真实用户凭据或生产入口。该历史片中的 eager facade 最多五秒，晚到的 ClientHello 可能已失效；后续单次阶段签名片改变签发时点，不延长五秒、不重开 key、不自动重试或续期。

## 原网络任务的单次阶段签名

准备现在只产生两惰性 facade 和不可 Clone 的 HostSigningStages。唯一阶段控制权随原材料槽进入原网络任务；只有原 registration 构造的 NetworkSigningScope 能绑定它，同一批准 token 身份和原 cap 都须精确一致。相同公开 key 不允许混用另一 lease 的材料。异源材料退回原 A owner，不撤销 B 或消耗 B 的 fresh 槽；丢弃退回材料只撤销 A。

原任务先 arm Relay，第一次 sign 调用唯一占有许可，第一次 poll 才创建 min(now+5s, 原 cap, 原 setup end) 的 attempt。原 connect_admitted 返回 Accepted 后才 arm Endpoint，实际 TLS Signer::sign 消费其一次许可。choose_scheme 可重复探测但不 mint；同一 fence 的 generation 只在完成前一签名交付后进入下一阶段，已交付 Relay 签名的旧五秒到期不会被当成网络寿命。失败、未 poll/queued Future Drop 或原任务结束都不能归还许可或续领。

Host Relay 手写 Future 直接保管原 SignTicket，Drop 先纯撤销；poll 栈的纯 token guard 在 panic 时先失效，再允许后续票据/waker 析构。阶段锁不包 fence、adapter、等待、外部 hook 或句柄析构；原两 key/custody/Unknown/原 join 保管链未替换，Client 的默认无 token 语义不变。原 owner 的四个已确认失败点只为匹配原请求先纯撤销 Host token，再允许通知/析构；不改变 RequestFact、Unknown 或原 join。

延迟 fixture 只证明原预算内的准备后等待，以及 Relay Accepted 后、Endpoint arm/mux 创建前由原任务保管的门控延迟。原 mux 启动后 2 秒无 Binary 的 IO idle 限制仍存在，不能称其可以空闲等待 Phone 超过该期限。原任务正常受保管退出和 work panic 已覆盖，不宣称任意强制丢弃整个 runtime Future 或进程异常能完成退休。该阶段签名片结束时，生产安装、真实 loader、start IPC/UI 与历史 Phone-first Offline 仍未解决。后续已有密钥适配的实现和剩余验收边界见下节。
## 已有密钥供应适配的当前边界

私有 NativeHostProfile 现将原公开 binding 与 Endpoint/Relay 两个已有 key 引用冻结为不可变供应对象；冻结不执行 native I/O，引用不是权限，Issuer 仍无私钥 opener。ExistingKeyLoader 固定 Microsoft Software KSP、当前用户、legacy spec=0、silent/open-only；加载及签名前 metadata 复验均沿原 owned key 的私有来源标记使用 SILENT，旧 Client/default 行为不变。没有枚举、持久创建、真实私钥导入/导出、属性修改或 provider/交互回退。

原 KeyCustodian 线程在首次 native 调用前保管空 partial slot，loader 只借用填充。失败或 panic 先纯撤销，再同线程显式按 key→provider 释放；Free 阻塞保持 Pending，失败保留原 slot/handle 与 Unknown/outstanding，不重试或 detach。已知拒绝、free 完成和原线程真 join 分别记录，普通 RAII Drop 不代替这些事实。

HostOpen11 的 named OpenKey 与部分失败由永久拦截的模拟 API 验证；010 和 NativeRemote018 使用公开匿名 CNG key 完成真实签名，018 另经原批准、activation、登记和隔离 Relay/TLS 握手及撤销/join。匿名导入不能证明实际已有用户 key 已成功打开，Unknown 子进程退出也不能证明进程内退休。最终同源码矩阵、产物哈希与历史失败分类见计划“已有密钥供应适配与原线程 partial 保管片”。

生产配置来源、native profile 安装、start IPC/UI 仍未接入，默认无配置仍 Locked；没有实际用户 key、真实 native dialog、手机或公网验收。两秒 mux 空闲和 Phone-first 行为保持，不能据本片宣称完整生产可用。

## Controller 保管的自主准备与登记

内部 prepare_owned 仍要求原严格 document/run/config owner 准入和原一次性 claim。唯一 DriverOwner 留在 controller，service 同 closing 锁登记 operation、独立持久观察事实及 Weak join Core；driver 不持自己的保管外壳，operation 不等待 driver。原 handle 先保管再放 gate，外部 claim clock 在原准备线程、准备真 join 后的登记在原 driver 线程。业务 Registered 不等于 key/driver 已退休。current 只读，不执行 clock、登记或 join；没有让可取消 IPC Future 承担保管。

退出或最后 Owner Drop 先纯撤销并共同推进原资源，仅 join 已结束的原线程。Controller 在原 ApprovalSlot full cancel/同步通知析构前先处理 DriverOwner，且不在 controller 锁内释放它。driver 内 panic payload 在原线程撤销后释放；若最终 join 仍返回未知 payload，则隔离保管并持续 Unknown/outstanding，不由唯一 async waiter 析构、不重试或 detach。既有 service source/closing 裁决没有复制。

HostDriver8 与 NativeRemote019 覆盖自主一次登记、原线程/尾门保管、失败/退出，以及原批准→Existing公开匿名CNG→真实隔离Relay/TLS→撤销/join。019还断言异源/重复请求无原批准副作用、到期通知前已撤销且controller锁可用、查询零clock增量。核心阻塞测试使用合成回调，不把它称为真实 GrantClock 阻塞验收；Unknown负向子进程退出不是退休。

最终证据按两版明确归属：冻结1完成广矩阵/default/client-only，原产物已保存副本；冻结2仅移动HostDriver002已有Pending采样块，其余源码不变，再跑严格Clippy、构建、Driver8与019。哈希、精确计数与差异证明见计划“Controller 保管的准备与登记驱动片”，不称完整矩阵由一个最终产物运行。生产配置/应用触发/start IPC/UI、实际用户key和真实本机对话框仍未接入或验收，默认无配置仍Locked。
