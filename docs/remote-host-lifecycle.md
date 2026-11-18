# Experimental native Host lifecycle

`remote-host` is an opt-in Cargo feature, excluded from the default build. It
includes the existing `remote-client-ipc` feature and the local Host dependency.
The application service starts Locked: there is no JavaScript owner, identity,
credential, pairing or native-provider creation API. Enabling this feature does
not make remote sessions usable or certify a mobile/CLI/platform combination.

The native Host facade retains each original pending rebind and its original
`RunningChannel`. An observation token does not own the operation. Dropping or
timing out an observer cannot replay it. Installation facts are cached only after
the actor's actual success reply; they grant neither Data access nor snapshot ACK
or input-window eligibility. Revoked authority prevents disclosure.

Phone disconnection closes that exact channel authority, without sealing the
session writer or stopping the user's process. Application shutdown instead seals
admission, requests all owned stops, and concurrently observes original network
tasks, pending installs, writer joins and native stop work. Both desktop close and
explicit exit use one retained waiter and the first exit code; handoff maintenance
refusal takes precedence. There is no timeout-to-success or automatic retry.

`WriterRetirement::Joined` proves only the owned writer thread joined. Network
join proves its original task was awaited, not normal protocol completion:
`RunningChannel::wait_closed` folds task panic into `Closed`. Root process exit,
EOF, an empty active cache or the old five-second shutdown return do not prove
whole PTY/reader/master/descendant or command-owner retirement.

In Host-enabled builds, the existing output reader's original JoinHandle is held
in one fixed slot, published before adoption completes, and actually joined by
the retained shutdown observer. EOF/output callbacks do not prove thread exit.
The aggregate remains Pending while that reader is Publishing/Running, even if
root/stop facts are already Unknown, so the application cannot stop observing it
early. A thread panic stays Unknown, not Joined; a second reservation cannot
replace an existing handle. No hard preemption or join deadline is promised.

The native adapter also retains each original waiter/retry thread handle in a
per-run bounded set in Host-enabled builds (64 unjoined handles maximum). Only
finished original handles are joined; panic stays sticky Unknown while other
live threads remain Pending. A full set rejects creation without losing custody;
an explicit stop/shutdown may retry after finished handles are collected. This
is a local custody bound, not a global capacity/DoS guarantee. Default-off
non-test thread spawning behavior is unchanged.

The final registry-retired check and new-thread publication share the custody
lock. A late starter cannot create a thread after terminal joins. Observers use
try_lock and report Pending during publication; no deadline for thread creation,
join, or native calls is promised. wait_workers_joined requires successful real
joins plus registry retirement, not waiter_running=false or a lifecycle callback.

Host-enabled creation wraps the three original Rust I/O owners with passive
destructor-return tracking. The transferred reader stays tracked until its actual
Drop, not take_reader or EOF. master and writer use Deref to the original objects;
the master-to-writer-to-untransferred-reader order remains unchanged. No added
flush, waiting, native close policy, thread or handle duplication is introduced.
The reader wrapper allocation occurs before child creation. Default-off non-test
builds retain the original unwrapped types.

SupervisedRun captures a token containing only shared atomic status; it cannot
keep the I/O objects alive or change their last release. A completion bit is set
only after the corresponding underlying Drop returns normally. An unwind guard
records failure only, never successful return. Aggregate observation stays
Pending while any original owner is nonterminal, then Unknown if any failed.
original_io_drop_returned proves only that these Rust destructors returned; it
does not prove kernel close success, console-process exit, thread join or tree
cleanup. A blocked destructor remains pending; no hard deadline is claimed.

The current native adapter still deliberately reports overall Unknown for a
tracked remote native run after output_reader_joined, wait_workers_joined and
original_io_drop_returned become true: kernel/console, command-owner and
whole-tree retirement witnesses remain unavailable.
io_threads_joined therefore stays false. Unknown prevents application exit in
this opt-in mode and keeps custody. Default-off production reader behavior is
unchanged; no new process, worker framework or stop policy is introduced.

Remaining admission gates: a real authorized same-run writer/provider factory
(the facade currently creates an empty owner), complete native retirement proof,
authenticated peer-install bootstrap, actual WebView/CLI/phone integration,
multi-Host behavior and capacity/recovery qualification. No arbitrary peer/fence
constructor, production database migration, CLI version restriction, network
listener or deployment is introduced here. The staged native adoption seam
currently admits one exact live supervisor/run scope and is not a product
capacity promise.

The ordinary desktop paths now converge on the original `OwnedPty` mutation
owner: `NativeRuntime::input_commit` and `input_protocol` use
`RunAccess::with_writer`; `NativeRuntime::resize` uses `RunAccess::resize`.
Original writer and master handles share one mutation mutex. A logical frame
(including flush) excludes resize, while reader and root-stop locks remain
independent. Exact document/run/resource identity and Running phase are checked
again after waiting; Exited resources remain available for snapshot/output drain,
but cannot admit new write or resize effects. Revocation after admission does not
roll back an already executing effect. Existing input receipts/no-replay and
nonzero desktop dimensions remain unchanged.

The resize IPC awaits a blocking task just like the existing input paths, so
waiting for a logical write does not synchronously occupy an async worker.
Dropping the awaiting future does not cancel or replay an admitted native effect;
the original blocking task retains the same access and performs its lock-time
authorization check. No timeout is turned into a successful resize.

This is local serialization, not remote ownership transfer. It promises neither
FIFO among not-yet-admitted calls nor a deadline/preemption for blocking native
write or resize. The authenticated remote actor is still not installed as this
owner, and no parallel remote writer is exposed. A production provider must route
or reject the desktop authority at its actual handoff; this patch alone does not
solve that gate or complete PTY/Job retirement.

## 可信 Native Host 来源槽

`RemoteHostService::begin_native_network` 先通过原 `NativeRuntime` 获取本次真实
`NativeRunAdmission`，随后必须选择同一 service 的可信 native 注册槽。生产入口不再
接收可直接进入网络的原始 bundle；旧的原始输入入口仅保留在 `cfg(test)`。
当前已有逐次 native 批准 owner，以及仅提供 current/request/cancel 的 controller。
普通应用启动仍没有真实 profile/材料供应者或 activation 注册调用者，槽默认
为空并保持 Locked。没有生产 start IPC、配置文件审批、开键或 enrollment，也不把
客户端 `NativeGrantPresence` 解释成 Host 启用批准。现状见 [批准组件](native-host-approval.md)。

注册固定原 Source Arc、弱 resource identity、scope、完整 HostSnapshot（包含原样
activation/epoch/origin）、peer/browser/issuer/principal、policy、route 与预算。注册
只能发生一次，元数据不能替换。每次尝试消费一份 move-only LocalIdentity/Relay
能力；可信 native owner 只能凭 opaque weak token 补给下一份材料，不能覆盖批准
元数据。缺注册或缺新材料不能开网。activation 保留原协议的 opaque 值域，没有新增
“全零等于缺失”的协议约定。注册本身不产生 Ready、grant 或原生输入权限。

每次选择仍复查本次原 document/run/resource 权限，沿原 SourceClock、TLS/AppBind、
browser grant、peer install、exact ACK/window 和同一 writer 执行。注册活性加入原
连续检查，并在外部 clock 返回后重验；原 ClockOrigin/Source Arc 与 sticky Unknown
均不能重置。撤销不可恢复，最终成功发布与槽关闭在同一短 service 锁下决定。

正常撤销和退出在锁内移出未用材料，锁外先通知原网络 owner 再释放本地材料。
时钟/continuity 调用栈内撤销只同步失效，不 Drop 材料或唤醒 Waker；私有采样 guard
退出只恢复 depth。原 begin 返回或 owned task 终态到达安全边界后，才完成幂等 stop
和材料释放。当前通道仍依原 LivePolicy/channel 终态真实 join；跨 Source 的回调撤销
只保证资格立即失效，未观察到的资源终态仍为 Pending/Unknown，不承诺同步退休。
材料 Drop、撤销返回与网络 join 都不证明外部 signer/key worker 或整个 Job 已退休。

公开 TEST_ONLY loopback fixture 验证同槽 fresh 材料的原资源重绑、原 ACK/window/
HostWritten、撤销后本地 Node 输入继续及原 handle 回收。Host-first fixture 的资格
前提不关闭历史 parallel 启动失败；真实批准来源、实际调用该入口的产品 activation
流程、手机/公网 WebPKI 和完整原生生命周期仍是独立门禁。

槽固定的是批准描述与 Source Arc/弱 origin。LocalIdentity 现已提供 endpoint identity
和 role 元数据，Relay identity 已提供 key id 与 role 元数据。单次批准的注册边界将
实际材料与冻结描述比较，随后继续通过原 TLS/pin/AppBind/Relay/grant 校验。底层
replenish 接口仍依赖同一可信 native supplier；当前批准 owner 不开放补充或自动重连。
`UnavailableProfile` / `UnavailablePresence` 本身也只在 `cfg(test)` 中编译；生产实际
缺少 profile/presence supplier 对象。普通 `lib` 启动没有调用 register/begin，不能将
这一强制选择方法描述为真实来源已配置。

`NativeKeyProfile` 只规定 metadata/open 契约；`NativeOwnedSoftwareKey` 尚无生产
constructor 或现有密钥打开路径。`RemoteClientService::native_signers` 可以绑定
`RelayRole::Host`，但签名批准期限仍由可信调用者提供；它既不产生 Host activation
批准，也不能把具体 COSE 请求的 `NativeGrantPresence` 解释为 Host 启用批准。
客户端 `install_profile` 连接的是 Client 角色，不能用其成功结果代替 Host 来源。

现有本机批准 owner 已冻结完整 Host 描述、原 run/resource 范围、activation 与期限，
并保管 sticky 撤销。生命周期首片增加 service 的唯一 activation operation 和原退出
waiter 保管，但仍无生产启动调用者。真实调用链仍缺真实 profile/材料供应者与 signer
生命周期接线。这些不是 relay 地址、origin/RP 或公钥 JSON 的
解析结果，不得增加默认批准、占位启用 handler 或另一套批准系统；默认空槽保持
Locked。真实来源实现与实际密钥登记/配置仍是两项不同门禁，不能把批准组件的
存在称为真实供应者已经实现。后续方案见 [启动保管链计划](native-host-activation-lifecycle-plan.md)。

材料从 fresh 槽移出前登记单一 preflight reservation，原 network.begin 正常返回
（已交原任务或失败析构已返回）才清除；预检期间拒绝重填与第二个预检。材料移出
关闭槽直至实际 Drop 正常返回期间保留 release 事实，unwind 记录不可恢复的 Unknown。
fresh=None 不能证明材料已经退出。退出的原唯一 waiter 先保管 handle，再在自身任务
内通知并停止原 owners；采样回调不再同步进入完整停止准备。准备失败不发布退出，
首个 code、重复调用与晚到 pin/pump 的原保管不变。原 waiter 同时等待材料事实，
最终与 retired 决定在同一短锁内重验；Pending/Unknown 不因观察超时变成退休。

## Never-remote desktop exit contract

A standard remote-enabled build may return to the original local desktop exit
contract only after every remote admission domain is sealed and its conservative
responsibility history proves it never owned remote work. Controller approvals
(including Forget), registration/key preparation, client construction, unattended
preparation, source/activation and network responsibility are marked before their
respective effects under the original admission locks. Failed attempts do not
clear this history. Read-only scope/current/list operations are not approvals.

The opaque LocalDesktopExit proof is minted only after these seals. It still runs
the original Native shutdown and requires its join, settled launch/command slots
and empty active desktop inventory. LocalDesktopStopped does not attest all IO
threads or descendants and is not OwnedSessionReleased. Any remote responsibility,
unknown state, failed join or incomplete command remains on the existing strict
remote shutdown path. This fix cannot repair a previously loaded binary already
stuck in ExitIntent::Unknown.

Validation includes the real production app-exit service with an ordinary Native
run (allowed) versus prior controller responsibility (still refused), plus original
preparation shutdown/cancel ownership tests. This is not exhaustive scheduling
proof of every concurrent entry point. Unprepared reserve_start is test-only;
production reserve_prepared remains behind the original same-flow reservation.
