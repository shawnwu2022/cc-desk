# Native Host 单次启动保管链设计

状态：首片已完成实现、定向验证与独立复核。核查基线为 `cbe7cae13b0b7f4c3291bf07decb26f996a9b230`。测试证据和未完成验收见末节；本文不代表完整供应链已验收。

本片要证明：即使 Host 尚未注册网络或 pin，应用退出仍保管本次启动预留、原准备线程和所有已交付材料。生产默认配置继续为空。生命周期首片通过之前不增加生产 start IPC、UI 按钮或真实密钥打开调用。

现有行为见 [Native Host approval component](native-host-approval.md) 和 [Experimental native Host lifecycle](remote-host-lifecycle.md)。首片生产代码只保管生命周期；材料工厂、批准安装与启动调用仍仅由隔离 fixture 驱动。后续两用途工厂与 start IPC 未实现。

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

2B 仍须把原批准 lease 的 cancel/expiry、operation cancel、service close 和准备失败绑定到同一 token，覆盖先取消后晚绑定；被冻结的工厂不得在 Yes 后重新读取 profile binding。两原 key owner、signal handle、待通知 mailbox 和最终 slot 释放必须由 canonical 保管链覆盖，晚到 owner 必须接收、晚到 start gate 不得放行。两个 key 槽的 Unknown 不能短路另一个槽的真实 join。原准备线程尚未 join 时，即使两槽暂为空也不能宣布退休。

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
