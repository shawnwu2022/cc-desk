# Client-owned bounded unattended access

Status: design, not an enabled product feature. No real device, key, server or
authorization is provisioned by this document.

## User direction and implementation choices

The user wants the development-machine client to decide unattended access
explicitly and revoke it at any time, with convenient everyday use and automatic
reconnection where this does not weaken authentication. The coordinator selected
WSK-MSI, one explicitly identified paired phone, a maximum 30-day delegation and
candidate site ccdesk.wushangkun.cn. The phone's real identity is NOT known.
These are constraints for an eventual local consent, not an active grant.

First baseline scope: one selected live native run and the selected installed
phone. The user's later direction also requires evaluating recovery after App
restart, so the complete feature must have a separate durable target policy and
native restore owner. Persist only explicitly authorized target/profile identity,
scope, device, role and absolute end. On startup begin Locked, resolve the target
against current trusted native workspace/profile state, then issue fresh runtime
scope and keys only after all checks succeed. Never restore old document/run
handles or broaden a missing target into any available terminal. An unverifiable
target remains Locked with a specific reason, not a claim of completed recovery.
No new OS boot registration or blanket future-process authority is implied.

## Alternatives

1. Extend the current one-shot approval to 30 days: rejected. Its supplier and
   permit are consumed once; controller ownership is exact document/caller/run.
   Reusing it would conflate a connection grant with a persistent delegation.
2. Let the Relay approve reconnects: rejected. Relay availability is not endpoint
   authority and cannot implement local device revocation.
3. Add a native delegation owner issuing fresh bounded per-attempt permits:
   selected implementation direction. Retain the original strict default path.

## Required authority and persistence

Default is disabled. Enabling requires an authenticated local Host UI operation
and native confirmation displaying the installed phone's verified fingerprint,
selected live run, role/capabilities, exact origin/RP/Relay route and absolute end.
Never derive a phone from last-used order or accept a JavaScript approved=true.
Missing installation, identity ambiguity or unsupported trust data stays Locked.

Store a versioned, bounded public delegation record, not keys or old assertions.
It binds the native installation, selected phone and credential/key fingerprints,
enrollment revision, issuer/principal, all relevant epochs, exact origin/RP/route,
selected run scope, allowed role, consent revision and absolute expiry. Separate
the 30-day delegation from the short-lived capability generated for a connection.
Changing any authority-bearing field requires a new local consent.

Durable state must distinguish Disabled, Enabled and Revoked, retain a monotonic
consent/revocation revision and observed wall-clock floor, reject malformed or
partial records, and serialize writers. Persistence succeeds before Enabled is
published. On disable, close the in-memory admission gate immediately, persist
the revocation and cancel existing activity. If persistence fails, remain blocked
in this process, report the failure and do not claim reboot-safe revocation.
Startup must check durable state, current installation/trust/run and clock before
any key supplier or network task is created. No absent-file auto-initialization
may restore Enabled. Define a protected local storage trust boundary explicitly;
local administrator rollback is not solved by a JSON revision or wall clock.

Anchor the absolute end to both wall and monotonic time without extending it on
reconnect. A backwards clock observation, expiry, deleted device, changed trust,
missing state or uncertain record closes admission. Restart never resets the end.
Do not extend the delegation silently when refreshing a per-attempt capability.

## Owner and reconnect state machine

States: Disabled, EnabledIdle, Starting, Connected, Retiring, Backoff, Revoked,
Expired and BlockedUnknown. A single native owner serializes admission, revision,
attempt identity, cancellation and retry decisions. At most one attempt is live.
Each attempt carries the exact consent revision and scope; recheck these before
key supply, before network creation and before publishing the connection.

For each attempt create fresh source material, channel authentication, nonce,
exporter binding and short-lived grant. Never clone consumed ActivationTicket,
FrozenHostApproval, native material supplier, grant, input buffer or assertion.
Reuse the existing service reservation/driver custody and exact run admission;
do not impersonate a stale document caller to make a background path work.

Retry transient transport loss with bounded backoff only after the original
socket, key/material owner and tasks have verifiably retired. Pending or Unknown
retirement blocks retry; a timer expiring is not proof of join. Retry timers are
owned/cancelled/joined too. Authentication, trust, policy or storage rejection
does not retry into success. Never replay pending or uncertain terminal input.

An ordinary user Disconnect stops the current connection. A separate prominent
Disable unattended access both stops it and prevents reconnect; make this
distinction visible rather than surprising the user with an automatic reopen.
Device deletion and trust invalidation call the same disable/cancellation path.
Existing pairing Forget only writes a public tombstone and is NOT sufficient:
wire that operation to the delegation owner before claiming device-level revoke.

## Phone-side convenience without implicit authority

The existing Browser channel requires fresh presentation and WebAuthn UP/UV for
its new binding. Keep this validation. Do not replay the previous assertion or
replace UP/UV with a stored-key possession check. Device capability determines
whether a fresh operation needs a prompt; no cross-device silent UX promise.
Retain enrollment and prepare the next authenticated connection conveniently,
but show verification-needed state instead of repeatedly opening prompts.
Phone background resumption cannot become permission to select a new Host run.

The existing public Host configuration uses a 60-second grant, not a convenient
long-running session. Stage 1 preserves that bound to avoid silently widening an
old capability. Delivering the complete low-interruption feature requires a
separately reviewed per-connection budget/continuation design inside the approved
delegation, with revocation checks throughout the real data path. Reconnecting
every minute with another prompt is not acceptance of the convenience goal.

Implementation proposal for the separate delegated path: at most 15 minutes per
fresh connection capability, clamped to the delegation's absolute remaining life,
with continuous trust/revocation checks. This does not change the old 60-second
configuration path. A longer uninterrupted authorized session or silent phone
recovery is not implemented or claimed by choosing this budget; the actual
channel/credential behavior and user interruption cost still require validation.

## Native delegated admission boundary

The existing NativeRunAdmission continuously checks RunAccess's original document
caller, including on input effects. Retaining that object is not a background
authority. Do not add a skip-document boolean or re-create CallerIdentity.

Add a separate closed native DelegatedRunOwner captured only by valid local
consent plus current real run admission. It retains original supervisor/state/
resource/scope identity and the delegation gate, not an old webview capability.
Each non-Clone DelegatedRunAdmission checks both original resource lifecycle and
consent revision/device/trust/expiry. Reuse NativeRunAuthority and the exclusive
NativeProducerCheckout without treating either as remote permission on its own.

Use an independent prepare_delegated_owned path with the same service reservation,
driver publication gate and key/prepare/release custody sequence as one-shot.
Never relax replenish_native_source's existing exact admission contract. Native
and delegated paths must contend for the same service-owned resources. A private
retirement witness may be emitted only after original network, preparation,
driver and key/material release completion all converge; no timeout or vanished
Weak reference is such a witness. This integration is not present in Stage 1.

## Revocation semantics and offline limits

Local disable prevents new attempts and cancels existing authority before any
success UI. Distinguish authorization revoked from resources retiring/unknown.
It cannot undo bytes already accepted by the real CLI or stop unrelated CLI jobs.
The original input-effect and task-custody boundaries still apply.

An offline or suspended Host cannot receive a phone-side revoke immediately.
Do not invent an online revocation service. A local durable revoke made while
network-offline takes effect on the local process and is checked before future
connections. If the application is closed, no in-memory connection is live;
restart must check the durable tombstone first. Exact reachability and resume
behavior require real suspend/network tests.

## Implementation sequence and evidence gates

1. Process-local native policy model: strict binding and disabled default; finite
   deadline, revision, revocation, competing attempt and failure tests. No
   persistence, production consent constructor or networking exposed by this stage.
2. Separate protected consent store and exact durable target policy: atomic
   persistence, concurrent writers, uncertain commits and restart revocation tests.
3. Integrate original live-run admission, fresh material suppliers, cancellation,
   actual task join and one-at-a-time reconnect. Exercise the real call chain
   against fixture Relay, including Relay generation change and clock rollback.
4. Local Host UI and authenticated IPC: installed-device selection, finite scope,
   explicit native consent, status and single-action disable. Wire device deletion
   and trust changes. Verify stale replies, double-enable, disable-during-start,
   document replacement, changed run and failed persistence, not only state getters.
5. Restart/storage tests and actual Browser/native test: fresh phone proof, loss
   and reconnect, no replay, revoke while connected, revoke while starting, old
   revision after restart, expiry while suspended and unknown-retirement closure.

   Durable target owner: distinguish a stable approved target from a transient
   run/document. Build a dedicated native admission path, not a synthetic webview
   caller. Prove restart reconstruction against current workspace/profile/device
   versions, old revision rejection, absent/moved/changed target rejection and
   revocation before network on startup. The single-run baseline alone does not
   fulfill this stage or the user's cross-restart convenience goal.

No stage may present the feature as usable until the UI-to-network path and
revocation chain pass. No preselected real phone, DNS/server change, new key
configuration, startup registration or security-setting change is authorized by
an implementation test. Record exact test artifacts and unresolved platform gates.

## Existing code anchors

- remote_host_configuration::freeze currently constructs a 60-second candidate
  and Controller role, with a one-shot native material supplier.
- remote_host_approval::ApprovalMachine consumes the description once; its
  native lease revokes key/source authority and existing channel work.
- remote_host_controller::prepare_owned_mode authenticates document/caller/run
  and reserves the service-owned driver before publishing an attempt.
- remote_host_network replenishment still requires the exact original registered
  source/run/scope and rejects Unknown; it is not a generic reconnect API.
- remote_native_forget_approval currently persists a public identity tombstone,
  not revocation of an independently approved running connection.
- browser-run-handoff retains per-channel presentation/WebAuthn and does not
  overwrite the stored enrollment with the newly authenticated run scope.

## Stage 2b transaction evidence (inactive)

The independent unattended directory now owns a fixed `commit.journal` beside
`delegation.dpapi`. Neither parsing nor recovery constructs native consent,
a live owner, a key supplier or executable admission. Results remain
`LockedSnapshot` descriptions. Existing one-shot admission is unchanged.

The journal is an append-only sequence of 224-byte records, with at most 8,192
records (4,096 commits including initialization). Each little-endian record has:
magic `CCDUJ001` (0..8), kind Pending=1/Clean=2 (8), zero padding (9..16),
sequence (16..24), transaction UUID (24..40), store ID (40..56), storage revision
(56..64), authority revision (64..72), canonical target SHA-256 (72..104), exact
sealed blob SHA-256 (104..136), previous full-record SHA-256 (136..168), zero
padding (168..192), and SHA-256 of bytes 0..192 (192..224).

Parsing requires exact alternating Pending/Clean pairs, matching pair stamps,
consecutive sequences/storage revisions, stable store/target, authority revision
unchanged or incremented once, and unique transaction IDs across the entire
bounded history. These hashes detect inconsistency; they are not authentication
against the principal controlling the trusted local directory. The complete pair
capacity is reserved before Pending. Capacity exhaustion rejects further writes
without reset or compaction.

All steps run under the existing private NTFS control lease:
1. Recheck process poison after acquiring the lease. Open and verify the pinned
   journal identity; read and strictly verify the full chain and matching blob.
2. Reflush the matching blob and journal before returning recovered evidence.
   A successful read permits at most one commit; another read must fully verify
   again. Reuse without rereading is rejected before writing.
3. Check native permit/CAS and capacity before Pending. Append and sync Pending.
4. Recheck authorization at the existing publication fences, publish the main
   blob atomically, verify its identity/content and sync it.
5. Append and sync matching Clean. The successful Clean sync is the normal
   commit linearization point. There is no fallible production I/O afterward.

Any failure/cancellation after entering the Pending write interval reports
Unknown and poisons that real store identity in this process. The bounded
process registry is keyed by NTFS directory/journal identity, not only path;
creating a new Store/backend does not clear known failure. Registry exhaustion
fails closed and never evicts known Unknown entries. Lease acquisition and read
return recheck poison, including the deterministic waiting-reader race.

A different process can accept a complete matching Clean only after rereading,
verifying and successfully flushing both files. That is independent transaction
evidence at the new flush boundary, not proof the earlier caller received an
acknowledgement or that a failed flush had made data durable. The earlier call
and any same-process known failure remain Unknown. Missing/empty existing,
malformed, partial, trailing Pending or mismatched evidence cannot be repaired
by reopening or reinitializing. Only a newly created backend may initialize its
own empty journal; abandoning creation and reopening is not implicit consent.

This stage does not prove physical power-loss durability, resist restoration of
an entire old valid blob+journal by a local storage principal, or propagate
revocation to a live owner in another process. No real DPAPI runtime operation,
real credential, cloud endpoint, UI, startup registration or networking is
activated by these tests. Native delegated admission remains a later gate.

## Stage 3a captured native run (inactive)

`run_supervisor/delegated.rs` separates delegated custody from the existing
document-owned `NativeRunAdmission`. Capture consumes a genuine admission,
checks its original document/resource before and after matching a sealed native
consent, and retains the exact supervisor/state/resource/scope. It does not retain
an old caller token as delegated authority. The existing one-shot path still
rejects a retired document.

The private capability adapter reuses Stage 1 Owner/Attempt: one live ticket,
fixed absolute/monotonic deadlines, exact binding/revision, rollback and stale
ticket rejection. There is no production consent or VerifiedBinding constructor,
no IPC command, no conversion from LockedSnapshot and no automatic reenrollment.
Synthetic bindings and clocks exist only in tests.

The only new effects are typed calls to the original NativeLocalInput frame and
resize operations. The policy lock covers synchronous enqueue, original actor/
PTY effect and result receipt. The operation fence cannot escape this interval:
the generic helper and fence type are private, consumed inside typed operations.
The actor checks the fence, original live supervisor/resource/scope with
nonblocking lock acquisition, then the fence again. View bindings cannot use
either control operation. Supervisor shutdown is checked as soon as its shared
shutting_down flag is set, not only after the asynchronous per-run stop worker.

NativeLocalInput uses an unbounded receive for its original result. No timeout
was added to discard or retry an uncertain frame. Revoke first sets an independent
atomic close request without waiting for the policy lock; new attempts and the
actor fence reject it. Nonblocking observation returns Retiring while an effect
holds the lock. EffectsClosed describes only closure of this effect interval,
never socket retirement, producer join or complete task reclamation. Partial
frames and non-Policy actor errors retain their original result and latch Unknown.
A known Policy rejection with no effect can close normally. Drop requests close
without blocking; it never manufactures retirement proof.

Already-started frames cannot be preempted. If the actor or PTY stalls, retirement
can remain pending indefinitely and cannot be reported as complete. Clock failure
and rollback fail closed; sampled expiry is enforced at the effect boundary.
This stage has no expiry watcher, production trust-change subscription, fresh
material supplier, channel adoption/rebind or reconnect loop. Those require a
separate network/retirement integration stage. The captured single-run owner also
does not reconstruct authority after application restart.

## Stage 3b-1: Host-local channel retirement facts (inactive)

`NativeHost::begin_channel_retirement` issues an opaque token branded by the
original Host identity and exact channel/grant/attempt set. It seals each matching
attempt's dispatch admission before requesting channel close. It never seals the
original native writer. Repeated observation uses the original bind receiver and
original coordinator request/response task handles, collecting their actual join
results. A discarded observation token cannot discard those handles or pending
input results. The existing disclosure API may report Unknown after channel close;
that does not erase the independently retained terminal facts.

Each installation attempt now owns its own dispatch custody registry. Native
connections reserve in that exact registry; retirement seals only the matching
registries, while full Host shutdown seals and observes every attempt. There is
no parallel global result registry. All attempt registries share one 64-cell
budget. The credit belongs to the result Cell and is released only when its last
Arc is dropped, including any original Wait still holding a ready result. This
intentionally tightens the old registry-only limit. Pruning a Drained index does
not release a retained Wait's credit or erase its result; Unknown and unresolved
cells remain owned. Cleanup snapshots are dropped before a fresh credit is taken,
and credit destruction never takes an Owner lock.

Observation exposes only Pending, Retired or Unknown. Lost original replies and
actual task panics latch Unknown and prevent rebind; the full shutdown owner
still retains and joins remaining tasks. Retired proves only these Host-local
facts. It does not prove socket, RunningChannel transport, App PeerPump or material
supplier retirement, and cannot construct a complete Stage 1 RetirementProof.
There is no new consent constructor, supplier, service-mode integration, retry
scheduler or direct phone-to-NativeLocalInput route. Prepared-channel branding and
adoption/rebind within the same service reservation remain subsequent work.

## Stage 3b-2a: sealed single-channel handoff (inactive)

A DelegatedSession reserves the existing ServiceState activation slot and uses
its original HostCheckout. One-shot activation, source registration, adoption
and publication reject that delegated mode under the same service lock. The
session holds only a Weak service reference. Its prepared channel is not an
arbitrary RunningChannel wrapper: the only constructor is a test-only real
TLS/AppBind/grant exchange using the session's owner-bound clock. Both session
and attempt brands must match before the original producer can be handed off.
The native create/adopt methods accept only that sealed prepared type.

The original NativeProducerCheckout is consumed once. Even a failed later handoff
retains the same permanent NativeHost; no replacement producer or factory reset
is introduced. The registered PeerPump Arc is the original task, not another
executor. Actor installation rechecks the original resource/scope and the channel
fence. Control does not silently promote View. Revoke closes policy first;
ordinary disconnect closes the attempt clock even while preparation is still
Starting. Neither disconnect nor dropping an attempt releases the Stage 1 live
slot. This slice deliberately has no complete receipt or second-generation rebind.

The session keeps one stable Arc GrantClock domain. Initial binding is one-shot;
there is no reset method. Clock sampling serializes only the short Stage 1 state
check, retaining revision, epoch, deadline and final close checks. A network
attempt owns a private Attempt, never a local effect Ticket. The single-live rule
prevents a valid local effect from coexisting; stale local tickets are rejected
before any IO. The resource callback executes before the Owner lock, and selected
clock custody is cloned and unlocked before sampling. These invariants must be
reviewed again before adding retirement receipts or clock rotation.

After full capture, clock lifetime checks use the exact captured weak identities,
supervisor shutdown/stop/reap atomics and the original input's atomic Complete
status. They do not contend with the producer's canonical checkout mutex. In the
current production lifecycle, resource is initialized once, never replaced or
refilled; every production removal first retires the writer, whose retirement
seals the original input. Canonical scope is initialized once. Actual native
handoff and actor authorization retain their full resource-slot and scope checks.
Any future resource-refill feature needs an irreversible invalidation gate rather
than silently relying on this lifetime argument.

Only fixed public test identities currently construct prepared channels. Complete
production enrollment, key-fingerprint/issuer/pin validation and material supply
are not implemented. There is no real phone approval, UI, restart authorization,
retry scheduler or cloud change. Starting/prepared cancellation remains closed
without any complete retirement claim; the later slice must retain and join all
original preparation, transport and material tasks before issuing a receipt.

## Stage 3b-2b-i: original socket and material custody (inactive)

The one-shot and sealed delegated runners share NetworkWire for original
socket/MUX/TLS/AppBind/grant setup and channel/MUX close-and-join. It allows only
one setup and grants no authority. One-shot SourceClock, approval, deadline and
publication checks remain in the original entry. The delegated runner is compiled
outside cfg(test), but material and native consent constructors remain test-only.
Tests use the existing localhost Relay and fixed public identities, not the
previous duplex-only fixture and not a real enrolled phone.

Session retains private SocketCustody before startup. Start obtains the original
join registration guard and checks exact service/session/activation operation
under service.state then network.state. A coordinator waits behind go; its actual
JoinHandle is installed and published to service.network before locks are released
and go opens. Material construction starts only inside that registered task.
Closing Reserved prevents later start. Failed go publication latches Socket and
service Unknown while preserving the original task for joining.

Start retains Session before taking any launch/service/network lock. Locked
checks only borrow that keepalive. All guards are released before its final Drop,
which may close the attempt before go. This prevents recursive last-owner
shutdown under those locks. Original material and release threads are each
registered before their work gate opens. Generated, transferred, released and
explicitly NotSpawned are distinct facts. Cancelled preparation joins its worker
and destroys late material through a separately registered and joined release
thread. Panic payloads are quarantined with Unknown. Cancelled observation cannot
remove handles; Session Drop cannot detach registered tasks from service custody.

SocketCustody wait_closed covers only its original network task and material
work. It does not aggregate the exact NativeHost local token and original App
PeerPump, mint RetirementProof, clear Stage 1 live, replace the clock attempt or
allow a second generation. Those remain Stage 3b-2b-ii. Production key provisioning
and release semantics, phone consent, UI, restart restoration, retries and cloud
deployment remain unavailable.
## Stage 3b-2b-ii: complete original retirement and same-Host renewal (inactive)

The Session now aggregates the exact original NativeHost channel-retirement
fact, original PeerPump join, and original socket/MUX/channel/material task joins.
The Host contributes an opaque Installed-readiness value bound to its original
owner, attempt, channel and fence. It is not a complete receipt or authorization
by itself. Only the Session aggregation module constructs CompleteRetirement;
its fields are private, it is not cloneable or serializable, and no boolean or
from-parts constructor exists. Missing installation, panic, lost/uncertain result
or mismatched custody cannot produce a usable receipt. Cancelling observation
retains all original handles. Unknown keeps custody for full shutdown and never
releases live admission.

One observer owns the receipt slot. A single locked decision distinguishes Empty,
Ready, NextStarting and Unknown. Ready is historical retirement evidence, not a
current permission: later revocation must still reject consumption. Consuming it
changes the slot to NextStarting exactly once. Observing that state returns
Pending without disconnecting the next attempt or mixing the old active channel
with new socket custody. Successful handoff establishes the next observation
epoch under the Session transition lock. A rejected rebind permanently closes
policy/clock and records Unknown after the inner handoff guards have been
released. Here Unknown conservatively means no usable renewal proof remains;
it does not claim that a rejected handoff necessarily performed partial IO.

Renewal preflights the original captured run and retains a sealed NativeRunLifetime
containing the original supervisor/state/input. At commit it reads only their
irreversible lifecycle atomics and original input Complete status. It performs
no resource callback, Weak upgrade, IO, task join or Host rebind under the Owner
lock. The lock order is Session transition, selected clock generation, then Owner.
The original live retirement and next begin happen under the same Owner guard.
All post-consumption failures, including sequence overflow after clearing live,
close the Owner gate and irreversible termination bits before unlocking. Absolute
cutoff, original monotonic deadline, binding, revision and lifecycle are rechecked
at commit. Test-only barriers exercise revocation immediately before commit and
after selecting the new generation; they do not exist in production builds.

The GrantClock Arc/domain remains stable, but each generation has an irreversible
closed bit. Disconnect before first binding leaves an irreversible initial-cancel
fact. An old sample must pass a final exact-generation and closed check after
releasing the Owner guard. No old closed latch is reset. Old channel authority is
explicitly closed before rotation; a late old SocketStart close is identity-bound
and cannot disconnect a newer generation. Host rebind runs outside transition
and rechecks the opaque original Installed-readiness against the same Host.
The original producer is adopted only once and is not checked out or recreated
for the next connection.

The executable success fixture uses two real localhost Relay socket/MUX generations
and the original terminal: remote na, exact receipt settlement, complete retirement,
same-Host rebind, new operation/continuous sequence 2 writing ti, then local ve.
The child's exact native report and one retained producer pin are required.
Original receipt settlement and anti-replay history remain intact. Reusing an
operation with changed payload is a separate negative case, not a retry strategy.
The rejected-handoff state test uses a duplex prepared channel only for fault
injection after a real first-generation retirement; it is not socket/material
join evidence for a successful second generation.

This milestone does not provide production consent, enrollment/key supply, UI,
restart restoration, retry scheduling, real-phone acceptance or cloud deployment.
It supports renewal only after an actually Installed generation and complete
original retirement. The existing 64-attempt/install/retirement/network custody
limits are unchanged, as are WriterHostOwner limits. The current Stage 1 attempt
maximum is still 60 seconds, not a new 15-minute grant. Thus 64 full current
windows cover at most 64 minutes of authorized connectivity; a hypothetical
15-minute window would cover at most 16 hours before the same cap. Neither is
30 days of continuous operation. A later bounded-history reclamation design must
preserve non-reused monotonic IDs, original fences, pending result custody and
credits retained by old Wait handles. No safety cap was silently raised here.

## 容量第一片：Host 已确认历史回收（2026-11-16）

本片不提高 64 的同时托管上限。InstallAttempt 改为进程内不复用的 Host
brand 与 Host 内 checked 单调序号，退休条目使用 checked 单调 ID 和原私有
pin；删除 Vec 元素不会让旧 token 指向新条目。分配器耗尽拒绝，不回绕。

仅在旧 Installed-readiness 已消费、同一精确 scope 的后继已经真正 Installed
且其原响应协调任务成功终局后，才考虑删除更旧整代。最新精确 fence 必须保留，
不能经回收回退到旧授权。旧 bind 接收器、request/response 原任务、pending
结果均须终局；Unknown 不删除。旧 channel 另须原 wait_closed_checked 成功
取得真实完成事实，authority.closed、Drop 和 is_finished 都不替代 join。
Host 局部事实仍不能替代 App 的完整退休 receipt。

GC 单次 poll 原 checked-wait 并取消 Pending future：原实现持 Tokio task
互斥锁跨 join.await，取消只释放锁，不取走 JoinHandle。已有 observer 持锁
时 GC 不会 poll 同一 JoinHandle 或覆盖其 waker；GC 先持锁而取消后，后续
observer 重新登记原 handle 的 waker。两个真实 channel 定向测试分别覆盖
这两个顺序，并要求原 join 成功。

原 token/readiness pin、外部 Connection 的原 pending Owner、外部 Wait 的
原 Cell 都形成保守背压，不是 join 证明。每 Owner 的 live-Cell 计数仅在
最后 Cell Arc 析构时减少；全 Host 共享 credit 规则保持不变。因此即使 Ready
结果已从 registry prune，只要 Wait 仍持有结果，就不能回收这代。调用方持续
保留 token 会在 64 代后得到 Capacity，这是明确的可用性限制，不丢弃结果。

本片只改 Host 历史，未解决 App PeerPump/network/material 容器的长期容量，
未改变 60 秒 attempt、8 秒启动或 grant/材料期限，也不提供生产授权、UI、
自动重试、真实手机验收或 30 天连续运行证明。

## 容量第二片：App 原任务历史

Service admission 的原 reservation/state 锁内只移除 PeerPump 的已成功 join
缓存：joined=1 且能 try_lock 确认原 handle slot=None。join 仍持自己的 slot 锁
跨原任务 await，清空 slot 与发布 joined 都在同一锁内，不反向取得 Service 锁。
GC 不 await，不因 is_finished、取消观察或 Drop 清任务；try_lock 竞争保留原条目。
Unknown 和未消费 handle 的终局任务仍占容量，Service 关闭位不被重置。

已 join 的旧 PeerPump Arc 只可再次观察相同完成事实，没有输入、安装、材料或
重放能力，因此不采用 Arc 引用数背压。Host 的 token/Connection/Cell 可操作
custody 背压规则不变。network 本来已有 terminal=1 + slot=None 的真实 join
回收，本片不改变它的语义。旧网络观察句柄只能关闭其自己的旧 stop 或读取旧结果，
不能指向后来登记的任务。

材料不新增平行 GC：每 Session 仅一个 socket 槽，每代原 MaterialCustody
只有一个状态机。原 coordinator 的 teardown 收齐 channel/MUX 和材料释放，
Released/NotSpawned 才正常结束；Unknown 保留并关闭，不经历史清理恢复。
完整 receipt 后才替换旧代槽的原规则不变。

130 个 PeerPump 任务和 130 个 network coordinator 测试用于有界容器机制验证，
不是 130 次公网/真实手机会话，更不等于 30 天运行。真实 localhost Relay 双代
回归另行执行。生产授权/UI/供钥、60 秒窗口、8 秒启动和绝对最长 30 天均未扩展。

## 预算基础第一片：grant 绝对上限（2026-11-16）

GrantGate 新增只收紧的 limit_expiry：要求传入同一个 GrantClock Arc，在任何
challenge/accept 尝试之前设置。历史 ever_challenged/ever_accepted 不随
pending 清空、失败、browser_attempt 或 revoke 重置。未设置 ceiling 的原
one-shot 行为不变。challenge 保留原 1..900000ms 限制，再与固定 ceiling
取最小且要求 end>now；management 的 issued/accepted/pending 候选及 accept
快返、pending、验签后采样均核固定上限。

此切片只是 security 的收紧能力，尚未接入 App delegated wire；不是更长
窗口已可使用的声明。后续必须由 sealed 原 attempt 提供同域固定 deadline，
禁止把 DTO、当前 wall+重新完整窗口或解析持久记录当权限。

## 预算基础 App 接线与尚未完成的长期输入（2026-11-16）

本地 typed effect 仍为 60 秒；sealed network attempt 最多 900 秒，均受原
30 天绝对总截止约束。创建时固定 wall 与 monotonic 双截止，不因握手、活动或
材料迟到重算完整窗口。NetworkBudget 只能从原 attempt 生成，与固定 GrantClock
同 Arc 绑定；delegated wire 必须安装 ceiling，不能退回无 ceiling 的旧入口。
原 one-shot 路径不变，bootstrap 仍最多 8 秒，本轮真实 fixture 的 outer MUX 2 秒 IO 预算未放宽。

只有总授权、设备/资源、revision 和时间仍有效时，单代结束才分类为
AttemptElapsed：封旧代并保留 live/Retiring，必须完整原任务 receipt 才能续代。
总截止、时钟回退、资源失效和 Unknown 永久关闭。原 network coordinator 内
固定截止唤醒及 100 ms 检查共用原取消/回收；不是新无主 timer，也不承诺同步
立即撤销或操作系统调度下的硬实时上界。

真实 localhost Relay 验证自然短 grant 到期、完整 receipt 后第二代、总截止
不可续、近 8 秒材料/握手仍受固定 ceiling、迟到材料真实 join 后拒绝准入。
15 分钟边界仅由受控测试时钟验证，不是实际等待 15 分钟或 Relay 自动续租。
长测临时公开合成 Relay policy 显式为 120 秒，默认 30 秒不变。

**在预算基础阶段，无续租路径的真实超过 60 秒远端输入当时未通过，不能据该阶段称为长期连接交付。** 原无流量通道
约 1,978 ms 以 Io 结束；Snapshot+ACK 每 500 ms 可维持 61 秒活动，但随后
Window 失败。Host 原 ControlState 控制 lease 独立只有 15 秒，Snapshot 不续租；
已有 renew_lease 仅 crate 内部接口，当前认证 wire 并无续租消息。新增受控
15,001 ms 测试证明 grant 仍有效且快照成功时 Window 仍被拒绝，本机输入保留。
不通过 fixture 私调 renew、不延长 lease、不重置输入 journal 来掩盖缺口。

后续需分别设计双端有界 outer 心跳和已认证控制 lease 续租：前者只维持 IO，
后者仍受 exact lease/fence、原设备/会话、连续序号与固定 grant/attempt 截止。
本阶段未接生产授权、供钥、UI、自动重试，也未验证真实手机或 30 天持续运行。
原全库 PTY 发布门槛未消除，不进行 push/merge/RC 发布。

## 认证手动控制租约（native，生产授权仍关闭）

AppBind 新模式精确要求 canonical-json、control-lease-renew-v1、
grant-management-v1、peer-install-v1，双方完整 offer 进入原 transcript/MAC。
旧 peer/grant/base 工厂不变，能力不匹配拒绝且不自动降级。仅 sealed delegated
wire 选择新 Host 工厂，native 客户端显式选择相同模式。私有认证事实沿原
channel 传递；协议支持不等于 Controller、Installed、用户同意或恢复权限。
浏览器/WASM 配置、TS UI、真实手机与生产 consent 本阶段均未接通。

LeaseState 仅含 profile/request；回复含原 actor 派生的 lease/fence、
notAfterHostMs 和 nextRenewalSequence。LeaseRenew 请求含新 request、原
lease/fence 与 renewalSequence，回复逐项关联同一身份和 Host 确认的截止。
不允许请求选择另一设备、run 或 target；原 DataRecord 的 channel/scope/fence、
活跃 authority、Controller 与 Installed 事实在请求及回复点检查。

原 actor 保留 15 秒 lease：未到期时才允许续租，截止为 min(now+15s, grant.end)。
同 renewalSequence 幂等返回既有截止，不再延长；只允许 next sequence。
重试须使用新的单调 request，但保持原 renewalSequence。NativeClient 额外要求
回复 deadline 严格大于原 grant.created 且不超过 grant.expires，允许 Host 更短。
回复仅是原动作历史事实，不是当前 lease 仍有效的授权证明，不可据此重置期限。
续租不刷新 InputWindow、ACK、receipt、grant、presence 或 attempt，不写 PTY。

LeaseState/续租原 receiver 使用同 attempt 的 retained custody，取消观察不丢
receiver 或原输入结果。取消/超时若已 CloseOnDrop 封闭 channel，就不能在旧连接
重试恢复；必须保留关闭与原完整回收事实。只有应用未交付而原通道仍有效的回复
可按同 sequence 幂等查询/重试。旧能力、Observer、错误 lease/fence、跳号、
重复 request、到期和排队后撤销均拒绝。普通 lease 结束或通道关闭不主动 seal
原本机终端；已有输入的 Partial/Unknown 安全语义不变。

当前只有手动 API，无生产自动续租、心跳或 reconnect scheduler。后续 owner 必须
固定初始 lease/fence，单 outstanding、单调 sequence；不能通过新 LeaseState
重置不确定结果。同 sequence 的成功重试必须保持同 deadline。外层心跳仅保 IO，
必须与控制 lease 和固定 grant/attempt/总截止分开，不构成任何授权刷新。


### Browser 显式手动续租认证模式（仅模式接线）

peerInstall.controlLease 的唯一可选值是字符串 manual-v1。缺失保持原 Peer
协议；存在但 undefined/null、对象、未知值或额外字段均拒绝，Observer 不能
选择此模式。它只是请求原 AppBind 精确能力协商，不是批准标记。Rust 配置、
Browser grant、Entry、PairingProfile/RegistrationPlan 严格校验和显式复制均
保留该字段；不会因已配对或 enrolled 自动开启，也不写入新的持久默认。
exact scope 与 authenticated-host-run 仍互斥，不自动选择后者。

WASM 只由原 application owner 选择 ControlLease 认证入口，peer-only 入口
拒绝；原 transcript、grant、Installed、driver 和最终 NativeClient 权限检查
继续生效。对方旧模式不匹配时握手关闭，没有静默降级或优雅协商的承诺。
Native start/start_peer_pending/start_mode 仅编译在 Native 平台；Browser
仍走原 browser_prepare_data，不能把 Tokio Instant 转作 Browser 时钟。

本片未增加 TerminalClient 手动请求、统一 lease 状态、自动 timer 或心跳。
pagehide/freeze/hidden 仍关闭原 owner，并等待原任务退休；没有解除前台
生命周期限制。实际验证是 Node Worker 执行新编译 WASM、真实 TLS 与 Host，
平台事件和 WebAuthn 使用公开合成 fixture，不是 Chromium 或手机验收。


### Browser 手动 lease owner（原 TerminalClient）

TerminalClient 的 leaseState/renewLease 复用原 request counter、Busy、epoch、
一秒有界调用和断线关闭，不新增 Rust/RPC counter。首次成功 State 固定本代
lease/fence/next sequence；同一连接不能再次 State 重置。Renew 只能使用保存
的精确身份及下一个序号，成功回复必须匹配 request/lease/fence/sequence，
且 deadline 不回退；随后才推进本地序号。不存在公网 retryLastRenewal 接口。
协议层同序号幂等仍要求新 request + 原 deadline，但未知/取消已关闭的本代
不能藉此恢复。u64 边界由原严格 codec 拒绝，不允许 wrap。

这些回复只是原 Host 历史事实，不是浏览器产生的授权或实时有效性证明。
原 NativeClient/Host 仍复查认证能力、角色、Installed、原 grant/租约截止。
浏览器不借 wall clock 重算 Host deadline。lease 操作不会更新 input window、
projection ACK 或 receipt。NeedsSettlement 中可以纯续租，成功后仍恢复该
原阶段；pending/receipt 在失败、断线和晚到旧 epoch 时均不被抹掉。

已用真实 Node-WASM TLS→Host 验证两次手动续租、其中一次处于未 settlement
的真实输入之后，原精确 query/settle 和仅一次写入保持；旧模式、700ms 合成
grant 到期与 freeze 均拒绝后续操作。此项约秒级，既非 Browser 61 秒验收，
也不是无流量空闲、自动续租或 outer 心跳。生产授权/供应/UI 仍未接通。
