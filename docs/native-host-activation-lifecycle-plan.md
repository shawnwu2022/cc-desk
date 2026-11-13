# Native Host 单次启动保管链设计

状态：生命周期首片、2A 底层前置条件、2B 双密钥保管、批准绑定、单次阶段签名、已有密钥供应适配及 controller 保管的准备/登记驱动均已完成实现、定向验证与独立复核。生命周期首片核查基线为 `cbe7cae13b0b7f4c3291bf07decb26f996a9b230`，后续分片基线与证据分别记录在末节；本文不代表完整批准供应链已验收。

本片要证明：即使 Host 尚未注册网络或 pin，应用退出仍保管本次启动预留、原准备线程和所有已交付材料。生产默认配置继续为空。生命周期首片通过之前不增加生产 start IPC、UI 按钮或真实密钥打开调用。

现有行为见 [Native Host approval component](native-host-approval.md) 和 [Experimental native Host lifecycle](remote-host-lifecycle.md)。生命周期首片生产代码只保管生命周期；后续 2B 已增加独立两用途 custody、冻结 supplier 契约与 operation 保管接线，具体边界见末节。材料工厂、批准安装与启动调用仍仅由隔离 fixture 驱动，生产供应与 start IPC 未接入。

## 分片范围

1. 首片实现一个应用持有的 activation operation、必要的冻结绑定，以及原唯一退出 waiter 的观察。公开测试材料负责驱动该链路。没有生产材料供应者时继续 Locked。
2. 两用途工厂拆成 2A 底层前置条件和 2B Host 集成。2A 提供共享只可置位撤销事实、无回调停止与延后通知、同一 owner 锁下的完整退休观察。2B 再实现 Host 的 Endpoint、Relay 两用途签名工厂及保管链；复用底层 `KeyCustodian` 和签名 facade，不复用 `RemoteClientService` 的状态或生命周期。
3. 两用途工厂及其退出保管通过后，才接 strict document/run 的 start IPC 和原 registered network begin。
4. 实际配置、已有用户密钥打开、生产原生批准、手机与公网端到端分别验收，不由前三片的 fixture 结果替代。

Host 的 issuer 是验证对端 grant 的公钥。Host 不需要打开 Issuer 私钥，也不需要借用客户端 `NativeGrantPresence`。现有 `RemoteClientService::native_signers` 固定准备三个用途，不能直接作为 Host 工厂。

## 冻结绑定

`NativeConfiguration` 继续以原 `OwnerIdentity` 绑定实际文档、run 和 resource。配置描述只能由可信 native 调用方安装，不能来自 JavaScript 的批准布尔值、JSON 路径或原始句柄。

后续供应契约与同一份 `HostApprovalDescription` 一起冻结。需要保留固定 profile 身份或 revision、Endpoint 与 Relay 的公开 SPKI、用途和批准上限。实际工厂必须使用这份被冻结的 profile，不能在 Yes 后重新查找当前选择。

首片只加入保管所需的具体类型边界。若需要材料工厂契约，生产可以只有接口而没有实现或安装者。不能安装返回占位材料、默认批准或可用标记的假供应者。`cfg(test)` fixture 不代表真实供应者已经存在。

批准前只持有公开元数据、未启动的供应契约和弱 run/resource 身份。配置与批准回调不得持有已打开密钥、网络任务或原运行强引用。原 `ActivationTicket` 仍只能移动一次，先消费再准备。

## 单一 operation 与原线程

首片对应 `remote_host::activation` 的实际类型与阶段如下。

- `ActivationOperation` 由 `RemoteHostService.state` 中一个固定槽保管，拥有原准备线程及必要时原材料释放线程的 `JoinHandle` 和退休观察状态。该槽不会因取消、失败、join 或过期而清空重用。controller 只持弱引用，不能成为另一个启动清单。
- `ActivationSignal` 只持取消事实。准备线程持它，不持 operation 或 controller，也不升级 service。准备线程返回 `PreparationResult`，其中 `PreparedRegistration` 必须在原准备线程真 join 后由显式启动推进方法处理。`current`、`snapshot` 等查询不登记、不调用外部 clock。
- service 的预留、`closing`、`retired` 与 `complete_exit` 由同一个 `state` 锁裁决。原 waiter 持续观察这个槽，不接受 controller 的一次性快照来证明 service 没有 activation。
- 准备结果与退休事实分开记录。准备失败可以在清理与原 join 后得到已知退休；准备成功、材料 Drop 或一个完成通知都不能直接证明退休。

operation 的保管阶段区分 Reserved、Publishing、NotSpawned、Running、Joining、Prepared、Completing，以及 ReleasePublishing、ReleaseRunning、ReleaseJoining、ReleaseFailed 与 Joined。Unknown 是独立的粘滞失败事实，不能代替这些生命周期阶段。线程返回结果后、原 handle join 前，或准备结果仍未消费/释放时，均未退休。

### 预留与放行次序

1. 先通过原 strict document/run admission，确认原 slot 身份。尚无批准的调用不能消费其他配置或进入材料工厂。
2. 本地批准状态允许尝试后，释放 controller 锁，向同一 `RemoteHostService` 申请唯一 operation。在 service 的短锁内检查 `closing` 和原槽，安装预留。取消或到期在后续检查胜出时，预留转成不重用的终态。
3. 释放所有应用锁后再调用 `take_activation` 消费 ticket。该方法内部调用原 `GrantClock`，可能重入、阻塞或 panic，因此 service 预留必须早于它存在。此时没有线程也不表示已经退休。
4. 通过启动栅栏创建准备线程。原 `JoinHandle` 先安装进 operation，之后才允许线程执行工厂。不能先 spawn 自由运行的线程，再往 Vec 追加 handle。
5. 在工厂前再次检查取消、本机期限和原 resource。工厂只准备并归还材料。原准备线程真 join 后，显式启动推进才在锁外沿现有注册边界重验原 document/run/resource、身份、期限及撤销；查询不推进登记。关闭路径只释放，不登记。
6. 后续网络片只调用一次原 registered network begin，保留其原 attempt。首片不新增网络启动入口。

预留在调用者尚未创建线程时也必须可观察。同步预留 guard 负责记录调用者结束：已知未创建线程的失败记录 `NotSpawned`，unwind 记录 Unknown。`NotSpawned` 证明本次没有线程，不伪称 Joined。观察者不能根据当前 `JoinHandle` 为 None 推断不存在工作。

线程创建失败不返还 ticket，不再次尝试。spawn 已成功但 service 随后 closing 时，仍必须先把原 handle 放入该 service 的原 operation，再关闭启动栅栏并观察原 join。不能走拒绝分支直接 Drop `JoinHandle`。这次晚发布只完成保管，不重新获得执行资格。当前重复启动返回 Locked，不返回原 operation 的幂等观察，也不产生第二条准备链。

## 唯一退出 waiter

`RemoteHostService::request_exit` 在原 `state` 锁内设置 `closing`，从此拒绝新的 operation 预留。已存在的 Reserved、Publishing 和 Running 槽仍由该 service 保管。锁内只同步写入原 operation 的原子取消位使资格失效，锁外执行停止与释放；原 controller 的关闭继续撤销批准和未启动配置，但不能作为 service 退出安全性的唯一前提。

- Reserved 或 Publishing 保持 Pending，不能因为尚无 network attempt、source material 或 run pin 而跳过。
- 同一 service 已 closing 且从未预留 operation，才可以证明以后不会产生 activation。
- 首次预留已成功时，即使原 controller 被替换、另一个空 controller 被关闭或不再被调用，原 service 仍必须观察该预留。

原唯一 `start_exit`、`spawn_waiter` 直接持续观察 service 的固定槽，不增加 controller shutdown token 参数，不新建第二个退出 waiter。`state.retired` 和 `complete_exit` 在原 service 锁内同时检查 activation 终态与原材料门禁，不能仅信任前面一次观察结果。已有独立调用 `request_exit`、`start_exit` 的路径因此不能漏掉 operation。handoff maintenance 拒绝继续先于任何关闭动作。

退出必须先封闭新预留，再通知原 key、网络及准备 owner，最后观察原句柄。观察准备线程时，只提取已经完成的原 handle，释放锁后实际 join。join checkout 本身仍是 Pending，不能暴露一个被误读为退休的空槽。取消观察 future 不得丢失原 handle 或结果。

退出时 Prepared 材料先转入 operation 保管的独立 mailbox，再创建唯一释放线程。线程只持 mailbox 和启动 gate，不持 operation/service。原释放 handle 先入槽再放行；publication unwind 断开 gate 时仍继续释放，而不是沿准备取消逻辑直接返回。原 async waiter 只推进与观察 handle，不同步 Drop 材料，否则阻塞析构会饿死同一 `tokio::join!` 中的其他原 owner。只有 mailbox 已消费、析构未 panic、原释放线程真实 join 才能已知退休。spawn Err 或线程提前失败留下材料时保留 ReleaseFailed、Unknown 与 Pending，不自动重试，也不在 waiter 栈丢弃材料。

Unknown 是保留事实，不是完成信号。如果该失败 owner 自己或其他 owner 仍在运行，继续观察并保管它们。只有所有原 owner 已结束后才返回整体失败；任何 Unknown 都阻止 `state.retired` 和实际退出。业务拒绝、取消或已知准备失败与 Unknown 分开：资源确已释放并实际 join 时可以证明退休，但不能返还 ticket 或重试。保留原 first exit code、材料析构事实、网络 join、PTY 和 Job 的全部门禁。

`KeyCustodian::observe` 的 Unknown 可能仍持原 handle，不能据此停止观察，更不能照搬客户端 key 聚合的遇错立即返回。后续两用途工厂必须分别取得失败事实和仍在执行、原 handle 是否已 join 的事实。首片用受控 owner fixture 固定该聚合规则。

现有 native dialog 没有可 join handle。它仍只持弱状态，callback 返回不加入 key 或 run 的退休证明，本片也不声称能关闭它的独立线程。

## 锁与引用边界

- controller 锁只负责冻结配置、批准 slot 和本地关闭。向 service 申请预留之前先释放该锁。不在锁内调用 `GrantClock`、factory、service、通知回调、join 或可能持 native 资源的 Drop。
- operation 锁只负责原 handle 和阶段转换。需要调用外部代码时，先留下可观察的 checkout，再释放锁。
- service 的原锁负责 canonical operation 预留、关闭和最终退休发布。service 锁内不调用 controller 或外部代码。operation 观察与 join 在锁外完成，随后凭原槽身份在 service 锁内提交事实；checkout 期间保留 Pending。新增路径不得持 controller 或 operation 锁进入 service，再从 service 回调 controller。
- 准备线程只持取消 signal、本次工作必需的 admission 与 runtime，以及装入待完成结果的弱 service 身份。它不能升级 service 登记；登记只由线程 join 后的显式外部启动推进完成。完成后的闭包/材料释放也必须结束，才能清除本次强 run 引用并退休。
- service 持 operation，operation 持原 handle；准备线程持 signal，释放线程只持独立 mailbox。signal/mailbox 不持 handle、controller、operation 或强 service 引用。controller 只持弱引用，不新增 service-controller-operation 引用环，也不让 worker 最后释放自己的 JoinHandle owner。同步预留 guard 仅在调用者发布期间保管原 operation，不移入工作线程。
- 审查仍需检查每个失败路径与 Drop 顺序。单看没有 `Arc` 环不能证明 native 句柄已释放。

## 后续两用途工厂的保管要求

工厂必须把两个原 `KeyCustodian` 的保管交给 operation，然后才能放行各自的 `KeyStart`。材料与 key owner 不能分离为一个返回值和无主后台线程。

Endpoint 与 Relay 使用不同的固定 SPKI。批准期限来自原 ticket，单次 signer horizon 仍不超过现有五秒，不因准备慢、超时或取消续期。Host 不推进客户端 signing fence，也不调用客户端 shutdown 来回收 Host 密钥。

真正 loader 后续只能打开已明确授权的既有 key，固定 Microsoft Software KSP 和 silent/open-only 参数，不生成、导入、枚举或改写用户凭据。部分打开失败必须保管已经取得的原 provider/key，不能返回普通错误后丢弃它们。

现有 `NativeKeyProfile::open` 返回错误时，`KeyCustodian` 保守记录 Unknown。不能把这个错误直接改写成 NoKeyCreated。若将来区分已知未打开、已知清理完成与未知清理，必须提供对应的原 handle 退役证据。

本阶段不为 named-key 验证创建任何持久测试 key。测试只使用仓库公开 TEST_ONLY 向量、CNG 匿名临时 key 和 API 仿真。仿真可以核对 provider、参数和失败保管，不能证明真实 existing-key loader 已实测。实际读取用户 key 需要另行授权，缺少该验收时明确保留缺口。

## 2A：两用途工厂的底层前置条件

2A 的代码边界只涉及 `native_crypto_owner.rs`、`native_crypto_signers.rs` 的完整观察，以及相应测试。它不修改 controller、approval、activation 或 service，不新增 Host 工厂、真实 loader、start IPC 或 KeyInitialization/open-ready 协议。`prepare` 成功只证明原线程和能力已创建，不证明 key 已成功打开，更不能称为 Ready。

- `NativeRevocation` 是不可复位的共享事实；Host 可选的 `SigningFence::new_revocable` 与 `KeyCustodian::prepare_revocable` 接受同一 opaque token。原 attempt 的 admission、claim、native-return、delivery、publication 和 fence advance 都重新检查它。工厂失败、签名失败、释放失败及 panic 必须在外部 wake 前失效；原 request 的 unwind guard 先于 reply sender 析构，只持 token，不持自己的 owner。
- `Shared::prepare_cancel` 在锁内封闭资格并更新原事实，锁外只执行 bounded `try_send`，返回持有原 waker 的 `KeyShutdownNotice`。`notify` 才实际执行回调。原 Client 的 `cancel`/`shutdown` 继续同步执行两步，默认 fence 不绑定 Host token；没有改变 Client 的三用途状态或保管链。
- 两 key 的调用方可以先对两者执行 `prepare_shutdown`，再在受保管通知路径中消费 notice。第一个 wake 阻塞时，第二个 key 已失效且仍可由原 owner 真正 join。notice 的最后 Drop 也可能运行 RawWaker 代码，因此后续集成不能把未消费 notice 随意丢在唯一 async waiter 或应用锁内。2A 没有实现该应用级通知 worker。
- `KeyCustodian::observe_complete` 在同一次原 owner 独占观察中取得 retirement 和 `holds_original_join`，`NativeKeySlot::observe_complete` 路径持同一 owner 锁。Unknown 与原线程是否仍在运行独立；CNG Free 后原线程尾部尚未返回仍 Pending，不能用业务结果或空的瞬时快照伪造退休。

以下是 2A 收口时的后续要求；2B 独立保管片的实际进展见末节。完整接线仍须把原批准 lease 的 cancel/expiry、operation cancel、service close 和准备失败绑定到同一 token，覆盖先取消后晚绑定；被冻结的工厂不得在 Yes 后重新读取 profile binding。两原 key owner、signal handle、待通知 mailbox 和最终 slot 释放必须由 canonical 保管链覆盖，晚到 owner 必须接收、晚到 start gate 不得放行。两个 key 槽的 Unknown 不能短路另一个槽的真实 join。原准备线程尚未 join 时，即使两槽暂为空也不能宣布退休。

业务登记和资源 outstanding 将在 2B 分开：材料交付后原 key worker 仍活着，Registered 不是 Ready，也不意味着整个退出已退休。首片 operation 的 prepare/release Joined 不能代替 key、通知与最终释放线程的原 join。冻结工厂、key worker、通知/释放 worker 都不得持自己的 service/operation/custody 强引用。上述应用级整合仍待独立实现和验证。

## 首片测试计划

新增测试放在现有 Rust 测试体系。所有并发控制使用有界 channel 或 barrier，不用 sleep 制造顺序。保留真实两个线程的取消交错，不能仅用同步回调冒充并发。

1. **无资源退出**：直接关闭一个没有 operation 的 service，原唯一 waiter 可以继续；之后向该 service 的准备请求被拒绝。测试不依赖 controller 已关闭。
2. **无网络的预留**：预留完成后、消费 ticket 的外部 clock 返回前暂停调用者。没有网络或 pin 时，退出仍 Pending。释放屏障后验证原工作结束，不允许提前退出。
3. **发布窗口**：在 spawn 前、handle 安装前和 start gate 放行前分别关闭 service。工厂调用数为零，spawn 已成功时 closing 分支仍保管并 join 原 handle，重复调用不创建第二个线程。spawn Err 只记录明确未创建线程，不计 Joined。
4. **阻塞工厂**：取消立即撤销批准，但退出等待原工厂返回及原线程 join。晚到材料不能注册或进入网络。
5. **材料拒绝**：错误身份、已撤销的 resource、过期及 clock 错误返回原材料给原 owner。材料正常释放后才能完成相应事实，析构 panic 为 Unknown。
6. **panic 与创建失败**：工厂 panic、预留 unwind 和线程创建失败分别验证固定结果；Unknown 不变为已知成功，ticket 不返还。
7. **重复与并发**：两个 start 尝试争用唯一槽，最多一个实际工厂。cancel 与 close 不产生补充配置或自动重连。
8. **原 join**：工作结果已发布但线程尚未结束时仍 Pending。实际 join 后才能发布退休。取消一个观察 future 后，另一个观察者仍消费同一原 handle。
9. **退出聚合**：activation Unknown 自身仍运行，以及另一个 owner 仍运行时，原 waiter 都继续观察，不能提前释放保管。直接调用 service 退出路径、替换或关闭无关空 controller 都不能绕过原 operation。首次退出码和唯一 waiter 断言保持。
10. **锁与释放**：工厂、clock 与材料 Drop 重入 cancel/current/退出时不死锁。全部原操作结束并释放应用 owner 后，用弱引用与 Drop 计数证明没有新增 controller-operation-service 引用环。
11. **原回归**：保持所有 HostApproval、Host controller、材料释放、checkout、source clock 及 NativeRemote 测试。Registered 仍只表示历史注册，current 不采样原 GrantClock。

两用途工厂片另外验证 Endpoint/Relay 签名与跨角色拒绝、Issuer open 计数为零、部分 key 准备失败的原 join，以及 Host 取消不影响客户端请求。start IPC 片另外验证 feature-off、strict DTO、重复字段和 document bridge，无配置仍 Locked。

## 执行与验收边界

- 所有源码、证据及 `TEMP`、`TMP`、`TMPDIR` 都在项目内。使用同一个 `target-browser-profile`，`CARGO_INCREMENTAL=0`。运行前与运行期间检查 E 盘空间，低于 3 GiB 停止，不清理未授权目录。
- Windows 直接 Cargo 前运行 `node scripts/prepare-conpty.mjs`。Cargo 固定 `+1.98.1 --offline --locked`。如需 SDK 资源，沿用已有 prepare-session-sdk 流程。
- 先编译并记录本次 Cargo 返回的精确 lib test executable，再跑定向测试。全局 Host admission 不可 reset，NativeRemote 每个测试使用 fresh process 和 `--exact`。
- 复用 `scripts/verify-remote-host-isolated.ps1`。新增库存必须显式更新预期，不删原断言、不降低原数量、不用历史 binary 替代编译失败。
- 按变更范围执行 Rust 格式、相关测试、remote-host 构建及适用的严格 Clippy。测试通过、构建通过、未运行和环境阻塞分别记录。
- 首片不触碰前端 API，因此不宣称 start IPC、真实 native dialog、生产 key/profile 或手机链路已验收。原 Phone-first Offline 失败继续独立保留，不以 sleep、buffer 或 Host-first fixture 替代任意顺序会合。

## 实施与审查记录

首片最终冻结源码已通过独立复核。证据保存在项目内 `.audit/host-activation-lifecycle-20261113/`，不得把不同源码快照的结果合并成一次最终通过。

- `red-build.*` 是初始缺接口的编译 RED，不是行为复现。
- `selfjoin-red.*` 是原 worker 最后释放 service 导致自 join 的行为 RED：exit 101、3.02 秒有界断言失败，非外部超时。两阶段修复后 `green2-activation.*` 的 16 项 operation/service 测试通过。
- `green2-composition008-exact.*` 精确运行原 008，1 项通过。较早无 `-exact` 后缀日志选错完整名而执行 0 项，明确不计验收。
- `release-red.*` 是唯一 waiter 同步材料析构阻塞其他原 owner 的组合行为 RED：017 单项失败，exit 101、3.01 秒。测试先释放材料并回收所有原 owner 后才断言失败，没有遗留被测试线程。
- `green3-activation.*` 证明独立释放线程修复后 20 项通过；随后范围格式化错误将 017 的 release 语句拼入注释。`format-regression017.*` 真实记录该快照 exit 101、3.02 秒、Elapsed 失败。此错误用 Node 直接 UTF-8 修复，未改生产语义；`final-isolated/` 的较早结果不计最终验收。
- 最终源码清单为 `frozen2-source-hashes.json`，精确产物 SHA256 为 `E32767C3335D9C122B2AF23ADC0CCE414DBEE4C2B8DBC4CEEDFD6DCC35629DC9`。`final2-activation.*` 实跑 21 项全通过，包含 017 材料阻塞时原 native owner 仍真 join、018 release spawnErr 保留材料、019 材料已释放但原线程尾部仍 Pending、020 publication unwind、021 take 前 panic 后仍保留 mailbox 与 Pending/Unknown。
- 同一产物的 `final2-approval.*` 6 项、`final2-service.*` 14 项、`final2-query.*` 1 项均通过。`final2-isolated/results.json` 记录 NativeRemote 001–010 每项 fresh process、005 额外重复 5 次、network 23 项、checkout 3 项全部 exit 0，库存计数正确且无 timeout；部分 service/checkout/activation 库存有重叠，不合计成独立测试总数。008 包含真实 clock 阻塞且无 network/pin 的预留退出、重复启动拒绝、profile 材料不符、迟到过期及 current 不调用 clock/登记。
- `final-bridge.*` 记录现有前端 document bridge 的 4 项定向测试通过。`final2-fmt.*` 与 `final2-clippy.*` 分别记录新模块/controller/approval 只读格式检查和 `cargo +1.98.1 clippy --features remote-host --lib --offline --locked -- -D warnings` 通过。service 只格式化新增范围并直接核对 UTF-8 注释及相邻语句，未重排旧代码。
- 首次 Clippy 仅发现本片未接入生产调用者的 `Completing` 与 `finish_preparation` dead-code；现按具体接口注明未安装调用者原因，没有修无关基线代码。未重新构建 feature-off harness，其 commands/IPC/前端生产代码本片未改；未运行全量 Rust、Clippy tests、安装包 WebView2、真实 native dialog、真实 key/profile 或手机公网验收。生产仍 None/Locked，没有新增 start IPC 或真实凭据操作。

- 确认 reservation 必须早于外部 clock，且同步调用者尚未返回时仍在退出清单中。
- 确认 reservation、closing、retired 与 complete_exit 都由同一 service 锁裁决，所有退出路径持续观察原槽，不依赖 controller 快照。
- 确认所有原 handle 在工厂执行前入槽，join checkout 不会暴露假空槽。
- 确认 Unknown 不短路其他原 owner 的等待，Drop 不触发同线程自 join。
- 确认冻结配置不被当作真实供应者，首片无生产 start 与实际 key 操作。

## 2A 实测记录与剩余边界

2A 基线为首片提交 `cbd8e4b43ceb5bc1fb9bac36c99f8d05765265fe`。证据保存在 `.audit/native-crypto-host-prerequisites-20261113/`；最终 Rust 清单为 `frozen2-source-hashes.json`，最终八文件清单为 `final8-manifest.json`。最后一次源码修改只将新测试 helper 的 `join().ok().expect` 改为直接 `join().expect`，之后重新构建和验证，没有把前一产物结果套到新源码。

- `red-build.*` 成功编译原接口上的行为测试；`red-deferred-stop.*` 以 exit 101、1.01 秒复现顺序同步 shutdown 在第一个 wake 阻塞时无法停止并 join 第二个 key。该 RED 说明原调用组合不适合 Host 的唯一 async waiter，不声称原 Client 同步接口有生产故障。退出断言前已释放测试 gate 并 join 两个原线程。
- `check1-host.*` 首版 6 项中 5 通过、1 失败：原 request unwind 先销毁 reply sender，外部 wake 当时还看不到撤销事实。新增只持 token 的 request unwind guard 修复此顺序；它不替代原 JoinHandle 或 Unknown 事实。
- `final2-build.*` 是调用目录错误导致离线 aliyun 索引找不到 `coset` 的环境失败，不是行为 RED。恢复在工作树根目录使用 `--manifest-path src-tauri/Cargo.toml` 后，直接复用既有 crates.io 缓存、同一 target 和 `CARGO_INCREMENTAL=0`；没有改配置或下载依赖。
- `final3-artifact-hash.json` 的精确 test executable SHA256 为 `5D5897AAE5F2409FFF2623BC083FB1A3ABDEC277E14E566148D2A7D76F5D93D7`。`final3-host-prerequisites.*` 新增 7 项、`final3-native-crypto.*` 23 项、`final3-client.*` 25 项、`final3-activation.*` 21 项、`final3-approval.*` 6 项均 exit 0、0 failed、0 ignored。新增 7 项包含在 native crypto 23 项内，不重复计数；完整名称见 `final3-inventory.txt`。
- 新增 001 验证两 key 先无回调停止，首个通知阻塞仍能 join 兄弟 key；002 覆盖原五处签名检查及 fence advance；003 实测已 Free 但线程尾部受 gate 持有时的 Pending/Unknown 与原 join 独立；004 覆盖 gate 前取消、factory Err/panic 及非法元数据；005 用匿名 RFC6979 CNG 签名验证 Client 独立；006 验证 panic 在 sender/waker 析构前撤销；007 在有界独立子进程用固定假句柄和永久 free 拦截验证 Unknown 仍持原 worker，不触碰真实 CNG 对象。007 的进程退出不是 key Free 或原 join 成功凭据。
- `final3-fmt.*` 是六个改动 Rust 文件的只读格式检查；`final3-clippy.*` 是 remote-host `--lib -- -D warnings` 严格检查，两者通过。严格 `--lib --tests` 仍受已提交的 `native_cli_remote_composition.rs` 三处 lint 阻断：978/1008 的 `possible_missing_else` 来自首片，1759 的 `chunks_exact_to_as_chunks` 是更早基线。本片 helper lint 已修，不修改或屏蔽上述范围外代码；不能称全目标 Clippy 通过。

2A 未运行全量 Rust、feature-off 新 harness、NativeRemote 全套重跑、真实 existing-key loader、真实原生批准、手机或公网端到端。controller/approval/activation/service/IPC 未改，保留首片原隔离证据。本片不创建或打开持久/真实用户 key，不增加生产 Host profile、两用途供应者、通知/释放 custody 或 start 调用者。生产仍为 None/Locked，后续 2B 必须单独验收上述组合边界。

## 2A 后续独立测试 lint 收口

以 `d1a32989e623f1d812b1ebca6ee0b4a753506f19` 为基线，独立修复上述三处 composition 测试 lint：两段等待循环仅换行，保留原先相互独立的 `if`；`chunks_exact(2)` 改为 `as_chunks::<2>().0.iter()`，仍仅遍历完整两字节块并忽略余字节，兼容声明的 Rust 1.89 MSRV。没有 lint allow、整文件重排或生产逻辑变更，中文注释原文保持不变。

证据为 `.audit/remote-composition-lint-20261113/`。`clippy.*` 记录同源码 remote-host 严格 `--lib --tests -- -D warnings` exit 0，关闭三处 lint；前述 2A 历史失败记录继续保留。原隔离脚本的 `isolated/results.json` 记录 NativeRemote 001–010 各 fresh process、005 额外一次、network 23 项及 checkout 3 项全部 exit 0、无 timeout、测试摘要计数正确。本次实际 Cargo test artifact SHA256 为 `5F13099C1A30DCB9E787180B1D7C52AF183D07EE6923E6958B5EBF82F3E90E82`，没有沿用 2A 旧产物。仅收口测试 lint，不提前实现 2B 或扩大生产验收范围。

## 2B 独立双密钥保管片

基线为 `23f1077fdd01046ed51b8bb6ccc04027f7b31b9e`。本片只实现 core custody、冻结两用途 supplier 契约及原 operation 的实际保管接线，不安装生产 supplier、不修改 controller/approval，也没有 start IPC。测试显式传入同一 native token，不等于原 ApprovalLease 已完成绑定。

### 固定所有权与推进

- 唯一 `ActivationReservation` 在原 service 已保管的 Reserved operation 中先安装无线程 custody，才向锁外调用栈发出 notification builder。终态 operation 不能新增线程或 key 责任。通知原 handle 先入 canonical 槽，再允许唯一 publisher 发出；真实 spawn 后的 publication unwind 不能记成 NoWorker。
- operation 持有 custody 和原通知 handle；Core 只保管两个私有 Box slot、原 signal keeper、notice 队列及冻结 opener mailbox。prepare worker 只持 publisher/Core 或 signal/token，不持 operation/service/custody。通知 worker 只持 Core，没有自己的 JoinHandle owner，因而没有反向强引用或自 join 环。
- Core 锁内借用固定 slot，观察并复制 retirement 与 original-join 事实；观察者不取得 slot/keeper 的 Arc。关闭先对两个 key 无外部回调地失效并收集 notice。唯一通知原线程在锁外 wake，并在两原 key 真 join 且 publisher 封闭后取走实际 slot、keeper 和未消费 opener，锁外 Drop 正常完成后才写 disposal_complete。
- 通知可能阻塞第一个 wake，唯一 exit waiter 和最后 operation Drop 仍能独立推进另一个 finished key 的原 join。最后 owner 不先 blocking join prepare/notify，而共同推进所有 finished 原句柄；publisher_closed 从不替代 prepare Joined。通知线程自身也能 join key，无需外部 observer 先写入 prepare 完成。
- 通知 handle 的 checkout 保持 Pending。完整 custody snapshot 按 notify→Core 同一锁序采样，不把旧 unknown=false 和后来空 handle 拼接成已退休。通知 panic 后，即使原通知已 join、mailbox/slots 已取空，未正常完成的通知或析构责任仍 Unknown+Pending；不重试、不 detach，也不把退出负向测试进程当成退休。
- operation signal 一次绑定 opaque 只可置位 token。取消先于晚绑定时，新 token 立即失效。未 settled reservation 放弃、prepare spawnErr、Complete(false)、prepare panic、登记 false/panic 都走相同纯撤销；worker 只持 signal 的 unwind guard，观察者幂等兜底。成功 fixture completion 后，cfg(test) result helper 可观察 true，而原 key/通知资源继续 outstanding；实际 finish_preparation 仍返回资源观察，不表示生产 controller Registered/start 已打通。

### 冻结供应与失败封闭

`FrozenHostSupplier` 将 profile 身份、revision、Endpoint/Relay 两个不同公开 SPKI、relay origin/id、原本机 Instant cap 与两个私有一次性 opener 绑定为同一移动对象。没有 Issuer opener，没有可变 profile 引用或重新采样入口。生产可信构造尚未安装，测试构造仅使用公开 TEST_ONLY 标量及匿名 CNG key。

工厂在任何 opener 运行前验证非空身份、revision、两个 P256 SPKI 的现有格式合同、SPKI 不同和 relay adapter 的公开 bounds。每个 key 原 handle 先存入固定 slot 再放 KeyStart；取消后晚到 owner 仍接收，但 gate 不放行。原 worker 开键前再次检查同一 token 和原 cap，不延长时限。prepare 返回仅证明原线程和限定 facade 已保管，不证明 key 已打开或 Ready；可用性另用真实匿名 CNG Endpoint/Relay 签名验证。

第二个准备创建失败仍保管第一个 owner。工厂/native/free 失败由 2A 原 worker 先撤销 token，Core 通知循环无需用户退出便关闭兄弟 key。到期只读取原本机 Instant，不在 Core/通知线程调用外部 GrantClock。业务 Registered 与资源 outstanding 必须分离，本片以 fixture completion/result helper 验证该语义；current/snapshot 不执行登记或调用外部 clock，生产 controller 接线仍未完成。

### 行为证据与验收范围

证据目录为 `.audit/host-key-custody-20261113/`，所有历史失败保留：

- `red-three-gate.*`：原 operation 最后 Drop 先等 prepare，导致兄弟原 key 无人 join，exit 101。测试实际关住 prepare 尾部与首个 wake，只放第二 key 的 after_free 尾部，主线程仅查询复制事实；先打开全部 gate 并 join fixture 再断言。
- `late-red.*`：旧首次 attach 允许终态 operation 新增 custody，exit 101。修复为 reservation 先保管无线程 custody 再创建通知。
- `snapshot-red.*`：旧分锁采样把先前成功与后来已 join 的通知拼接，exit 101。修复为一致 notify→Core 采样与独立 joining 事实。
- `abandon-red.*` 两项和 `outcome-red.*` 参数化测试实际复现放弃、spawnErr、prepare false/panic、登记 false/panic 未撤销 token，均 exit 101；检查发生在任何 retire/drain 之前，不能由清理阶段的 cancel 掩盖。`green4.*` 中 010–012 已验证修复，成功结果对照保留活 key。
- `green2-build.*` 仅是新增 SPKI 校验缺少借用的编译失败，不是生产行为 RED。`check2-existing.*` 的 006 是 loopback 测试误用证书域名 localhost；`green4.*` 006 的实际签名已通过，但取消后 choose_scheme 返回 None 被测试错误 unwrap。修正测试使用原 fixture 的 foobar.com，并同时断言旧 signer 拒绝签名、新 choose_scheme 为 None，不减弱撤销要求。

最终七个 Rust 文件冻结清单为 `frozen-source-hashes.json`，九个源码/文档文件清单为 `final9-manifest.json`。固定 core 和最终 15 项测试经独立只读复核，没有新增源码阻断。

- `green5.*` 的 14/15 通过保留为历史结果；015 首版把签名拒绝错误等同资源 Unknown，实际拒绝、token 撤销与原 key 正常 free/join 均已完成。最终测试分别断言拒绝及已知资源退休，未改变生产逻辑，也未削弱既有回归断言。
- 最终 remote-host artifact SHA256 为 `411EBAB9472319E2B364CA1A03CA76E54376B76AC723DBB7ED5D1B6B38285897`，来自 `final-isolated/selected-artifact.json`。同一产物 `final-groups-results.json` 实跑 HostKeys 15、native crypto 38、Client 25、HostActivation 21、HostApproval 6、service 14、strict query 1，全部 exit 0、无 ignored/timeout；HostKeys 包含在 crypto 内，activation/service/checkout 也有交叉，不合并成去重总数。
- 新 013 区分明确未创建通知线程与已 spawn 后 publication unwind；014 用三个有界独立子进程分支分别验证通知回调 panic、已取出但未正常完成的析构、合成 free Unknown 都不伪造退休。free 分支的兄弟 join 也可能由通知原线程自主完成，不单独证明 fallback 亲自 join；实际最后 operation Drop 共推进由 001 的三 gate 及通知失败后的观察组合验证。015 用实际匿名 key 验证原 native purpose 不符时拒绝、撤销及无需退出的资源收口。
- `final-isolated/results.json` 包含 NativeRemote 001–010 各 fresh process、005 额外五次、network 23 和 checkout 3，共 17 个原进程、41 次测试执行；全部 exit 0、计数正确、无 timeout。008 保留原 clock 预留组合、错误 document/run、迟到取消/过期与只读 current 合同。
- `clippy1.*` 记录最终 Rust 源码 remote-host 严格 `--lib --tests -- -D warnings` exit 0；`final-fmt.*` 记录七文件只读格式检查通过。没有扩大 lint allow 或重排旧 service/composition。
- `default-build.*` 实际重建同源码默认 feature-off harness，feature 仅 default/rusqlite/sqlite。`default-result.json` 的原 native crypto 11 项全通过；其独立 artifact SHA256 为 `50ED00058F9DD135E873A1D80999407A978B35BCC024C3415DB13A6A759837AB`。它不是 remote-host artifact，不混用结果。两份文档及七个 Rust 文件均通过直接 UTF-8 字节解码与替换字符检查。

本片没有运行全量 Rust、真实 GUI/native dialog、持久 existing-key loader、手机或公网端到端，也不声称完整批准供应闭环或真实用户凭据验收。下一片必须将 ApprovalLease cancel 的 token 撤销放在任何 watch 通知之前，并定义 take_activation 失败及原 ApprovalClock expiry/error 的主动撤销；不能依赖 UI poll。仍需审查冻结公开描述与原 lease 一次性绑定，完成 strict document/run 的实际启动推进。既有 Phone-first Offline 与任意顺序会合缺口不由匿名 fixture 结果替代。

## 批准与冻结供应绑定片

基线为 0b16a2f8fcb292289817b45e1be897a6117cc1c0。本片实现及同源码矩阵已完成，冻结核心与末次发布门禁经独立只读审查。没有生产供应安装、真实 key loader、start IPC/UI 或部署；默认无配置仍 Locked。前述 2B 的未完成项是该历史提交的边界，本节记录本次实际推进。

### 实际类型与一次性顺序

- ApprovalLease 创建时唯一生成 NativeRevocation，cancel 先置位再发原 watch/source 通知。原 operation signal、SigningFence、两个原 key 及 RegisteredHostSource 使用同一 token。先撤销后绑定不能恢复资格，token 不是新凭据。
- NativeConfiguration 保存一个 move-only FrozenHostApproval，公开描述、弱原 run 身份、原 cap 和 FrozenHostSupplier 一起消费。HostKeyProfileIdentity 明确属于原生签名配置域，不能代入 CLI Profile 或 WebAuthn BrowserProfile 的身份/修订。两个不同 SPKI、Endpoint Host id/role、Relay Host id/origin、原 policy.scope 与 admission.scope 在任何 opener 执行前核对。供应对象没有 Issuer opener，也没有可变 profile 仓库引用。
- 严格 document/run/config owner 门禁先于原 service reservation；随后本地 claim 唯一取走原 MachinePermit、bundle 和 approved_end，不调用外部 clock。取得 claim 后才安装原 token custody/通知线程，之后锁外验证 clock 并准备。重复 Consumed 和外国请求在 claim 前返回，不新建通知 owner、不消耗或撤销合法批准。
- 未消费批准每次使用检查原期限；claim/ticket 原样携带原 Instant end；活 key 的 Core cap 只能缩短。没有新计时线程。已取得 claim/ticket 的失败、owner 丢失、clock Err/expiry/panic 先纯撤销；准备/材料释放线程不执行会升级 Weak service 的完整 lease.cancel。ApprovalClock 保留 panic 传播，原 SourceClock 继续记录 Unknown。
- prepare worker 只持原一次性 ticket、publisher/Core/signal、必要原运行引用和 Weak service；登记在准备原线程真 join 后由显式推进完成。查询无登记/外部 clock 副作用。成功业务 Registered 和资源 outstanding 独立，原退出保管链不因业务成功而跳过 key/通知/释放原 handle。
- 私有 NativeApprovalStop 只持原 token/end，无 lease、service、run 或回调。它原样进入 SourceClock、已登记源后续准入/补充、原 setup/established select 及最终 service commit 门禁。SourceClock 的普通错误、回退、host-ms 到期、重入和 panic 都纯撤销，保留原错误/Unknown 分类；网络等待只用原任务内的有界 timer。is_closed 的新纯活性检查只供资格门禁，不冒充槽材料或原资源已释放。

### RED 与最终证据

证据目录为 .audit/host-approval-key-binding-20261113/，历史结果全部保留。

- clock-red.*：实际行为 RED，源 clock 首次 Err 后同 lease 仍可在新 wrapper 返回 Ok；exit 101。修复为 lease failure guard 的只可置位撤销，且先于 watch/waker。
- network-red.* 的 011 首次运行实际上通过，原因是原 Channel 路径关闭，不将它冒称失败或缺陷复现。随后加入撤销前 100ms 原通道保持 Pending 的观察，并用同源码实际 cause=Approval 验证显式 stop 分支；不靠延长原 grant 或删除生产检查制造 RED。
- value-red.*：SourceClock 对 999 回退与 11000 到期均已拒绝授权，但 token 仍 false，exit 101。最终同源码两路径 token=true、watch=false、Unknown=false。它是新批准绑定中的失败传播缺口，不声称旧网络仍可授权。
- commit-red.*：008 在仅纯撤销、尚未 full cancel 时，原末次 commit 返回 Ok 并发布 Some(Ok)，exit 101/0.96s。修复仅令原 source 槽资格门禁读取原 registration 纯活性；最终 commit=Err(Closed)、publication=None。008 同时保留外国请求拒绝，并新增原 token 仍 live、正确原 admission 可继续 claim/validate 的断言。
- combination1-build.* 的 Endpoint Role 枚举名错误、clippy1.* 的旧 SourceClock 测试 literal 缺字段都是编译问题，不是行为 RED。没有放宽断言或添加 lint allow 来掩盖失败。

最终 Rust/脚本冻结清单是 frozen2-source-hashes.json，完整十五文件清单是 final15-manifest.json。最终 remote-host test artifact SHA256 为 67FAE4BED147EAF281CE630EE1D33A17942635D7F41F73A5F1FCF795F000CE91，来源为 final2-isolated/selected-artifact.json，不沿用早期产物结果。

- final2-isolated/results.json：NativeRemote 001–013 各 fresh process，005 额外五次，network 25、checkout 3，共 20 个原进程、46 次测试执行；全部 exit 0，无 timeout，摘要计数正确。原 006 panic→Unknown、原网络/PTY/Job/退出和 008 clock 预留边界保持通过。
- 011 验证纯 token 撤销、watch 不通知时空闲已建立通道沿原 stop+join 退出；012 验证五类冻结描述错配零 open、严格 controller/原 operation 准备、显式 Registered 同时 keys outstanding、两公开匿名 CNG opener 各一次及立即 Relay/TLS 握手；013 覆盖九类批准/claim 失败，均在任何清理前检查撤销/零 open，并实测原一秒 cap 下两个匿名 key 已打开后无需 UI/exit/watch 自主失效与最终退休。Issuer opener 在类型中不存在，不是打开真实 Issuer key 的测试。
- final2-groups-results.json：HostKeys 15、native crypto 38、Client 25、HostActivation 21、HostApproval 11、service 14、strict query 1 全部通过。HostKeys 包含在 crypto 内，Approval 中两个新源/stop 测试也包含在 network 25 内，其他旧分组亦有交叉，不合并成去重总数。
- clippy3.*：冻结2源码 remote-host 严格 --lib --tests -- -D warnings exit 0。final2-fmt.* 是八个本片适合整文件检查的 Rust 模块只读格式验证；旧 compact network/composition/crypto 基线未做无关整文件重排。全部十五文件另做直接 UTF-8 字节及 diff-check。
- default2-*：同源码默认 feature-off Cargo 调用选择独立原 artifact，native crypto 11 项通过，无 timeout；其 SHA256 为 67D068AF1C7A9BF494E82F133DBBFD296D273C416861263C18E31ED7A2C7D232。没有混用 remote-host 产物。底层改动在 feature-off 中仅增加受 cfg 限定的测试 helper，原行为保持。

### 仍未完成的生产验收

没有运行全量 Rust、真实 GUI/native dialog、已有用户 key loader、手机或公网端到端。原批准到双用途供应保管链已由公开匿名 fixture 验证，生产安装/配置和真正启动调用者仍未接入。该片提交时，prepare 将两种 facade 的同次 attempt 限为最多五秒；Relay 建立、会合及晚 ClientHello 可能超过此窗口，当时不能据立即握手通过声称等待 Phone 后仍可用。后续实际修复与保留边界见下一节“原网络任务的单次阶段签名片”。

该片结束时规划的下一独立片，是在原 network/stage 独占的一次性 permit 中，于实际签名时领取不超过五秒的 attempt，Relay 和 Endpoint 分开领取，仍受同一原 key/fence/token/cap；其实现和验收记录现见下节。LocalIdentity::new 会提前探测 choose_scheme，不能在那里消耗 permit；不得延长期限、重开 key、失败后续领或自动续期。原 Phone-first Offline 与任意顺序会合缺口继续保留，不用 sleep/buffer 掩盖。

## 原网络任务的单次阶段签名片

基线 3e88ddf7a9e742f6a12e5982fe5b4713cce02c62。本片仅移动原两 key 的签名资格签发时点，没有增加批准、真实凭据或生产启动入口。完整最终验证记录在本节收口部分。

### 实际所有权与失败边界

- FrozenHostSupplier 的同一准备流程产生 HostSigningStages、Endpoint/Relay 惰性 facade；原 HostKeys 测试也走此流程，没有完整 eager 工厂旁路。个别 custody/factory 专项仅用明确 cfg(test) 的 Endpoint-only driver 隔离故障，它不证明生产 Relay 顺序。
- 阶段控制对象不可 Clone，随原 fresh material、原 NetworkStart 唯一移动到已保管网络任务；共享 facade 只有短锁下的占用/交付事实，没有 arm 权。NetworkSigningScope 的构造限制在原网络模块，并携原 registration 的 token 身份、原批准 cap、原 setup end。同一公开 key 不是同一批准身份；注册、补充和 bind 拒绝异源 staged 材料，B 资格与合法 fresh 槽不受 A 拒绝/Drop 影响。
- Relay sign 调用原子消费一次，首次 poll 才 mint；Endpoint 在原 connect_admitted 成功返回后 arm，实际 sign 才消费/mint，choose_scheme 不消费。attempt.end 不晚于 now+5 秒、原 cap、原 setup end。Relay 交付及 Accepted 先于 Endpoint advance，同 fence 不覆盖尚未交付的前阶段；旧 Relay attempt 在等待后到期，不阻止原 setup/cap 内的新 Endpoint 阶段。
- Bounds/Busy/超时/取消/panic 不回填许可、不重开 key。未 poll 与已排队 Relay Future 的 Drop 先 pure revoke；其手写 poll 持纯 token unwind guard，原票据保存在 Future 字段中，避免 opaque async 内先析构票据再撤销。共享阶段锁从不包外部回调或资源释放。正常原网络退出先 revoke，再按原 stop+join 退出；Client 的默认 None 路径与原 key 退休事实不变；下述 owner 窄修只补失败撤销和外部回调的顺序。
- 原网络任务没有新增 abort 入口，原 JoinHandle 持续保管，work panic 通过 guarded 返回后显式 cancel。该事实不等于证明任意强制 Drop 整个 runtime Future 的局部析构顺序，更不代表进程异常退休。

### RED、测试含义与验收边界

证据目录 .audit/host-signing-stages-20261113/。

- delay-red-014.* 是真实行为 RED：准备/登记后等待超过旧五秒窗，再开始原网络，旧 Relay 签名拒绝；exit 101。green3-014.* 在同样延迟和原预算下握手通过，两个匿名 opener 各一次，原 network 已知 join。
- delay-red-015.* 与 green1-015.* 的早期测试是混合失败：Host 已启动 mux，2 秒无 Binary 的原 IO idle timeout 先关闭它，不能将其冒称纯 Endpoint 签名 RED。原日志保留，没有提高 IO/setup 时限或弱化生产检查。
- 修正 015 使用仅测试的原任务门，位置在真实 Relay Accepted 后、Endpoint arm 和 mux 创建前；等 5.2 秒才放行并发送真实 ClientHello，原 8 秒 setup、2 秒 IO 不变。green4-015.* 已通过。这同时检验旧 Relay attempt 到期后仍可 arm，以及临近实际 Endpoint sign 才 mint；不证明已启动 mux 可空闲等待 Phone 超过 2 秒，也不解决历史 Phone-first Offline。
- green4 的 HostStages 001–005 覆盖重复 choose_scheme 零 mint、并发八个已得 Signer 仅一次成功/mint、未 poll Drop、真实 Queued 票据及 RawWaker clone panic、撤销先于阻塞 wake且兄弟原 key 真 join、公开 bounds/原 setup 到期与 token/cap 替换拒绝。新 006 补 Busy 与原五秒排队超时不续领。
- NativeRemote012 增加 A 材料进入 B 的 Authority 拒绝、B token/fresh 保持、A 退回仍 live而 Drop 后仅 A 撤销，并继续原 B 真实握手。016/017 分别让 Accepted 门等到原 8 秒 setup 结束、让门取消，确认失败时原 key/network 收口，不因阶段存在延长时间或漏原 owner。
- green2-build.* 的四个旧测试 gate 类型调用点、clippy1.* 的测试 helper cfg 与类型布局/复杂度问题都是编译/检查问题，不计为行为 RED，不靠 lint allow 掩盖。

### 到期失败通知的独立复核与补丁

冻结1的完整隔离矩阵与八个分组均通过，但独立审查发现这些测试未覆盖三个已知失败点中的阻塞回调顺序，因此冻结1不能替代修复后验收。

- expiry-red.* 的007在实际五秒到期后 poll Queued 票据，原 cancel_observer 先同步 wake，撤销仍 false，exit101/5.03秒。
- expiry2-red-008.* 让原 worker 先发现到期，observer 尚未再次 poll；reply通知前撤销仍 false，exit101/5.04秒。
- expiry2-red-009.* 用原 hook_delivery 在交付时过期，原 clear_notification 先析构旧 waker，撤销仍 false，exit101/5.02秒。三例都先放开门、等待原资源收口，再断言失败，不把悬挂或进程退出当退休。
- 修复仅在原 id 匹配且取消/拒绝/交付失败事实已明确时，先置既有可选 NativeRevocation，再执行外部 wake/析构。原 worker native return 已有正确顺序，保持不动。另在 SignTicket::poll 入口、任何 waker 回调前对当时已知失效的匹配 Host 请求纯撤销；它不替代前三处，也不声称能抢占运行中的外部回调或完全消除跨 deadline 竞态。原 RequestFact/closing、Client None、正常成功和 stale id 都保留。
- 最终新增010覆盖入口已过期时的旧 waker析构，011覆盖原已交付票据的 stale cancel 不撤销新合法请求。HostStages总计11项，均使用公开匿名key和原票据，没有新增生产凭据操作。

### 最终同源码收口

冻结2源码及核心独立只读审查完成，实际最终矩阵全部通过。源码/脚本清单 frozen2-source-hashes.json 共11项，完整十三文件清单 final13-manifest.json；验收汇总 acceptance-summary.json。本节记录提交前的实际最终产物验证，不将早期结果替代最终验收。

- 最终 remote-host artifact SHA256 为 285EC05A342BC9582C1285ACE7B601429EDB07F87B8FECE5D01DCEAF8C8F2463，来自 final2-isolated/selected-artifact.json。同源码严格 Clippy3（--lib --tests -- -D warnings）exit0；final2-fmt 对八个适合整文件检查的 Rust 模块通过，旧 compact network/composition 不做无关重排；最终文件直接 UTF-8 字节检查和 diff-check 通过。
- final2-isolated/results.json 实跑 NativeRemote001–017 各 fresh process、005额外五次、network25、checkout3，共24个原进程/50次测试执行，全部exit0、摘要计数匹配、无timeout。014与015延迟后真实握手、016原setup到期、017门取消及原网络/PTY/Job/Unknown/首次退出码边界全部通过。
- final2-groups-results.json 的窄过滤 HostKeys15、HostStages11，以及 crypto49、Client25、HostActivation21、HostApproval11、service14、strict query1 全通过。实际 inventory 中 host_tests 总26包含前两组，crypto49包含host_tests；其他分组也存在交叉，不将这些计数相加成去重总数。007–010日志均记录已知失败的外部通知/析构前 token=true；011证实 stale cancel 不影响新的原请求。三个历史 RED 的 false 事实保留。
- default2-* 从同源码重建独立默认 feature-off 产物，仅 default/rusqlite/sqlite；原 native crypto11全部通过、无timeout，包含默认 owner 的 queued/started/旧票据取消对照。其 SHA256 为 78F646243DDFC942D0B175999955374A4FD59FFFA00007FF5E436B39B8138410，不混用 remote-host artifact。

本片未运行全量 Rust、真实 GUI、持久 existing-key loader、真实手机或公网端到端。生产无配置仍 Locked，不新增 start IPC/UI、部署或自动续期。mux 启动后的原两秒空闲限制、Phone-first Offline/任意顺序会合、真实配置/启动与公网验收仍未完成。
## 已有密钥供应适配与原线程 partial 保管片

基线 aea09248f4804b8ed886f2675931fb4f043d4d08。本片完成私有不可变 NativeHostProfile、两用途 open-only 适配和原 KeyCustodian 线程的 partial 保管合同；没有安装生产配置来源或启动调用者。证据目录为 .audit/host-existing-key-20261113/。

### 实际类型与释放边界

- NativeHostProfile 将原 FrozenHostBinding 与 Endpoint、Relay 两个已有 key 引用一次冻结。引用、名字和公开 profile 都不是权限；冻结不执行任何 native I/O，不枚举或猜测用户 key，不含 Issuer 私钥 opener。实际供应仍受原批准 token、原 Instant cap、严格 document/run 与一次性 claim 约束。
- ExistingKeyLoader 固定 Microsoft Software KSP、当前用户 scope、legacy key spec=0、NCRYPT_SILENT_FLAG 的 OpenKey。Host owned key 的私有来源标记让加载及签名前复验的全部 metadata 读取都使用 SILENT；旧 Client/default 构造仍使用原 flags=0。只导出公开 ECC blob 核对批准 SPKI，不创建持久 key、不导入或导出真实私钥、不写属性、不切换 provider 或交互回退。
- 原 KeyCustodian 线程在首个 native 调用之前拥有空 partial slot，sealed loader 只借用填充。provider/key 只有 API 成功才被原 slot 接收，每步前后核对同 token/cap；不猜失败输出的非零值是有效句柄。loader/metadata panic 的 catch 外仍保留该 slot，先纯撤销，再在同一原线程显式按 key→provider 释放。
- 已知业务拒绝不等于 Unknown。未取得资源可记 NoKeyCreated；取得过 provider/key、成功释放并原线程真 join 才证明已退休。panic 保留 sticky Unknown，即使后续已成功释放和 join；Free 阻塞继续 Pending。Free 失败或 panic 保留原 slot、原 JoinHandle 和 Unknown/outstanding，不自动重试、不 detach，也不以普通 RAII Drop 吞掉资源义务。
- 原两槽 custody、通知/释放保管及 operation 共同推进保持不变。一个 key 的 Free 阻塞或失败不把另一个原 key 的实际 join 隐藏；free 返回和原线程尾部结束是两件事，只有原 handle 真 join 才完成对应线程事实。没有新增 open-ready 协议。

### 行为证据与真实调用范围

- red.* 是旧普通 Result factory 合同的实际行为 RED：仅模拟 provider 已取得后 panic，首次 Free 入口 token=false，exit101。新 sealed-slot 路径的 001 在同一边界先撤销，实际 token=true；不把这个未来适配所需合同的缺口称为已部署 named-key 漏洞。
- HostOpen_001–011 覆盖 provider-only、OpenKey/metadata/SPKI/usage 拒绝、取得 provider/key 后 panic、key→provider 顺序、原线程身份、provider 调用中取消/原期限到期后零 OpenKey、阻塞 Free 时兄弟原 join、Free 返回但线程尾门仍 Pending、第二用途失败无 user exit 收口，以及无 native I/O 的 profile 冻结。fake provider/open/property/export/free 全部由同一测试 API 永久拦截；fake key 的签名前准入拒绝，不能落入真实 CNG。
- 009 的三个有界子进程覆盖 key Free 失败、key 已释放但 provider Free 失败、Free panic。它们证明 Unknown 下原资源和 handle 仍被保管且不重试，子进程结束不证明这些资源在进程内已退休。
- 010 使用公开 TEST_ONLY 向量建立匿名、非持久 CNG key，真实签名并验证 Endpoint DER 与 Relay fixed 签名，随后真实释放/join。018 继续走 NativeHostProfile.freeze→Existing→原 worker 空 slot，再经原批准、activation、显式登记、真实隔离 Relay/TLS 握手与撤销/join；旧 012 及其断言保留。018 的 cfg(test) 适配只将 named OpenKey 替换为公开匿名 key 导入，不打开任何用户已有 key；它不能替代实际 named-key OpenKey、真实配置来源或 GUI 批准验收。
- green1 的测试 helper 参数错误是编译失败；green2 的两个失败来自 mock 对 PCWSTR 指针而非文本比较，修正测试比较后通过。composition1 在编译成功后因缺 CC_DESK_TEST_RELAY_ROOT 拒绝初始化，composition2 使用明确既有 relay 路径通过。上述记录保留，不冒充行为 RED 或生产缺陷。

### 最终同源码验收

冻结1的十一项 Rust/脚本由 frozen1-source-hashes.json 固定，独立只读复核无阻断；两份文档和源码的完整十三文件清单为 final13-manifest.json，验收汇总为 acceptance-summary.json。以下结果均对应冻结源码，不用早期绿色产物替代。

- final1-isolated/results.json 实跑 NativeRemote001–018 各 fresh process、005额外五次、network25、checkout3，共25个原进程/51次测试执行，全部 exit0、摘要计数正确、无 timeout。最终 remote-host artifact SHA256 为 F93F62854517EC7EC992A7956C39A3EC0F2CD6677E22C1B2DD2C8FBB7A4232D1，来源 final1-isolated/selected-artifact.json。
- final1-groups-results.json：native crypto60、Client25、HostActivation21、HostApproval11、service14、strict query1 均通过。crypto60 包含新 HostOpen11 与原49；原 host_tests26 包含 HostKeys15 和 HostStages11。分组存在交叉，不将它们相加为去重总数。
- 同源码默认 feature-off 独立 artifact 的 native crypto11、RemoteHost_Unsupported3 均通过。其 SHA256 为 5DBBA41F36D35BC2BB73C28A80D7FD0228900BB75267B9C7E745A087C1D3D7DA，来源 default1-artifact.json，不混用 remote-host 产物。
- final1-clippy.*：remote-host 严格 --lib --tests -- -D warnings exit0。client-only.*：不含 remote-host 的 --lib --tests --features remote-client-ipc 编译检查 exit0；仍有三处既有测试 helper dead_code 警告，不将此 check 称为 client-only 严格 Clippy。没有为本片修改这些无关 helper。
- final1-fmt.* 对六个本片适合整文件检查的 Rust 模块验证通过，旧 compact 基线未作无关整文件重排；完整十三文件直接 UTF-8 字节检查与 git diff --check 通过。未运行全量 Rust 或全仓格式化。

生产配置来源、native profile 安装、start IPC/UI 和真实本机批准流程仍未接入，默认无配置仍 Locked。本片未读取、生成或配置真实用户凭据，也未部署或改变系统设置。实际用户 key 名称、公开 profile/信任材料及相应访问授权必须另行明确，不能从测试向量推断。真正 named-key 打开、手机和公网端到端仍待独立验收；原 mux 两秒空闲、Phone-first Offline 与任意顺序会合缺口没有改变。

## Controller 保管的准备与登记驱动片

基线 95c19d5009f960bd8542facce0c457bcaf91fbd0。本片补齐内部 prepare_owned 的自主准备和登记驱动，不安装生产配置、不增加 start IPC/UI 或真实凭据调用。默认无配置仍 Locked。证据目录为 .audit/host-activation-driver-20261113/；最终采用明确区分的两版冻结验证，不能将它们描述成一个产物的全矩阵。

### 保管与退出接点

- 唯一 DriverOwner 外壳放在原 controller 槽；它持有原 JoinHandle 的 DriverJoinCore 和 activation。service 在同一个 closing 锁下预留 operation、独立持久 DriverObservation 与 Weak Core；service/operation 不强持 DriverOwner，driver 也不持 controller、Owner 或 Core。Weak 失效不表示退休，原句柄 publication、Joining、Joined、Unknown 分别记录。原 source 槽及 lease.registration 没有拆分或复制。
- 未来接入的 GrantClock、供应闭包及其持有对象不得直接或间接强持 controller/DriverOwner，也不得在准备或登记线程通过 Weak 升级形成最后 DriverOwner 的释放路径；接入前须复核该捕获图。
- 严格 document/run/config owner 门禁后安装空 Owner，唯一 claim 不调用外部 clock。原 driver handle 先被保管再放 gate；同一 token/cap 的 key custody 与原准备线程依旧先保管后启动。claim.validate 的外部 clock 在原准备线程执行；准备原线程真 join 后，登记 completion 在原 driver 执行。错误、取消、publication unwind 都先纯撤销，保留原资源与句柄。
- driver 以纯 preparation_result 判断业务工作已结算，不等待活 key 退休才报告 Registered。成功登记仍可有 keys outstanding。current/snapshot 不调用 clock、不登记、不推进 join；IPC Future 或观察者不是资源保管者。control_reply 可能同步处理到期通知，因此在 controller 锁外采样，锁内只核对原 slot 身份、closing 与 Owner 槽事实。
- 原唯一退出 waiter 同时观察 operation 与 driver，只有 is_finished 的原 driver 才 checkout/join，Joining 仍 outstanding。最后 DriverOwner Drop 先纯撤销，再共同推进 key、prepare、release 与 driver，不能先阻塞 join driver。Controller 显式先在锁外完成 DriverOwner 保管，再允许旧 ApprovalSlot 的 full cancel/同步 wake 析构，避免通知阻塞抢在资源共推进之前。
- 原 driver 捕获的 work/tail panic 先撤销，并在原 driver 线程释放 payload；释放阻塞时原 handle 仍 Pending，其他 owner 仍可推进。最终 join 若仍返回异常 payload，Core 以不可自动析构槽保管它，保持 Unknown+outstanding、不重试、不另建清理线程。原已知业务拒绝与原 prepare/completion 的 panic 分类保持，不将所有拒绝都改成资源 Unknown。

### 行为证据和测试范围

- red-build.* 编译成功；red.* 的旧019实际 exit101：原 test_prepare_owned 停在 Consumed，没有观察者显式推进时不会登记，清理已完成后才断言失败。这证明旧测试驱动链缺少自主登记，不称为已启用生产入口的故障。green1 的019已通过；随后独立审查发现锁内 control_reply 与观察者析构 panic payload 两处边界，已在最终冻结前修复。
- HostDriver001–008 分别覆盖无观察者自主一次登记、阻塞准备/登记时 direct service exit 仍 Pending 且原 native 分支推进、最后 service 与 Owner 释放无自 join、spawnErr/publication unwind/关 gate、业务返回但原尾门未 join、拒绝与 panic 分类、阻塞 payload 下兄弟 key 原 join、异常 payload 隔离。002/003 使用受控合成回调，不冒称实际 GrantClock 阻塞或完整 Controller/approval watch 最后析构的动态验收；后者的显式 Drop 顺序另由源码审查覆盖。
- 007 使用公开匿名 CNG key，payload 释放前已撤销、兄弟原 key 实际 join，driver 仍 Pending；key join 也可由原通知线程推进，不单称最后 Owner 亲自完成全部 join。008 有界子进程仅证明 Unknown 时原责任仍保管，进程退出不是正常退休。
- NativeRemote019 走新内部 prepare_owned、原批准与 Existing 匿名适配，等待原 driver 的完成信号期间不查询 current，之后只读 Registered。真实 claim/register clock 共两次、位于不同原准备/登记线程；反复 current 不增加 clock 调用。新入口 duplicate 与异源 caller 拒绝不撤销原合法批准。到期 watch 探针实测撤销先于 wake、controller 锁可用、零 reservation/零 open。随后真实隔离 Relay/TLS 握手通过，opens=[1,1]、token 撤销及原 network/key/driver 收口。旧012/018及断言保留。
- green2 的 Driver7/8 失败来自002在独立 native 分支到终态前就断言它已完成，记录保留；最终使用秒级有界等待，未改生产期限。冻结1随后通过完整矩阵，但该测试仍先采样 exit Pending 再等 native；冻结2仅移动原四行采样到 native 等待之后、release gate 之前，精确证明兄弟已推进时 driver 仍阻止退出。

### 双版本最终验收

- 冻结1八项清单 frozen1-source-hashes.json；final1-isolated/results.json 为 NativeRemote001–019 各 fresh process、005额外五次、network25、checkout3，共26个原进程/52次测试执行，全部 exit0、摘要正确、无 timeout。final1-groups-results.json 的 Driver8、crypto60、Client25、Activation21、Approval11、service14、query1 全通过。crypto60 包含 HostOpen11 与原49，host_tests26 包含 HostKeys15 与 HostStages11，分组有交叉，不相加为去重总数。
- 冻结1 remote-host 产物 SHA256 为 7CBA330E123558E15272A1906DD2B3583149B9AFBA826894964A194806217923，82685440 bytes，原执行路径见 final1-isolated/selected-artifact.json。它在冻结2重建前已复制到 final1-remote-host-artifact.exe 并复核字节/哈希，记录 final1-preserved-artifact.json。原 target exe 路径后来被冻结2覆盖，不能继续用该路径的当前字节代表冻结1。
- 冻结1默认 feature-off 独立 artifact 的 crypto11、RemoteHost_Unsupported3 全通过，SHA256 5DBBA41F36D35BC2BB73C28A80D7FD0228900BB75267B9C7E745A087C1D3D7DA；client-only.* 的 --lib --tests --features remote-client-ipc check exit0，三个未改测试辅助项 dead_code 警告仍在（test_only_install_original_client、admin、signed），不称为 client-only 严格 Clippy。final1-clippy/fmt 均 exit0。
- 冻结2清单 frozen2-source-hashes.json，freeze2-only-test.diff 与 freeze2-delta-proof.json 证明唯一差异是002原四行采样块位置；其余七项与全部生产代码逐字节不变，独立复核无待改项。final2-clippy 为 remote-host --lib --tests -- -D warnings exit0；final2-fmt 六模块只读检查通过，final2-build 成功。
- 冻结2产物 SHA256 42E77A4E5908E6CA0C6E5AB0E084182646B93F19E0633E0B9781FB9B668C9206，82688512 bytes，见 final2-artifact-hash.json。final2-results.json 实跑完整 Driver8（0.41秒）和 exact019独立进程（1.13秒），均 exit0、无 timeout。该产物没有重跑冻结1的完整隔离矩阵/default/client-only；本片据唯一测试断言顺序差异作受限增量验收，不将两个产物的结果混称同产物全绿。
- 最终十文件清单 final10-manifest.json，双版本汇总 acceptance-summary.json。源码/文档直接 UTF-8 字节检查与 git diff --check 通过。未重排旧 compact service 基线，未运行全量 Rust、全仓格式化、真实用户 key/native dialog、手机或公网验收。

生产配置来源、native profile 安装、批准成功后的应用触发、start IPC/UI 仍未接入。prepare_owned 是内部可编译的已保管驱动，本片仅由公开匿名 fixture 调用；它不等于生产来源或端到端已启用。原 mux 两秒空闲与 Phone-first 行为不变，没有真实凭据操作、系统安全变更、部署或推送。


## 单次 start 应用入口后续分片

现已增加严格 document/run 的 start IPC 与自主登记后单次网络触发，生产配置仍为空。调用链、状态语义、测试与未验收边界见 [Native Host 单次 start 入口](native-host-start-entry.md)；上文各历史分片的未接线声明保留其当时证据口径。
