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


### 自动续租 A：仅原 TerminalClient 受托管调度

默认 Disabled，必须显式 startAutoRenewal，且本代 leaseState 已捕获、原连接
有 waitClosed、当前非 Busy；Starting custody 在首 renew 调用前登记，首成功
才 Waiting。不存在 UI 默认开启、配对默认开启、独立请求 counter 或重试 API。
手动 renew 与自动 owner 互斥。停止 API 同步封准入/清 timer/关闭原 connection，
返回的 Promise 才等待完整退休。状态为 Starting→Waiting↔Due/Renewing，随后
Stopping→Stopped 或 Unknown；旧 Unknown 不得被 connect 覆盖。

performance.now 固定五秒节拍，每次准入和 timer 都核 Due，错过不补发。
三秒迟到限是固定工程检测界限，不是撤销 SLA 或 Host 权限期限；首 renew
仍受原一秒预算，返回晚于该界也关闭。时钟非有限/回退或严重暂停关闭。
这些 tick 不刷新长期授权、grant/attempt cutoff 或其他设备/范围。

Due 只一个标志，不积队列。原 Snapshot+ACK 完整结束后才交棒，在下一普通
请求准入前优先续租；input/query/settle 各自完整回复处理不能拆开。续租成功
恢复原阶段，NeedsSettlement 与 pending/receipt 不变。当前没有自动重连。

每次成功 enter 都有 finally。退休同时保留原逻辑 completion、原调用 Promise
及绑定原 this 的 waitClosed；timeout race 不等于原任务结束。内部失败只
requestStop，不能 await 自己。外部 close 的同步重入发生前已固定退休 latch。
原 Promise 永不 settle 仍 Stopping，waitClosed 拒绝为 Unknown，不伪称 Stopped。
旧 timer 和旧逻辑 completion 不能影响新 epoch。

本片真实 Node-WASM TLS→Host 只验五秒自动 tick、原精确输入/结算和退休。
期间 Snapshot 是测试活动，不是 heartbeat；不包括 outer Relay 数据心跳、
纯空闲、手机后台、生产供应/UI 或三十天 endurance。manual-v1 不代表心跳协商。

## B1：独立 LiveControl 认证事实（机制未激活）

显式 live-v1 选择新的 BindMode::LiveControl，必须双方精确提供
control-lease-renew-v1 与 data-keepalive-v1。协商裁剪副本仅复用旧校验；
MAC/transcript/binding 始终使用原完整 offer。旧 manual-v1、缺省 Peer
和 one-shot 不升级，mixed mode 不降级。

NegotiatedDataKeepalive 仅 bind 模块完成认证后构造，沿原 ManagedChannel、
PeerStart、BoundDataAuthority 传递。getter 只是协商事实，不能替代当前
authority、Controller grant、Installed 或截止核验。Native grant install
与 Browser 严格配置均拒 Observer；Browser 还必须 application 模式。
配置不是授权、不由 pairing 推断，原生命周期封口不变。

本片没有心跳 timer/transport activation，没有自动启动 A，没有 UI 或生产
授权。B2 后续提供一次 Dormant 机制；C 仍需另审原 Installed 与当前授权
事实交接，不能拿 peer_deadline 或布尔 getter 激活。

## B2：一次性 Dormant transport 机制，尚未认证接线

Rust start_admitted_data_keepalive_dormant 返回原 stream/OwnedTunnel 和
不可 Clone 的 activation；Browser createBrowserDataKeepaliveRelay 返回原
owner 及原实例独占一次 activation。它们不依赖 security，不是认证事实
或控制授权。没有生产调用方，C 尚未启用，live-v1 也不自动启动 A。

Dormant 保留原 setupEnd；只有 setupEnd 前可一次选固定 end<=maxEnd。
最大机制 horizon 900 秒、显式 IO budget 至少 2 秒，固定 500ms 节拍。
重复、迟到、已关闭和超界不得重启；原 read/write/等待均裁剪固定 end，
持续入站也不能延长。原 registration 的 120 秒/预算三分之一策略不变。
Rust close/driver exit 均封原槽，仍由原 OwnedTunnel 的真实 join 终局。
Browser live 检查有限时钟/安全整数期限以及时钟回退，原物理 closed 和
原 signer/write/等待 Promise 才构成退休；Drop/发送成功不是回收证明。

原唯一 writer 发送同 tunnel 的非空 WS Binary（outer payload 零字节），
不回声，不进入 TLS，不刷新任何 grant/lease/presence/receipt。空 payload
先严格校验路由及重复帧，再跳过应用 queue 容量占用。忙 tick 不补发。
Browser 最多一个原应用 write 等正在 drain 的 heartbeat，第二并发应用
写拒绝；等待从调用时固定预算，不给新 heartbeat 插队，不无界排队。

激活若收紧已有发送的期限，Rust 保留并 pin 同一原 send/flush future，
监听变更后重算 deadline，不取消再重发。Browser 原 Promise 每个执行点
动态 min 当前 end，不依赖 timer 及时触发。原在途写等待只收紧，不借
激活延长其已捕获期限。已开始的同步 socket.send 不可抢占，仍需原关闭
和退休证据。

本片测试是原 MUX 内存 WS 与 Browser-shaped Node socket 的机制验证。
额外实际 WSS→Rust oracle 是旧路径兼容回归，不是新心跳 Relay/WASM
组合或手机后台验收。C 仍需真实当前 Installed/authority 事实与固定时钟
交接，绝不能仅拿 getter、peer_deadline、配置或 JS bool 激活。


## C1：原 Host Installed 事实激活原 MUX

本片仅为封闭 delegated Live 路径接线。默认、manual-v1、原 one-shot 不升级；Live 材料选择仍只有测试构造，生产 supplier/consent/UI 未开放，Browser C2 未接线。

- LiveKeepaliveFact 仅从原 RunningChannel 缓存 Installed 产生，保存精确原 authority、peer 与 fence；要求 Live + Controller + expected local role，Terminal 不回到 Installed。缓存只读，不消费 PeerPump 原证明或事件。getter 不授权输入。
- 原 NetworkWire 托管唯一 Dormant activation、channel 与 MUX。handoff 后在原 8 秒 setup/cancel 任务内等待 Installed，每 100ms 检查缓存；无额外后台任务。Host 激活要求 Host role，原 NetworkBudget clock 与 fact 同域。
- sampleBefore 先于 GrantClock 采样；固定 end 取 sampleBefore + 当前 grant 剩余与原固定 NetworkBudget deadline 的 min。激活前后重核 authority、预算、域和原 stop；activation 只能 take 一次。任何失败走原 cancel/close_and_join/material release，不新建回收 owner。
- 真实本地 Relay fixture 的两端使用原 socket/TLS/认证 Live channel 后分别激活原 MUX。无 Snapshot/ACK/LeaseRenew/输入等待 4 秒，超过本轮 fixture 的 2 秒 IO 预算后仍有效；固定总授权 6 秒到期后原 socket/MUX join，不能 prepare_next，本机原终端仍精确消费 native。此证据不是 Browser/WASM/手机或 61 秒低流量验收。

首轮 5/6：空闲用例错误调用终局 report()，该 helper 等 ProcessLifecycle::Exited，而原 input probe 必须收到至少 6 字节才退出，造成 D15 lifecycle did not converge。修复只改为空闲观察原进程未退出，后续精确 native 字节报告排除任何额外输入；未扩大超时或降低协议断言。首轮日志保留。


## C2：Browser 原认证事实到原 port 的一次保活激活

显式 live-v1 才在 Window 创建原 Dormant Relay activation；manual/default 不创建该能力，不启动自动 lease owner。生产 Host supplier/consent/UI 仍关闭。

- WASM 原 NativeClient loop 的私有 kind4 只收空 payload，重新取原 Client + Controller + Live + Installed/current fact，返回恰 8 字节 little-endian u64 剩余毫秒（1..900000）。身份、scope/grant/fence 关联由原 NativeClient/channel 实例、原唯一 RPC sequence 与封闭 adapter 保持，JS 不提供身份或 approved 标记。
- 原 adapter singlepending/sequence 不拆分。live descriptor 发布前只允许私有 kind4；所有直接 snapshot/exchange/limitDeadline 入口均拒绝，不占 seq/pending。fact 已回但 ack 未回的空 pending 窗口也关闭；收到 ack 后末端检查取消/固定期限才开放，首普通操作保持 seq=2。
- 原 transport MessagePort（一次 claim）内使用同一 seq 的 prepare→challenge→commit→ack。Window 先置 Pending，再采 sampleBefore，再发 challenge；异常永久 Closed，不回 Dormant。Worker 在 challenge 后调用原 fact RPC，remaining=floor(min(fact remaining, 原 peerExpiry - 当前时间))，不能重读重置 peerExpiry。
- Window 预存 setupEnd=floor(原 setup deadline)、maxEnd=floor(创建时 monotonic epoch+900000)，不重新计时。固定 end=min(maxEnd,sampleBefore+remaining)。校验 safe integer、时钟回退、原 seq、一次槽与当前期限；消息延迟都从原 sampleBefore 扣除，handoff 最多 1 秒且不越 setupEnd。只是原 transport 保活机制，不授予或刷新 grant/presence/input 权限。
- 原控制 Promise 先登记后调用 fact，取消不会 race 丢弃 RPC。Rust run(&client) 真实返回后的专用 invalidated import 先 terminal 原 pending，再 client.retire；普通 JS invalidate/close 仅封口。这样不形成 control→RPC pending→transportDone→control 环。实际 WASM export 调用抛错时先 latch rustDone/runtimeFailure，再 terminal/关原 IO/等待物理退休，最终 Unknown；普通 JS 网络错误不伪称 Rust 已结束。
- 原唯一 writer、B2 500ms 空 outer subframe、固定截止和物理 close/join 不变。消息不进入 TLS 应用队列，不启动 A，不做回声响应或期限刷新。

实际验收使用 Node 合成 Window/WebAuthn + Dedicated Worker，公开固定测试密钥；新 WASM→原 browserRelay→实际本地 Relay→原 App Native Host。无 Snapshot/ACK/LeaseRenew/输入等待 4 秒超过 fixture 2 秒 IO，原总授权 10 秒截止后真正回收，最后原本机终端精确 native 字节无额外输入。这不是 Chromium/手机/公网或 A 低流量 61 秒验收。


## Production A1: historical installation confirmation

The real registration startup now retains a pending context only from its own
successful persist_native mutation and returned snapshot. Pairing entries gain
optional version-1 enrollment evidence (Pending or Confirmed), canonical public
record digest and the revision at which that evidence was written. Missing
evidence in an old schema-2 document remains Unverified. Skipping an absent field
during serialization preserves old canonical bytes/digests; reading does not
rewrite or upgrade them. Older readers reject the new field via their existing
strict schema rather than silently interpreting it. This is not bidirectional
format compatibility.

Only the original authenticated registration delivery, inner ACK/EOF/shutdown,
outer tail drain and final authority checks can produce the private completion
marker. Startup combines it with the exact retained store object, successful
mutation's device/digest and expected revision. The completion value cannot be
cloned or deserialized. A separate consuming PairingStore confirmation method
uses the original backend transaction and authorization rechecks; no cloneable
Mutation::Confirm or caller-approved flag is introduced.

Confirmation requires the exact old ExpectedStore CAS, including when an unrelated
device changed meanwhile. It records the resulting incremented revision, not the
old expected revision. Later historical reads allow the evidence revision to be
older than the document revision after unrelated changes, but selection and any
future admission must use current state. Forget clears the evidence; remembering
a revoked device starts Pending again. Wrong digest/version/revision or a revoked
record retaining evidence is rejected.

Confirmation and the final InstalledLocked reply now share the original
AttemptCommitGate. A cancellation that wins the gate prevents both; cancellation
after publication is ordered after that completion. A final authority failure
cannot be overwritten by a late successful reply. A committed historical
Confirmed record does not become current authority if cancellation occurs later.
An Unknown publication poisons only that store instance. Reopening may reconcile
a completely written Confirmed record as historical data, never as a permission
or a replay instruction; this is not a cross-process durable poison guarantee.

No production unattended switch, delegation consent, reconnect owner, restore
resolver or network authority is enabled by this stage. The new evidence is an
installation prerequisite, not 30-day consent. Process-lifetime exclusivity,
global status/revoke without a live run, journal revocation reserve and safe
observation cadence remain independent gates before enabling unattended access.

A1 verification: 34 distinct native tests passed (NativeEnrollment 3,
NativePairing 16, NativeRegistrationStart 14, original fresh-run product/Forget 1).
Actual legacy Edge/Worker/WASM/Relay scenarios passed 4/4: successful remembered
registration now reopens Confirmed at revision 2; native save No creates no
record; save followed by Host installation approval No reopens Pending at
revision 1; the original non-remembered success path also passes.
The original browser-entry transport boundary remains the A0 boundary above,
not real-phone or rendezvous-v1 qualification.

Same-source logs: unattended-enrollment-negative-final2, pairing-final,
start-final, product-final, legacy-final, nonremember-final and strict-final.
The last is remote-host lib/tests Clippy with -D warnings. The first genuine
feature RED (unattended-enrollment-red) reached successful original registration
and failed because enrollment.version was absent. Moving final reply publication
outside the cancellation gate caused the exact barrier failure in
unattended-enrollment-gate-red; the source was restored and recompiled before
final GREEN runs. The old-bytes fixture's first failure was a protected-file ACL
rejection, corrected by creating its test file through the original backend,
not by changing ACL policy. Independent implementation review found and closed
the final-publication race and required the second publication-fence test.


## Production A2: installed-device candidates, not permission

InstalledDeviceCandidate is a private-field, non-Clone/non-Deserialize selection
produced only while the original PairingStore reads and validates its current
snapshot under the backend lock. It requires exact ExpectedStore CAS, an Active
device and Confirmed historical installation evidence. Missing, revoked,
unverified and pending devices have explicit blocked results. Public records or
caller-provided hashes cannot construct this candidate.

Revalidation requires the same store instance and current exact CAS, then checks
the record digest and confirmation revision again. An unrelated write invalidates
the old candidate too. Dropping that store and reopening the file creates another
instance that cannot revalidate the old candidate. There is no new independent
Closed notification/state. Unknown publication poisons that instance and prevents
candidate revalidation. A successful revalidation is only a fact about that
locked read, not a promise of continued validity; future consent/publication
still needs its own exact CAS and final owner/trust checks.

The existing authenticated pairing-list response, TypeScript DTO and Host panel
now distinguish Unverified, Pending and Confirmed installation history, while
remaining Locked. Pending means completion has not been confirmed locally, not
that the phone definitely failed to install. Missing/unknown UI status is rendered
as unverified. Confirmed is explicitly labelled historical and is not unattended
authorization. The old manual selected/freeze path is unchanged.

This stage neither loads CNG keys nor constructs NativeConsent, creates a network
owner, selects a live-run target or restores anything across App restart.
HostKeyProfileIdentity must not be substituted for a CLI launch-profile identity.

A2 verification: missing installation display first failed all three new component
cases, and the real Windows store-to-product projection failed Null versus
pending. Final same-source results are 35 native tests (candidate/enrollment 4,
pairing 16, startup 14, fresh-run product 1), 40 component tests, full npm typecheck,
four actual legacy registration scenarios and strict remote-host lib/tests
Clippy. Logs use the unattended-candidate prefix (native-final,
NativePairing_-final, NativeRegistrationStart_-final, NativePairingProduct_-final,
ui-final, typecheck, legacy-final and strict-final). The actual successful
registration oracle selects and revalidates its Confirmed candidate; the
post-save native installation-approval No returns Pending/blocked instead.
Tests also cover stale revision, missing/revoked devices, foreign/reopened store
instances, Unknown publication, old Unverified bytes and conservative unknown
UI values. Component tests are not a Windows WebView2 visual/IPC acceptance test,
and the local legacy fixture is not real-phone or production rendezvous coverage.

## A3: local single-run approval preparation (not network authority)

The native approval child module consumes an A2 installed-device candidate and
an original NativeRunAdmission. It retains the original network_checks closure
(including RunAccess/document/caller checks), exact supervisor/state/resource/run
origin and native scope. A new document for the same textual run cannot substitute
for that original access. The candidate is revalidated against its original store
instance and exact current CAS before preparation, before the native prompt and
when consuming the approval. Preparation holds no producer and does not perform
delegated capture or sever document access.

The private original ApprovalMachine keeps its 30-second pending and at-most
300-second single-consumption window. A separate requested absolute cutoff must
be positive and at most 30 days from native wall sampling; a fixed monotonic
upper bound is frozen at preparation. This does not lengthen the original
connection approval. A short private clock lock samples and advances the greatest
observed wall/monotonic values, latching observed failure or rollback for this
instance. It does not detect unobserved clock excursions, survive restart, or
replace the durable unattended journal's clock contract.

The separate native prompt explicitly says this stage neither enables a network
connection nor persists unattended authority or restart recovery. The returned
non-cloneable fact retains the original cancellable machine identity and short
consumption end. A check is only a current observation, not a continuous trust
lease: store I/O is followed by original run/clock checks and then final approval
lease/deadline checks. Consumers must revalidate rather than treating this fact
as a lasting authorization. No IPC command or real device is enabled here.

Tests compose a TEST_ONLY Confirmed record in an isolated Windows store with the
original Node/PTY native-run fixture. That fixture is not installation proof; the
separate A2 actual registration oracle proves the registration-to-candidate path.
The tests do not display a real native approval dialog or touch user credentials.
The first four native-run cases passed, then a blocking Windows backend wrapper
exposed three genuine tail-check failures (cancel, short expiry and native run
shutdown during read). All three failed the final behavior assertion only after
worker join and original cleanup, and all three passed after tail checks were
added. The logs are unattended-preparation-first, tail-red and tail-green.

Before any production delegation can be enabled, lifetime-exclusive persistent
owner custody, reserved durable revocation/journal capacity and observation
cadence, current native-key/trust verification, and exact restorable CLI/workspace
target resolution remain separate gates. Host key-profile identity is not a CLI
launch profile. This preparation cannot bypass those gates or produce
NativeDelegationConsent/VerifiedBinding/network material.

A3 final verification: 48 distinct native tests pass on the final source: eleven
UnattendedPreparation cases, eleven HostApproval cases, sixteen NativePairing
cases, four NativeEnrollment cases and six DelegatedNative cases. Strict
remote-host lib/tests Clippy also passes (25.33 seconds). The final logs are
unattended-preparation-final2, unattended-preparation-{HostApproval_,NativePairing_,
NativeEnrollment_,DelegatedNative_}-regression and unattended-preparation-strict-final.
The cancellation gate contains no store, run-resource, canonical-scope or clock
checks; those happen before publication and must happen again at later use. The
store-read and original-run-lock barriers verify cancellation can return before
those locks are released in these fixtures, not absolute scheduling guarantees.
No full native-dialog/WebView acceptance, real phone, public route, persistent
owner startup, or unattended network connection is claimed by this stage.

## A4: exclusive OS lifetime custody (not a restored owner)

WindowsUnattendedBackend can claim a non-cloneable OwnerCustody using the existing
permanent lifetime.lock byte interval [0,1). Claim briefly holds the independent
control.lock interval [0,1), validates the original root/journal identities, takes
an exclusive nonblocking lifetime lock and releases control. Ordinary transactions
still take control exactly as before: no reentrancy, bypass, ACL change, new lock
file or journal-format change is introduced. Custody exports no lifetime witness.

Custody retains pinned root/lease handles, the lifetime lock, journal identity and
same-process poison observation. A check against a backend reopened on the same
NTFS root/lease/journal identities is allowed; a foreign root is rejected. This is
OS resource custody, not the A2 candidate's Store-instance authority. Check opens
its own short transaction and must not be nested inside an existing control
transaction. Known poison rejects an existing custody check and a new claim.

Claim does not parse or attest the journal chain, initialize a missing document,
prove Enabled, or create an authorization. A fresh empty store can be held without
writing its blob/journal. Original Store transaction validation/CAS remains
mandatory before any future recovery. Other ordinary transactions retain their
existing behavior; no cross-process live-owner revocation notification is added.
Dropping custody or exiting the holder process only releases OS custody and does
not change Disabled/Enabled/Revoked history. No production network owner uses this
capability yet.

The real isolated NTFS tests hold custody during original observe/revoke
transactions, reject a second object/child process, reject a foreign root, retain
exclusion after dropping the original backend, and permit reacquisition after
Drop or the dedicated child calls process::exit(7) without Rust destructors.
The child is bounded; timeout kills only its owned Child handle, waits and joins
its reader, then fails the test. This is not a guarantee about every possible OS
crash or storage failure. A temporary shared-lock mutation caused all three
competition cases to fail, including the actual child assertion. The mutation was
restored and the final exclusive source rebuilt; the shared-red log is retained.

The remaining journal gate is unchanged: 224-byte records, 8192 records and two
records per commit allow at most 4096 commits. Current observe persists each
changed wall value, even when Disabled/Revoked; equal wall values do not write.
Revocation currently consumes the same capacity and can fail at a full journal.
Production observe cadence is not wired. Reserving the final revocation pair and
closing before exhaustion is a separate planned fail-closed guard, not evidence
of 30-day usability and not permission to reduce clock checks, reset the journal,
or discard history. An old full Enabled record cannot be claimed durably revoked
merely because an in-memory owner is blocked.

A4 final same-source verification: five custody tests and the original 29 store
tests pass (34 distinct tests). The dedicated ignored helper is actually invoked
as child processes by custody case 003, not counted as an unrun end-to-end proof.
Strict remote-host lib/tests Clippy passes (27.55 seconds). Logs:
unattended-custody-shared-red, unattended-custody-final,
unattended-custody-store-regression and unattended-custody-strict-final.

## A5: reserve durable revocation capacity

The A4 capacity limitation is now guarded, without changing the journal format,
8192-record upper bound, or existing wall-clock observation semantics. The
transaction's remaining_commits fact is available only after its original
read_verified has validated a complete clean Chain under control. Unread,
Pending/torn, poisoned, or consumed transactions cannot supply usable capacity;
this is not a caller count or a raw file-length division.

Every Store publication that would leave Enabled requires at least two complete
commit pairs: this write plus one reserved closure. This covers enable and
observe through the shared publish boundary. With one remaining pair, observe
on Enabled closes to Revoked in the same exact-CAS transaction, advances revision
and authority_revision, and persists the maximum observed floor. The capacity
closure is checked before the same-wall fast return. With two pairs, normal
observe/enable still succeeds; the last pair remains available for explicit
revocation. Clock rollback and expiry retain their original closure precedence
and reason, and all publication failures take precedence over those reasons.

CapacityRevoked means only that this capacity-triggered transaction successfully
committed Revoked. Capacity means this call did not establish such a durable
closure (for example an old completely full Enabled record, or a rejected enable
that would spend the reserve). Unknown remains an uncertain write. These outcomes
are not authorization capabilities, and an in-memory store gate being blocked,
a durable Revoked record, and full connection retirement are distinct facts.
An old full Enabled record remains unchanged on disk, is blocked even for a
same-wall observe, and must not be presented as successfully revoked or restored.

The isolated NTFS boundary fixture constructs codec-valid full history, rather
than claiming 4096 real approval/commit operations. Its first RED produced three
behavior failures: enable and observe spent the final pair leaving Enabled, and
a full Enabled record returned a successful same-wall observation. Typed results
were then added, with final cases for positive two-pair behavior, explicit revoke,
clock/expiry, uncertain publication, and Pending/torn capacity rejection.

There is still no automatic production observe call or live-session revocation
notification. Per-call floor persistence is not reduced, no old records are
reset/discarded and the limit is not raised. This is fail-closed damage prevention,
not a 30-day availability claim: high-frequency durable observations could still
exhaust the journal and force an early durable closure. The production owner and
its observation/connection lifecycle require a separate reviewed contract.

A5 final same-source verification passes 44 distinct native tests: capacity 10,
original store 29 and custody 5, with the dedicated custody helper actually run
as subprocesses. Strict remote-host lib/tests Clippy passes (28.19 seconds).
Evidence logs are unattended-capacity-red, unattended-capacity-final,
unattended-capacity-UnattendedStore_-regression,
unattended-capacity-UnattendedCustody_-regression and
unattended-capacity-strict-final. The independent review found no stage blocker.

## A6a: original Host key-worker readiness (0.28 development)

HostKeyPublisher already releases each original KeyStart after storing its slot
and JoinHandle. The worker immediately runs ExistingKeyLoader; Dormant refers to
the signing stage, not lazy opening at first signature. This phase preserves
that prepare/signing/retirement behavior.

Only a successful original ExistingKeyLoader fill, including purpose, public
SPKI and native properties, can publish a private opened fact. Publication checks
the original control state, revocation token and fixed cap again after fill.
Failure, panic and the test-only Fixture factory do not publish that fact.
HostKeyCustody offers a nonwaiting take_ready observation: both original slots
must be ready before one non-Clone aggregate receipt is issued. No observer task,
extra signing request, timeout-as-retirement or network activation is introduced.

The receipt keeps weak original core/worker brands, matches each purpose/SPKI,
the exact cancellation token and cap, and checks the complete original frozen
Host profile binding on use. It does not export key bytes or a lifetime-extending
witness. Check/consume revalidate current original worker state; cancellation,
expiry and retirement invalidate the receipt. This is a current observation, not
a guarantee of future system key registration, paired-device trust, WebAuthn
installation, CLI target permission or any grant. HostKeysReady cannot enable
network access or restore unattended authorization.

The effective behavior RED is unattended-key-ready-behavior-red2: both isolated
original loaders completed and were safely retired, but the live observation
could not obtain a receipt before publication was implemented. Earlier compile
and canonical-origin fixture failures are retained separately and are not RED
evidence for this behavior. Seven new tests cover live one-time issuance,
first-key ready with the second blocked or rejected by the actual loader,
post-validation cancellation/expiry, foreign custody/profile/purpose binding,
receipt invalidation after retirement, actual loader purpose rejection and
successful Fixture exclusion. They use fully intercepted MockKeyApi or empty
handle slots, never real user keys.

The restored 0.28 work remains on its independent branch; no A6a changes were
included in 0.27. Existing public-vector CNG regressions, when run, import only
repository-published fixed test parts into unnamed, nonpersistent slots and use
the original retirement path. Those tests do not validate a user's CNG key,
phone, production permission, UI or long-term owner. The production service
combining consent, durable custody/store and exact-run capture remains a
separate reviewed step; default authorization stays off.
A6a final same-source verification passes 48 distinct tests: new HostReady 7,
original HostOpen 11, HostKeys 15, HostStages 12 and CngOwner 3. The initial
17-test restored run overlaps the final inventory (10 original HostOpen plus
the same 7 new tests), so it is not added again. Strict remote-host lib/tests
Clippy passes (58.75 seconds). Logs are unattended-key-ready-final,
unattended-key-ready-regression, unattended-key-ready-hostopen-final,
unattended-key-ready-HostKeys_-regression,
unattended-key-ready-HostStages_-regression,
unattended-key-ready-CngOwner_-regression and unattended-key-ready-strict.
This is targeted coverage, not the complete crypto or Rust suite. Project
lessons L02/L04/L06 were read from 9add5e02 without merging that documentation
branch, preserving ownership, same-source and historical-fact distinctions.
## A6b internal single-run service (production entry remains disabled)

This is a new v1 service definition, not a claim that the earlier model already
defined an installation identity. The existing pairing UUID identifies a pairing
document namespace only. A domain-separated digest of that namespace may be used
with the original Store instance, OS custody and exact run brands; it is not an
OS installation identity or copy/rollback protection.

A separate final native confirmation is required. A3 explicitly approved only
preparation, and must never be silently upgraded to durable Enable permission.
The final prompt must identify the device, original run/generation, exact role,
actual effective cap, durable Enabled write, no restart restoration, and the
difference between local closure, durable revoke and Unknown. It reuses original
bounded approval timing but has an independent, explicitly approved final stage.
Its cancellation epoch and short publication gate are shared with the service.
Blocking store/run/key checks stay outside that gate; only final lease/end/epoch
checks and the one-way Starting -> Active/Closed publication occur inside.

The implemented durable format is schema 2 with the exact mode
single_run_no_restore, included in canonical integrity and the journal binding.
Schema 1 remains historical Locked and cannot be upgraded, rebound or enabled
by this service. Unknown or mismatched modes fail closed. Loading a schema 2
Enabled record never reconstructs a policy or opens a gate; crash residue is
Locked, not falsely described as durably revoked. A fresh enable refuses such
residue; any later cleanup requires explicit exact-CAS closure.

The runtime binding must compare the full canonical public record digest, not
merely put it in the durable target. Explicit key slots are native endpoint,
native relay, browser TLS and WebAuthn; issuer is the domain-separated grant
SPKI fingerprint. The full-record digest also binds browser relay, credential,
profile, epochs and routing data. Confirmation revision comes from the original
candidate's historical confirmation revision, distinct from the current store
CAS revision. Exact session/run/generation is versioned and length-encoded, not
delimiter concatenated. These digests supplement, never replace, original
instance/run/worker identities and current checks.

The internal start sequence holds the gate closed through final confirmation,
original custody/key/candidate checks, durable exact-CAS Enable, original capture
and tail revalidation. Neither capture nor VerifiedBinding escapes before final
publication. Original document/caller network_checks are required before capture,
at the Enable tail and before final publication. After explicit delegation is
published, the captured owner follows the original exact run/resource/scope,
trust, local revocation and deadline lifetime, not the obsolete WebView. This
preserves DelegatedNative_DocumentRetirementPreservesOnlyCapturedEffect_001 and
the existing design's explicit transfer of ownership away from that document.
Any failed step irreversibly closes the policy first. If Enable was known to
commit, compensation uses only its exact returned CAS to persist Revoked; a
conflict must not overwrite another revision, and Unknown cannot be reported as
successful durable revocation. Crash after Enable is handled by the no-restore
read boundary, not by pretending compensation always ran.

Disable closes the live gate before disk work and retains the original
retirement owner. The internal Forget path must close first, then perform the
original pairing tombstone transaction; old external Forget paths cannot be
left as an uncoordinated production bypass when UI/network entry is eventually
connected. Expiry, trust change and any uncertain effect fail closed. A managed
background check, if introduced, needs explicit stop/join ownership and no
detached Drop path. Polling cannot substitute for the shared cancellation gate.

This first service must not expand the current 60-second frozen key cap into
30 days. It must disclose and enforce its actual cap and reject incompatible
requests. The 4096-commit journal remains a separate availability gate: every
existing durable observe keeps its floor semantics, reserved revocation capacity
is retained, and neither history reset nor silently reduced persistence is
allowed. Early capacity revocation is not proof of 30-day operation. There is
no UI, IPC, network activation or cross-process restoration in this slice.

Required real-composition RED cases include cancellation during Enable I/O,
after Enable before capture, and after tail checks before publication; capture
failure with exact compensation; compensation CAS conflict/Unknown; expiry and
trust changes; document invalidation before publication versus replacement after publication preserving only captured effects; original key/task retirement; and an owned subprocess exiting
after durable Enable followed by a reopened Locked store. The tests must use
explicit final-stage approval, isolated stores and public/mock test keys, not
production registrations or a test-only authorization shortcut.
### A6b implementation and evidence boundaries

FinalApproval is independent of A3 preparation. Its sanitized final description
includes the complete selected public record and record fingerprint, original
run/generation, role, route/origin/RP and actual bounded cutoff. It creates no
network entry. The service owns original key custody/signers and consumes the
original ready receipt against the exact frozen binding before Enable. Original
capture follows the committed Enable; current durable revision/full document
is rechecked before publication, followed by original run/device/key checks.
The transaction is released before the short publication gate, so a concurrent
external write can still occur in between. Case009 proves its injected timing,
not linearization with every external revoke. Shared revocation entry and a
cross-process protocol remain prerequisites to production activation.
The short final publication gate still shares the original cancellation epoch.

An exact-CAS compensation conflict leaves the newer record untouched and closes
the local gate; it is not reported as durable revocation success. is_published
only reports publication state, never continuing effect authority or UI status.
Every effect uses fresh candidate/key/run/deadline/durable checks. No background
watcher is installed, so proactive expiry notification is not claimed.

Normal service destruction retains OS lifetime custody until original key and
notification joins and captured/ticket destruction. One process-wide slot is
reserved before start. An uncertain effect keeps the original policy Unknown,
snapshot, keys and OS custody in one bounded process-lifetime quarantine; new
services are refused. No detached task, repeated authorization or history reset
is introduced. advance_quarantine advances original key retirement. It may
release only the private captured resource reference after sticky closure, a
non-poisoned original effect mutex observation and original writer Joined.
This means delegated synchronous calls are quiescent, not that unknown input
results became successful or EffectsClosed. WouldBlock, poison and an unjoined
writer retain the reference. Policy Unknown and the custody/slot remain until
process exit even after the original run resources can be closed.

The first quarantine test failed with output_reader_joined=false and
original_io_drop_returned=false while the original keys had already joined.
Those failures remain in quarantine-first, quarantine-diagnostic and
quarantine-observation logs. The corrected fixture starts original supervisor
shutdown and alternates its observations with quarantine advancement. Before
process exit it requires direct child reaping, output reader/wait/stop joins
and original IO Drop return; its observed io_threads_joined=false remains an
explicit whole-resource Unknown, not a claimed complete platform retirement.
The original observe_run Unknown constructor always sets io_threads_joined to
false; it does not identify a particular still-running thread. IoDropWitness
attests Rust destructor return only, not kernel close or all ConPTY internals.
The checked original joins therefore do not establish complete platform exit.
Only then does owned child exit release OS custody; reopening retains a Locked
Enabled history and cannot restore authority. The 20-second child deadline
was not enlarged, and forced child termination is a failure, never success.

Effective negative evidence includes the original custody-order RED and two
reversible mutations: omitting compensation fails case006 mode0 with revoked
false; omitting the final publication gate fails mode1 after cancellation with
Ok and revoked false. Each mutation restores the source byte-for-byte with a
SHA256 record. Final owner-final4 runs ten schema/composition cases, including
actual Node/PTY effects after document replacement, failed capture/cancellation,
concurrent durable revocation without overwrite, original key exit barriers and
the isolated quarantine child. Case010 explicitly identifies the second seal
as Enabled, cancels the original final approval, then returns normal ciphertext
to the original Windows transaction authorization fence. It proves no published
service, unchanged Disabled revision 1 and original key/run cleanup for that
seal-completion-to-commit timing, not every disk-I/O cancellation window.
This uses synthetic Confirmed pairing fixtures,
TestSealer and original ExistingKeyLoader through MockKeyApi, not a real phone
registration, real user CNG key or production WebAuthn approval.

The requirements above remain the production-entry checklist, not a claim all
are finished. In particular, the existing Forget dialog explicitly does not
revoke independent connections and is not silently repurposed here. A new
accurately confirmed close-first deletion path, shared trust/revocation service,
managed expiry/check scheduling and app shutdown advancement must be integrated
before UI/network activation. Service-specific post-publication Unknown and later disk-I/O
cancellation windows are not yet new composition cases; original
store fence/Unknown tests remain separate evidence. The 60-second cap and
4096-commit availability limitation remain, with no claim of 30-day operation.
No real device, network, cloud deployment, default UI or restart recovery is
enabled by this internal stage.

Project lessons were read from main 2d65d236 without merging it. Two L06
corrections matter here: a preparation approval is not durable Enable consent,
and explicitly captured delegation is not the old WebView capability. An
unknown effect outcome is also distinct from a currently executing call; the
narrow quiescence observation does not clear Unknown or release OS custody.

Final same-source verification passes 111 distinct tests: SingleRun 10, Final
approval 2, Preparation 11, DelegatedGate 2, Store 29, Capacity 10, Custody 5,
original model 13, DelegatedNative 6, NativePairing 16 and HostReady 7. The
Custody helper is ignored as a standalone test but is invoked by its owned
process case. Repeated earlier runs are not added to this count. A mistaken
remote_unattended_tests filter ran zero cases and is explicitly excluded;
the correct Unattended_ filter subsequently ran all 13 original model tests.
Strict remote-host lib/tests Clippy passes on this source (25.90 seconds).
Final logs are unattended-single-run-owner-final4,
unattended-single-run-final-{UnattendedFinal_,UnattendedPreparation_,
DelegatedGate_,UnattendedStore_,UnattendedCapacity_,UnattendedCustody_,
Unattended_,DelegatedNative_,NativePairing_,HostReady_} and
unattended-single-run-strict-final2. This is targeted regression, not the full
Rust suite or a release qualification. Original unrelated browser/network
acceptance changes remain outside this stage commit.
The exact final test-name ledger is unattended-single-run-final-ledger.json.
It uses only the eleven explicit final logs above, counts the nested nocapture
case008 name once, excludes the standalone ignored helper and verifies each
group against its final passed count. No historical approval log is included.

### A6c: internal close-first controller and separately approved pairing deletion

This stage adds an internal, uniquely owning SingleRunController. Its cloneable
CloseHandle can only close the original FinalControl; it cannot read or write a
run, change targets, reopen, or obtain network authority. Closing is a reduction
of authority and does not require the old document/run to remain alive or another
Enable approval. request_close closes that gate before acquiring the service,
effect or store locks; finish_close then attempts the original exact-CAS durable
revoke. A closed local gate is not itself proof of persisted revocation.

Pairing deletion is a separate, accurately worded native approval, not a new
meaning silently attached to the existing Forget entry. The request is produced
from the started controller and current pairing store. Its sealed controller
brand, original store instance/namespace, device, current record and exact CAS
cannot be supplied as an arbitrary mutation. The prompt includes the actual
record and original run, generation, role and cutoff. The new action validates
its current document/caller and original short approval lease. Its publication
lock invokes only the matched narrow close handle, never a caller-provided close
callback. Service/store work remains outside that short lock.

After closing, the controller must successfully persist the original delegation
revoke before applying the approved pairing tombstone. DurableRevoke and Pairing
errors remain distinct; Unknown is not reported as successful deletion, and a
failed first transaction does not automatically continue the second. Cancellation
or document change while waiting for the service lock may leave the original
delegation closed and durably revoked, but prevents the pairing tombstone. A new
pairing revision is never overwritten by an old approved CAS. Mismatched
controller/store approval is rejected before closing that other target.

Same-root re-enablement is deliberately unsupported: fresh confirmation on an
existing Disabled, Revoked or residual Enabled record returns Conflict without
changing blob or journal bytes. No directory replacement, journal reset, schema
upgrade or restart recovery is introduced. This is not yet a full user-facing
toggle. There is still no UI/IPC/network entry, proactive expiry watcher,
long-duration key supplier, or unified cross-process/legacy Forget notification.
The existing external Forget route is not claimed to close current connections.
A6b's Unknown quarantine and limited physical-retirement evidence remain unchanged.

The effective lock regression RED is unattended-close-lock-red.log: holding the
actual service lock prevented the previous close implementation from closing the
gate. The test explicitly handshakes the close thread, releases locks and joins
owned resources before asserting. The corrected independent close handle passes.
The composition-first failure was an invalid test Remember-over-Active setup,
not a failed security gate; the helper now uses the original Forget then Remember
transitions. A later extra-unwrap compile error is likewise not behavioral RED.

Five final UnattendedClose cases cover the close-lock barrier; eight independently
cleaned composition outcomes (success, changed CAS, durable Unknown, pairing
Unknown, No, cancel, foreign controller brand and foreign store); cancellation or
new-document invalidation after the gate is observed closed while the service
lock is held; same-root refusal for all three existing states; and natural run
exit followed by pure close. The last case uses the controller's test-only service
access to the original write path for six bytes, waits for actual child exit,
checks the old admission rejects, then uses the original close handle to persist
revocation. It is not evidence for a new production controller data channel.
The final five-case log is unattended-close-final.log (5/5, 21.66 seconds).

Final same-source targeted regression adds SingleRun 10, Final approval 2,
Preparation 11, HostApproval 11, NativePairing 16, Store 29, DelegatedGate 2 and
DelegatedNative 6: 87 regression cases plus the five new cases, 92 distinct names.
The exact nine-log list and names are in unattended-close-final-ledger.json;
no historical or repeated execution is counted. The eight regression logs are
unattended-close-regression-{UnattendedSingleRun_,UnattendedFinal_,
UnattendedPreparation_,HostApproval_,NativePairing_,UnattendedStore_,
DelegatedGate_,DelegatedNative_}. Strict remote-host lib/tests Clippy passes on
this same source in 25.94 seconds (unattended-close-strict-final.log; combined
serial command exited zero). This is not full-suite, release, real phone,
real-user key, or public-network validation. The pre-existing three unrelated
browser/network fixture modifications are excluded from this stage.
### A6d: original App custody, absolute-end close and checked watcher handoff

RemoteHostService now owns a default-empty unattended custody slot. Installation
consumes only the sealed, already-started SingleRunController; it creates no
approval, registration, grant, network connection or UI enable path. The slot
reserves ownership before spawning its one original standard-thread watcher.
The watcher uses the original FinalConsent monotonic end, including time already
spent confirming/starting/adopting. It waits on the original FinalControl
publication mutex/condition variable and closes on that end or an explicit close
notification. The condition loop prevents lost wakeups. No periodic refresh or
journal observe is performed, and the existing persistent wall-floor semantics
are not weakened. This does not solve the 4096-commit long-duration budget or
provide a long-lived key supplier.

The two lib.rs desktop close/exit handlers retain their original handoff refusal
before request_app_exit. That admission reads the original runtime binding and
document-state locks. Once accepted into the shared wrapper core, the unattended
gate closes before waiting for the old RemoteHostController state lock; direct
request_exit also closes it before its own state lock. Refused handoff exit
requests do not automatically revoke an otherwise running delegation. The
original App waiter now also observes the unattended owner. complete_exit has a
final independent known-result check. Tests exercise that shared wrapper core
with the actual legacy-controller lock held, and request_exit/start_exit with the
real NativeRuntime managed-run factory. They are not desktop window-event or
Tauri IPC automation.

The slot keeps the original JoinHandle through observer cancellation, timeout,
startup publication races and App shutdown. Controller ownership is retained
outside the spawn closure, so a failed spawn performs explicit lock-external
finish/Drop instead of losing custody. A worker panic is sticky failure; its
single panic payload is retained (count bounded, byte size unspecified), rather than invoking an arbitrary payload
Drop. Joining remains outstanding until the actual join and lock-external
controller Drop have returned. Observations distinguish absence, outstanding
work, watcher join, controller Drop return, durable result, failure and effect/key
retirement. No-worker absence is not mislabeled as an observed successful join. The retired
field describes the adopted slot only: with present=false it is vacuously true,
including when a rejected owner is quarantined. Only the composite known()
result includes outstanding rejection work and sticky rejection failure; neither
that single field nor absence grants an App exit/retirement receipt.
A rejected already-started controller is now explicitly closed, finished and
Dropped outside the slot lock, rather than silently leaving avoidable Enabled
state. The in-flight rejection cleanup is registered while the slot decision is
locked and remains outstanding through original finish and Drop. Failure/panic
is sticky before the counter decreases, so an empty slot cannot turn an unknown
cleanup into Known. Durable failures keep their real error; unknown effects do
not become a clean Conflict. Rejected Unknown also advances the original fixed
quarantine even when no controller was adopted into the App slot. The existing
slot is never replaced by the rejected owner. Production enable admission still
has to reserve App ownership before creating a started controller; no such
production enable supplier is introduced in this stage.
Spawn failure remains a conservative failed App-exit result even when its
synchronous close has a durable receipt; it does not wait forever or emit Ready.

Unknown effects do not become successful retirement. After the watcher actually
joins, original SingleRunService Drop moves unresolved ownership into the
existing fixed process quarantine. The original App owned-resource observer
advances that quarantine. Only the previously reviewed sticky-close,
unpoisoned/nonblocking effect gate and original physical-writer Joined condition
can release Captured references. The policy remains Unknown, the OS lifetime
custody and process slot remain held, and no App Ready callback is issued.
The isolated subprocess test additionally verifies original managed Job/child,
reader/writer/wait/stop and IO-drop facts before process exit releases the
quarantine lock. It injects a partial *result into the original policy*, not a
real partial PTY write. This is not proof of internal ConPTY server/kernel-thread
termination or authority to retry an unknown input.

Initial App-exit RED attempts separately record a missing test type, duplicate
shutdown-owner cleanup, and use of managed facts on the ordinary PTY fixture.
Those are not behavioral evidence. With the existing start_managed_run factory,
unattended-app-exit-red4.log reaches the intended failure: all original managed
resource facts and watcher cleanup are confirmed, but request_exit leaves the
new gate open. The corresponding exit-green log passes after the actual close
and waiter integration.

The deadline/Unknown mutation run deliberately adds five seconds to the fixed
wait and falsifies the retirement observation. It produces exactly three passing
and two failing cases: deadline=false while join/cleanup still succeed; and
Unknown with an incorrectly permitted App callback despite quarantine remaining
Unknown and the OS lock remaining Busy. The finally block restores the original
source bytes and verifies SHA256 in unattended-app-mutation-restore.log. These
mutations are not retained. Default-empty exit, startup failure/close-publication
race/panic, observer cancellation, unchanged idle journal bytes, original managed
exit, and Unknown subprocess isolation are separately exercised.
Final same-source evidence: unattended-app-final4.log passes all nine new cases
in 19.37 seconds, including independent adopted/rejected Unknown child processes.
The rejection case007 has an effective earlier RED
(unattended-app-rejected-red.log): its local close and original cleanup succeed
but the old rejection path leaves Enabled instead of the expected Revoked rev3.
Case008 then holds the real revoke seal stage while observing outstanding
rejection cleanup and no App Ready; clean completion and durable Unknown are
both tested before asserting original managed-resource facts.

Final regression is Close 5, SingleRun 10, Final approval 2, Preparation 11,
HostApproval 11, original remote_host::tests 18, the exact original managed exit
case 1, DelegatedNative 6 and Store 29: 93 cases plus the nine new cases, 102
distinct names. unattended-app-final-ledger.json lists the exact ten logs/names;
subprocess occurrences of the same test are counted once. The earlier helper-name
filter that ran zero tests is excluded; its real test
NativeRemote_ManagedOriginalJobAndRemoteOwnersRetireTogether_002 passes in the
final regression. Logs use unattended-app-final-regression-* with those exact
filters (colons replaced by hyphens). Strict remote-host lib/tests Clippy passes
on the same source in 27.12 seconds (unattended-app-strict-final3.log; the serial
command exited zero). Narrow dead_code reasons only identify the default-off
production install chain whose supplier is deliberately not yet connected; no
other lint or permission check is disabled. Final code fingerprints are recorded
in unattended-app-final-source.json. This is targeted Windows validation, not a
full suite, release qualification, real phone/public-network test or completed
long-duration unattended feature. The original three unrelated dirty files are
excluded from the stage commit.

## A6e App reservation before single-run start

The internal App path now accepts a sealed `PendingStart`, not an already-enabled controller. Construction claims the existing fresh service custody and retains the original final consent, exact admission, key custody/readiness/signers and pairing store. It performs no initialize, Enable or capture. The existing service start/compensation and retirement code remain the implementation of those operations.

`reserve_start` checks both `NativeRunAdmission::is_owned_by` and the final consent's original-run identity. The App slot records the first exact `Arc<NativeRuntime>` and owner in the same critical section. Only after acceptance can this module create `StartPermit`; its private fields bind the same final control. `PendingStart::start` requires that fact. The old already-started `install` is test-only. No IPC, UI Enable, material supplier or network entry is enabled by this stage.

One original thread and one retained JoinHandle cover start, fixed-deadline watching, durable close and original key/effect retirement. Cancelling a waiter does not cancel or take that handle. During initialization or Enable, App exit closes the same final control used by the original store authorization fences. Failed starts retain the original service for exact compensation and cleanup. Rejection and spawn failure do not run start as cleanup. Unstarted Drop moves the original keys into the fresh service before its fields are destroyed, so the original custody outlives key cleanup.

`start_result` is a historical return observation. `None` means no start result has been returned, including unspawned or panicked paths. `Some(Ok(()))` does not establish current authority or future trust. App `known()` remains a separate composite cleanup predicate. No UI may translate either alone into an enabled or fully retired state.

A foreign Runtime passed to `start_exit` is rejected before beginning any native shutdown. The original Waiting intent and owner remain available for a later call with the bound Runtime. `request_exit` closes the reservation gate before publishing Waiting; therefore an admitted exit cannot acquire a different Runtime through a later reservation. The original maintenance refusal remains unchanged. Unfinished HostCheckout and NativeShutdownCheckout drops also close this same gate before acquiring the original service-state lock. Normal finished checkouts do not close it. This short operation may notify the existing Condvar waiter, but does not execute user/async-waker callbacks, perform storage work, stop resources or join a worker on the Drop stack.

The five isolated tests use the original Node/PTY paths, ExistingKeyLoader with MockKeyApi, and isolated store files. Cases 001, 003 and 004 use the real managed NativeRuntime run factory; cases 002 and 005 use the existing ordinary PTY fixture. They cover start-return/close coordination, a real key-free barrier during unstarted Drop/rejection/spawn failure, foreign Runtime exit rejection, exit during Enable sealing before the original Windows publication fence, and both original unfinished-checkout Drop paths before admission and after start. The checkout failure fixture unwinds the real empty original checkout guards; it does not simulate a crashed native I/O worker. They are not real CNG, real phone, desktop window-event automation or a production enable UI. The free barrier is not a simulated free failure. The new reservation tests do not add a MockKeyApi free-failure case. Original key-Unknown results still deny the composite App-known predicate; this stage does not redefine the original key retirement/quarantine contract. Final same-source verification is recorded below.

A6e preserved evidence: `unattended-reservation-first.log` ended at compilation after a machine disconnect and is not a result. The four-case interim source passed 121 distinct tests and strict Clippy (`unattended-reservation-final.log`, `unattended-reservation-regression-*.log`, `unattended-reservation-strict-first.log`, 58.86s). The two deliberate mutations produced four failures after cleanup: missing request-exit close yielded `immediate=false`, and missing pending resource transfer yielded `busy=false` inside the original free barrier. `reservation-mutation-source.json` and `unattended-reservation-mutation-restored.log` record exact byte restoration. The later global-checkout RED returned `Ok(())` from reserve after the original checkout had failed; the target assertion failed after cleanup (`unattended-reservation-global-red.log`). These are behavioral failures, not interface/compiler failures.

Final A6e evidence: `unattended-reservation-final2.log` passed 5/5; the eleven `unattended-reservation-final2-regression-*.log` groups passed 117/117. `unattended-reservation-final2-ledger.json` records 122 distinct test names, without counting the earlier source or repeated runs. `unattended-reservation-strict-final2.log` completed strict Clippy in 29.80s. The command session exited 0. `unattended-reservation-final2-source.json` identifies the six tested source files. The product still has no unattended Enable UI or production supplier; long-lived key material, journal write budget and real device enrollment remain separate gates.


## A6f: bounded serial key epochs in the original App owner

Internal/default-off only: no Enable UI, network reconnection, or 30-day
supplier is provided. The actual final approval remains bounded by its original
parent deadline (at most 60 seconds). A child material window is at most 10
seconds and never extends that deadline. The App reservation, Runtime identity,
final approval, captured run, policy and durable Enabled revision are not
restarted, recaptured or re-enabled to rotate keys.

The accepted App slot lends its non-Clone StartPermit to startup and every
advance. A private non-Clone EpochSource consumes the original unconsumed
supplier once under that matching permit. Previously selecting one-shot
material forbids epoch mode. Child factories check the source's original
control identity and the same permit; no caller key/binding or approved boolean
supplies this fact. The production default remains the original empty slot.

The same App worker waits for min(child end, parent end). It revokes only the
child, drops the original signer set/ready receipt, and requires original key
and notification custody to report !outstanding, disposal_complete and !unknown
before replacement. The old known-joined custody remains owned until the next
custody takes over, including across cancelled final checks. Each new profile
uses the current exact pairing candidate/CAS and original profile, key names,
purposes and SPKIs. Ready checks retain the complete child binding including
its exact cap. Original run, current durable CAS, parent clock and cancellation
checks surround construction and publication. Pre-capture startup still checks
the original caller; later generations preserve the established independent
delegated-run contract rather than reattaching the old WebView.

NativeRevocation has one local monotonic bit and at most two immutable root
witnesses. Reads have no recursion, callback or lock; child revoke affects only
its local bit, and a child cannot export that local bit as a complete root
witness. The sealed factory binds the original approval lease and FinalControl
roots. Original loader checkpoints, signing fences, ready checks and retained
signer clones therefore observe parent cancellation without waiting for the
App worker. No effect/signer is lent out during rotation. This does not prove
rotation of future external signer leases or established network connections;
their exact network scopes remain separate gates.

### Unknown remains blocked ownership

The original native-key owner deliberately parks with its key and original
Join when native free is Unknown. A6f does not clear it, retry free, detach or
replace it. Observing Unknown closes the parent first while retaining the
original key, notification and service workers, OS custody and process slot.
The isolated child-process negative test checks parent closure, no-next-open,
OS Busy and retained original owners. Child-process termination is only test
cleanup, NOT an App/PTY/key retirement receipt. This failure can prevent normal
App exit. No new Captured-resource splitting is introduced. Normal and known
failure paths separately check the original managed PTY/task retirement facts.

### Real journal cost; checkpoint remains a design gate

The unchanged journal has 8192 records of 224 bytes: 4096 complete commits.
A successful transaction appends a 448-byte Pending/Clean pair and separately
publishes a complete sealed blob. Original initialize and Enable each cost one
pair. Same-wall Store.observe adds no pair; advancing its floor costs one.
An effect request runs that observation before later admission, so a request
subsequently rejected for a foreign supervisor still pays the pair. This is
not a known-outcome claim about failed physical effects: the earlier zero-size
resize fixture exercised the existing sticky Unknown policy, and its failed
cleanup log remains evidence rather than being relabelled a successful close.

The actual Store test measures journal lengths 448, 896, 896, 1344 and 1792 for
initialize, Enable, same-wall observe, advancing observe and revoke. The actual
service test measures +448 before the foreign-supervisor request is rejected,
then +448 for revoke. A separate near-full legal-codec history measures the
advance pair and successful use of the last reserved revocation pair; this is
not claimed as 4096 real commits. Two startup pairs plus the final revocation
reserve leave at most 4093 ordinary advances. This does not promise 30-day
availability. Epoch checks read the current durable CAS but do not add periodic
Store.observe or reduce the original effect observation frequency.

A future checkpoint must be a distinct verified format/anchor, not a larger
limit, truncated journal or fresh root. Current backend/lifetime custody pin
the journal FileIdentity and the codec has no checkpoint record. The minimal
future design must bind preserved wall floor, store/authority revisions, exact
target, sealed-blob digest and prior chain anchor under the original exclusive
transaction/CAS, retain revocation capacity throughout, and prove each
interrupted publication leaves the old trusted state or Locked/Unknown. Old
readers must fail closed. This stage changes no journal format, schema, limit,
publication cadence or restart semantics and claims no new whole-filesystem
rollback protection.

### Validation scope and final results

All new key-load tests use the original ExistingKeyLoader with isolated
MockKeyApi system calls, not real user CNG keys or a real phone. The App tests
use original managed Node/PTY runs and the original App worker, not a fixture
calling advance to keep the owner alive. The signer-clone negative test uses
the existing isolated endpoint-stage helper and does not prove Relay Accepted.
Earlier failures remain in the audit logs: parent propagation RED, consumed
one-shot mode RED, incorrect Unknown-join expectation, foreign fixture duplicate
initialization, and the two journal-measurement fixture mistakes.

Final same-source validation passed 178 distinct tests: the 11 original-App
epoch cases plus 167 targeted regression cases. The ledger is
`.audit/unattended-epoch-final-ledger.json`; the epoch group completed in 107.94s
(`unattended-epoch-final.log`). Strict Clippy with `--lib --tests -- -D warnings`
passed in 26.08s (`unattended-epoch-strict-final.log`). HostOpen's anonymous real
CNG signing case was explicitly skipped; its remaining 10 cases passed. These
results do not claim real user CNG validation.

Two independent behavior mutations failed as intended. Removing Unknown's
parent-close produced `parent_closed=false` while original ownership remained;
removing the worker's child-deadline wake produced only the first generation's
two opens and no second ready. Their logs are
`unattended-epoch-unknown-mutation-red.log` and
`unattended-epoch-idle-mutation-red.log`. Both files were byte-for-byte restored
before the final test run, checked against `epoch-mutation-source.json`.
The final 12-source fingerprint manifest is
`.audit/unattended-epoch-final-source.json`. No network, cloud, credential or
production UI was enabled. Independent final review remains the commit gate.

## A6g: explicit short network consent and original App-owned vertical path

This stage is an internal, opt-in short-session backend, not the production
unattended UI. The original Local final approval remains local-only. A distinct
private ShortNetwork mode displays the exact enrolled device, record fingerprint,
run/generation/role and Relay route, and freezes its actual deadline before the
native confirmation is shown. Its limit is 10 seconds including confirmation and
setup, not a renewed window after confirmation. The network PendingStart and
signing-authority factories both reject Local consent. No IPC/UI enable route,
public server, real phone enrollment or persistent user-key change is installed.

The original App reservation fixes the Runtime/App identity before start. The
service preserves the original captured owner on a rejected Host reservation,
instead of treating a consumed owner as never captured. On acceptance the same
owner moves into one DelegatedSession. Its material is derived only from the
same final consent, current record/CAS and original ready key workers. The
original HostSigningStages are present: Relay signing/Accepted precedes endpoint
signing, TLS, original Installed and activation. A private signing authorization
binds the exact attempt and original key token/cap; the setup scope is bounded by
the actual setup deadline, child cap and fixed consent deadline. There is no
network epoch replacement or automatic reconnect in this stage.

The durable single-run Binding is unchanged. Its device field remains a
pairing-UUID domain-separated identity, and its origin remains Browser origin.
Those are not the TLS endpoint ID or Relay origin. After comparing the complete
VerifiedBinding against the same consent, the policy privately fixes a runtime
ChannelIdentity using the original P256Spki Client endpoint derivation and strict
Relay CanonicalOrigin. All attempts share that immutable policy; epochs,
principal, scope, original channel authority and attempt checks remain required.
There is no caller setter or production fallback to a mismatching identity.

Each authenticated top-level DataRecord passes the same service refresh before
original dispatch: current pairing/CAS, durable observation, key/run/control and
tail checks. A sealed one-use permit owns the exact record, so another payload
cannot be substituted after authorization. The gate weakly references the
original service and uses try_lock; failure closes the independent control and
channel without waiting on a service that may itself be joining the pump.
This is a current observation, not atomicity with an external process changing
CAS after the observation. SnapshotNext is consumed inside the original fixed
one-second snapshot future, not by the top-level pump: it retains original
channel authority/deadline checks but does not perform a new pairing CAS or
journal observation per batch. Top-level ACK/control requests can still advance
the persisted wall floor and consume journal capacity; no checkpoint, cadence,
capacity or rollback semantics are changed here.

The same original App worker closes the short session at its fixed end; it does
not close and reopen a parent gate. Terminal cleanup distinguishes a setup that
never adopted a Host from an installed session. It waits for original socket/
material work and, where present, the original pump/channel/Host retirement,
then original keys/effects before overall known retirement. This terminal path
does not mint a reconnect receipt. Unknown retains the existing quarantine and
cannot become App-ready merely because one join returned. Physical key free may
precede network join after cancellation; overall release still requires the
original owners' evidence. Long-lived materials, long unattended availability,
background phone behavior and public-network performance remain unverified.

Validation uses original managed Node/PTY, the real local Relay, the existing
WASM data artifact and the real Rust production material/stage path. Node supplies
a synthetic Window/WebAuthn ceremony, not a real browser or phone. Only public
fixture scalars are anonymously imported through the original ExistingKeyLoader
CNG path; imports have no key name or persistence flag and do not open user keys.
A confirmed pairing record is isolated historical fixture data, not proof of new
real enrollment. Test-only changes select loopback trust and observe Accepted;
they do not remove signing stages or construct a production approval boolean.

The final short-chain group passed 4 distinct cases in 20.67s
(`unattended-short-network-final4-regression-UnattendedShortNetwork_.log`): one original 1920-cell snapshot and one
remote input byte followed by full original App/managed-resource cleanup;
Local consent rejected before socket/Enabled/journal writes; and real pairing
Forget after snapshot/window followed by rejection of the next actual Input;
and idle expiry without business keepalive. The expiry case waits for the
original channel to retire, then verifies durable success, watcher join,
controller Drop and known retirement before requesting App exit. Its observed
idle interval was 7686ms after setup/snapshot/window, inside the deadline frozen
10 seconds before confirmation. No fresh 10-second period was granted at start.
The pairing-revocation case checks the original PTY report after a six-byte local write, proving
zero additional remote bytes. Each network case runs in its own original child
process because the Host registry intentionally permits one admission per
process. A count of child and parent harness results is not two distinct tests.
The actual authorization also rejects a different attempt brand and cap +/-1ns
in read-only checks before it is consumed; this is not a second full foreign
network session.

Removing only the per-record refresh produced the intended behavior RED:
`unattended-short-network-gate-red.log` reports `Missing expected rejection` at
the real post-Forget Input. The original gate was restored before final tests.
Earlier setup mistakes (SEC1 prefix offset and mismatched public fixture
scalar/SPKI) and the original UUID/transport-identity mismatch are retained in
separate logs, not counted as expected test passes. The artifact paths/hashes are
in `unattended-short-network-artifacts.json`; the existing WASM was reused, not
claimed to have been rebuilt from this stage.

Final same-source validation covers 169 distinct tests: the four new cases plus
165 affected regressions. The exact names and 18 group logs are recorded in
`.audit/unattended-short-network-final-ledger.json`; isolated child harnesses are
not counted twice. Epoch's 11 cases use `unattended-short-network-final3-epoch.log`,
and the other groups use `unattended-short-network-final4-regression-*.log`.
Strict Clippy (`--lib --tests -- -D warnings`) finished in 58.40s in
`unattended-short-network-strict-final.log`; the strict TypeScript fixture check
also passed. The 23 source fingerprints are in
`unattended-short-network-final-source.json`.

One existing epoch test incorrectly required both concurrent generation-two
loaders to open before either could fail. The actual wrong-SPKI loader can fail
and revoke its sibling before that sibling opens. The test now records the
actual corrupt-public export (or actual open-error return), requires original
generation-one join before that fault, forbids second-generation verified/ready,
and checks parent closure, matching free events for every observed open, and
original retirement. It no longer demands unsafe extra work after cancellation.
This is a test-only race correction, not a relaxed production loader or budget.
The earlier failure log remains preserved. A separate PowerShell 5 harness
stderr-handling failure is also retained, not counted as a product test result;
the final suite ran under the original PowerShell 7 environment.

Independent final review remains the commit gate. The original three dirty
files are unchanged and excluded; this stage's snapshot fixture and preparation
tests do not depend on their uncommitted additions.

## A6h: explicit short-session product entry

The Host panel now offers a default-off, explicit ten-second network session for
an already Confirmed registered device and the current exact live run. Selection
does not enroll a device or create keys. The original preparation approval is
still not network consent: the separate final native dialog freezes the actual
ten-second deadline before display, including confirmation/setup time. The
product does not promise unattended longevity, reconnect, cross-restart restore,
or public deployment. Accepted means the original App start returned success,
not that a phone has connected.

Before the first dialog, the original App custody registers a Preparing slot
under its closing/Runtime identity gate. The slot owns the original run admission;
a single in-flight operation temporarily owns it and either returns it, transfers
it to the original PendingStart/adopt path, or drops it before releasing the
in-flight count. Flow/UI state cannot retain the run pin independently. Closing
first makes the request sticky closed, cancels original approval handles outside
the management lock, and reports completion only after cancellation and owned
resource disposal return. Late approvals cannot create Enabled state or key work.
The same tracked final-control identity is required at reserve_prepared.

Request uses the original live-run/document admission. Advance also binds the
original document, caller, operation and Runtime. Current and pure close instead
use the current trusted document-only admission, allowing a new trusted document
to close the old operation without requiring the old run to remain alive. Current
does not consume either approval; it may observe original retirement. The private
production factory uses the existing pairing store/DPAPI and a fixed user-home
unattended root, opening existing state rather than resetting it. Existing
persistent state and repeat starts remain fail-closed. The UI explicitly says
that a failed request is not retried within this App lifetime.

The test-only private environment substitutes isolated stores/sealers, the original
explicit Yes/No completion APIs and anonymous public-vector CNG material. It calls
the same request/advance implementation, not a parallel fake flow. No production
caller can supply a store path, sealer, approved flag or synthetic registration.
The Node/WASM/Relay oracle is not an actual phone, WebView2 native dialog test,
or a rebuilt same-source WASM artifact; it reuses the previously recorded artifact.

A real cancellation test initially timed out in original App exit after ten
seconds because Flow retained NativeRunAdmission. The failure remains in
`.audit/unattended-product-cancel-red.log`. Moving ownership into Preparing made
the same four cancellation/foreign-operation/document cases pass in 4.01s with
original App wait/join, and the two Preparing tests now explicitly assert original
cleanup success. The earlier Preparing test that ignored cleanup's return is not
used as retirement evidence. Final same-source tests and independent review remain
the commit gate; no A6h completion or production-phone validation is claimed here.

Final Rust validation covers 100 distinct tests across twelve group logs in
`.audit/unattended-product-final-ledger.json`: Preparing 2, Product 2 (the negative
case includes five scenarios), original ShortNetwork 4, App 9, Reservation 5,
Close 5, SingleRun 10, Final 2, Preparation 11, Store 29, Document 19 and Proof 2.
The fifth product scenario completes the original run through its local input
path, confirms old run access is rejected, then uses current-document status and
close and joins original App exit. The Product network case repeats both original
approval stages and original Relay/WASM snapshot/input on the final Rust source.
Child harness results are not counted twice. Strict Clippy passed in 1m17s in
`unattended-product-strict-final.log`; the initial equivalent boolean-expression
lint was corrected without changing the authorization predicate. The sixteen
source fingerprints and seventeen-file stage list exclude all three prior dirty
files. UI final tests/typecheck and independent final review are recorded at the
stage closeout; no actual desktop native dialog or phone interaction was exercised.

Final UI regression is 84/84 (seven new short-session cases plus 77 existing
Host/pairing cases), with TypeScript build and vue-tsc both successful. The late
request cleanup rejection after component unmount now emits the existing warning
toast rather than writing only to an invisible component. Logs are
`unattended-product-ui-final.log` and `unattended-product-typecheck-final.log`.

## A6i: explicitly configured Browser production pull source

The existing approved-profile assembly now preserves an explicitly present
`snapshotPull: snapshot-pull-v1` alongside `controlLease: live-v1`; absence remains
absence. The strict candidate/provider full-field comparison still occurs before
assembly, so a pull/non-pull mismatch cannot silently upgrade an existing record.
The production `browserConnectionSource` selects `browserPullConnection` only for
that approved explicit mode, after the original authenticated descriptor. Both
branches use the same wrapped original port/source close. Socket, Worker,
transport, brokers and enrollment-retained retirement remain owned by that source.
No missing capability/rejection falls back to another protocol, and the fixed
one-second operation budget is unchanged.

Two actual defects were reproduced: the original reader dropped the explicit
field (`browser-source-pull-profile-red.log`), and the real production source sent
legacy `snapshot` despite authenticated pull negotiation. In
`browser-source-pull-network-red3.log`, observed original MessagePort traffic is
exactly `[snapshot]`, followed by request rejection; original Host cleanup still
completed. The corrected source sends `snapshotPull`, installs all 1920 cells,
writes one byte with HostWritten, settles, and closes the original source.
`browser-source-pull-network-green2.log` records Worker retirement and termination,
physical WebSocket closure, enrollment retirement and the original Host's managed
cleanup facts. The child harness is one test, not an additional distinct pass.

The network test uses the actual production Source and profile reader, real
loopback Relay/TLS/WASM/Host and the same original two Host approvals. Node adapts
Window/WebAuthn/WebSocket/Worker interfaces using existing public anonymous test
vectors. Its Worker bridge invokes the original TLS Worker fixture and filters
that fixture's telemetry; it does not validate browserEntry.worker.ts artifact
fetch/import, a real desktop native dialog, actual mobile WebAuthn, or a public
site. The existing WASM artifact is reused. Initial test-routing, identical-key
handle and telemetry-adapter failures are retained but are not behavior REDs.
The first post-fix network run completed application work but incorrectly waited
on a test enrollment owner without a retire callback. The fixture now supplies
its original optional retirement contract; production retirement code was not
changed to hide that failure.

The source-boundary unit tests separately cover the unchanged legacy branch,
explicit pull branch and pull rejection without fallback, retaining original
source socket/Worker/broker/enrollment drain. Their mocked lower layers are not
an authentication claim. No real browser profile, key, registration, Relay policy
or deployment was created or changed. The example provider remains unavailable;
actual registered-device/operator configuration and real-device acceptance are
still required before claiming end-user availability or long unattended use.

Final same-source validation: seven distinct Rust tests (three Product and four
ShortNetwork, without duplicate child counts), 98 Vitest tests (77 profile,
18 enrollment, three Source branch/close cases), and 26 existing Node unit tests.
Root TypeScript/vue-tsc and the new fixture's strict TypeScript check both passed;
Rust strict Clippy passed in 30.37s. Logs are indexed in
`.audit/browser-source-pull-final-ledger.json`; seven source fingerprints and the
eight-file stage list exclude the original three unchanged dirty files. Independent
final review remains the commit gate. This is production Source wiring under
explicit approved configuration, not provisioned real-device or public deployment.

## A6j: isolated real-browser EntryWorker loading boundary

This stage adds `tests/remote-browser/browserEntry.real.mjs`, an explicitly invoked
platform test, not a change to production admission. It launches the installed
Chrome through Playwright in a new temporary context. It does not attach to a
user browser, import user credentials, alter trust stores, ignore certificate
errors, or expose a public listener. The HTTP server binds only `127.0.0.1` and
supplies COOP/COEP. Chrome treats that origin as secure and isolated, but the
operator source still correctly requires HTTPS.

Run from the repository root with `PLAYWRIGHT_CHROMIUM_EXECUTABLE` pointing to
an existing official Chrome installation and `CC_DESK_TEST_BROWSER_ARTIFACTS`
pointing to the previously verified test WASM/glue directory:

```
node tests/remote-browser/browserEntry.real.mjs
```

The test bundles the actual production EntryWorker and starts it with a real
module Worker and its original start/close message listener. No Node Worker
bridge or fake Window is used. Four checks cover: HTTP operator refusal with
zero `/browser-profile.js` requests; foreign artifact refusal before fetch;
WASM 404 without glue import; and actual same-origin WASM fetch plus official
glue module evaluation followed by fail-closed application/transport retirement.
The glue retains its original body and only appends a test-only evaluation
message. Invalid connection inputs deliberately prevent authorization. Fetch,
module evaluation and retirement are not proof of WASM instantiation, successful
TLS authentication, a real device enrollment, or an end-to-end remote session.
The transport peer is an isolated MessagePort fixture that only acknowledges close; these receipts do not prove retirement of a real socket or TLS connection.
The test waits separately for the original application and Worker receipts;
only then does it terminate the Worker and close its ports. A six-second
watchdog fails the test, rather than treating forced termination as retirement.

Observed on Chrome 157.0.8103.79: all four checks passed in
`.audit/browser-entry-real-final4.log`. The HTTPS-gate mutation runs only in an
in-memory test bundle (`CC_DESK_TEST_REMOVE_HTTPS_GATE=1`), never in the worktree;
`.audit/browser-entry-real-https-red.log` fails because the forbidden HTTP
operator configuration was fetched. Earlier strengthened-test failures are
retained: one incorrectly assumed cross-port delivery ordering, and one
incorrectly required an explicit `uncertain:false` on the already-served
application protocol, which normally omits that field. Neither was a production
authorization failure. A separate isolated CDP probe confirmed virtual CTAP2
creation/removal, but this test does not exercise WebAuthn registration.

A real HTTPS operator success path still needs a browser-trusted test origin
and matching operator configuration/artifacts. This stage deliberately does not
install a CA or bypass TLS verification. Existing unit tests, not this HTTP
platform test, cover HTTPS provider absence and candidate/provider mismatch.
Actual phone registration, native dialogs, HTTPS deployment and the full
EntryWorker-to-Relay success path remain unverified. Default-off behavior and
the original short-session limits are unchanged.

## A6k: current production artifact and synthetic-origin operator verification

The existing `scripts/build-browser-runtime.mjs` was run offline against the
current checkout with the existing LLVM 21.1.8 and wasm-bindgen 0.2.129 tools.
The previous ignored production manifest had 55 source entries, 22 of which no
longer matched the checkout. Its full manifest and file hashes were preserved
in `.audit/browser-production-rebuild-before.json`; no old evidence was erased.
The new production manifest has 65 matching source entries, three matching
compiler inputs, and two matching output files. Its six required browser
exports are present and there are no `TEST_ONLY` exports. The production WASM
is 5,808,647 bytes, SHA-256
`89dc2d466156e5da71fed8ad6f7cfc1c9e501066136305fb13fd9ba86145f75a`.
The glue SHA-256 is
`2631ef5e2536935d9dc797ba410edb358e63edbf70c2204da5f7d35827e1859d`.
These generated files remain ignored; this stage does not commit binaries.

`tests/remote-browser/browserOperator.real.mjs` first checks the manifest's
source, compiler-input and artifact hashes against disk, compiles the WASM with
Node for its export inspection, then opens isolated real Chrome contexts. Set
`PLAYWRIGHT_CHROMIUM_EXECUTABLE` to the installed official browser and run:

```
node tests/remote-browser/browserOperator.real.mjs
```

The default artifact directory is `public/remote-runtime`; the explicitly
TEST_ONLY `CC_DESK_TEST_PRODUCTION_RUNTIME` override permits a stale-manifest
negative run. The preserved old manifest fails the source-hash gate in
`.audit/browser-production-stale-red.log`, before browser launch. Current
production inputs pass in `.audit/browser-operator-real-final2.log`.

Playwright fulfills requests for the synthetic `https://desk.invalid` origin,
following the repository's existing browser test infrastructure. This exercises
real Window, secure-context/isolation and production dynamic import behavior,
without a certificate, a TLS handshake, trust-store modification or certificate
exception. Each operator scenario has a fresh context and ESM cache. Missing
operator, an explicitly undefined provider, extra module exports, and differing
candidate/provider pull configurations remain locked or rejected with zero
Workers and zero WebSockets. The mismatch uses valid nonextractable handles
imported from public test vectors; an independent original-reader positive
control confirms the matching configuration preserves `snapshot-pull-v1` and
the original handles. That positive reader control does not open a Source or
prove authorization. All WebSocket routes are intercepted and closed, so an
unexpected connection cannot escape the fixture. No enrollment or user-key
store is opened or modified.

Separately, the original A6j real EntryWorker test was rerun with
`CC_DESK_TEST_BROWSER_ARTIFACTS=public/remote-runtime`. All four checks pass in
`.audit/browser-production-entry-final.log`, now fetching the freshly generated
production WASM/glue instead of the older test artifact. The served glue has
only the A6j test-only evaluation message appended; its on-disk original hash
above is unchanged. This remains a real module-Worker fetch/import and
fail-closed retirement test, not a browser WASM-instantiation or authentication
success test. The operator suite contributes six checks (one artifact check,
four operator cases, one reader positive control), and EntryWorker contributes
four; they are distinct scopes, not ten successful network sessions.

A browser-validated HTTPS transport, real registration/phone, and a successful
production EntryWorker-to-Relay session remain separate gates. This stage
neither upgrades old configurations nor changes production defaults, user
credentials, public deployment, or the existing ten-second Host limits.

## A6l — real browser production Source and authenticated short session

The isolated `UnattendedProduct_RealBrowserProductionEntry_006` fixture joins
previously separate gates: the real Chrome Window imports the operator, the
original reader and enrollment provider feed production Source, and Source
constructs the actual module EntryWorker. That worker fetches the unchanged
production WASM/glue, not a Node bridge or test WASM. Each run verifies every
manifest source/input/output hash and rejects TEST_ONLY exports. Public test
vectors and a fresh CTAP2 virtual authenticator supply the matching RP,
credential and nonextractable key handles; a positive credential sign counter
is checked after the actual WebAuthn assertion.

Playwright still fulfills the synthetic HTTPS origin without a browser TLS
handshake. Its WebSocket proxy permits only the exact fixture Relay endpoint;
Node verifies the published fixture CA, TLS 1.3 and subprotocol before forwarding
binary messages. This is not browser certificate-trust, real-phone registration
or public deployment evidence. No user profile, credential or trust store is
modified. The original production defaults and approval limits remain unchanged.

The complete success path requires the original Installed channel, a 1,920-cell
snapshot, Window, one Host-written input byte and settled Idle. Before the
isolated browser is disposed, the original Source connection, enrollment,
Worker and proxy sockets must finish their own close receipts. The Host also
checks its original managed retirement observations, without elevating the
historical/Unobserved platform fields into a stronger claim.

Early runs exposed fixture timing instability. The retained `second` and
`mux-end` logs contain successes, while `final`, `recovery`, `timing` and other
diagnostic logs contain failures; none are substituted for final-source evidence.
A diagnostic matched the ClientHello ciphertext at the Node send and original
Host mux receive, and identified the existing two-second initial IO window.
That does not establish a unique cause for every earlier failure. Temporary
native tracing and Worker clock instrumentation were removed before final tests.

The fixture now prewarms only artifact checks, bundling, isolated Chrome, the
public virtual authenticator and an empty page. READY asserts zero operator/app
imports, Workers and sockets. Only after the original final approval and Host
readiness does the same owned child receive one GO. Neither the two-second IO
budget nor the ten-second authorization is extended or re-anchored. Both output
pipes are drained from spawn; EOF requests browser cleanup, and a failed
cleanup's last resort targets only that child process tree, never counts as
normal retirement. The absolute Node watchdog likewise cannot produce success.

A second, independent compatibility defect was reproduced after prewarming:
Chrome's actual assertion included `other_keys_can_be_added_here`, with a
37-byte authenticator payload, UP/UV flags and a valid-format DER signature.
The original signed-client-data regression then returned `Encoding` for the
same extra-key shape. [WebAuthn §5.8.1](https://www.w3.org/TR/webauthn-3/#dictdef-collectedclientdata)
requires tolerance of unknown keys and order; Chromium deliberately sometimes
adds this key. Only the inner `ClientData` serde structure now ignores unknown
members. The outer approval DTO stays strict; known-field types and duplicate
checks, origin/challenge/type/crossOrigin/topOrigin policy and size bounds stay
unchanged. Verification still hashes and verifies the original client bytes,
including every extension byte, rather than normalizing or stripping them.

The behavior RED is `.audit/browser-client-data-extension-red.log`; the focused
GREEN covers signed string/nested extensions, post-signature extension mutation,
raw duplicate known keys including an escaped equivalent, invalid known types,
2049-byte client data and explicit `topOrigin:null`. Unknown extension bytes
modified after signing now reach and fail signature verification, not parsing.
This change requires rebuilding both native Host and production WASM; the
previous manifest is retained in
`.audit/browser-source-real-before-extension-manifest.json` and is not valid
evidence for the new parser. No other protocol DTO is made extensible.

Final-source verification: the security library passes 143 tests, with its
pre-existing `Registration_BrowserOracle_900` ignored. The original short-network
and product groups pass four tests each: 151 distinct Rust tests total. The new
real-browser case additionally passes three bounded consecutive runs (6.62s,
7.58s, 6.52s); these repetitions and the product-group rerun are not extra
distinct tests. Logs are indexed in
`.audit/browser-source-real-final-ledger.json`; final source hashes are in
`.audit/browser-source-real-final-source.json`. Strict security and native Clippy
both pass (11.93s and 46.08s), and the MJS syntax check passes.

The rebuilt production WASM is 5,808,691 bytes, SHA-256
`c6573066b23a195404496e26eb9e95051945067edeefdbab1b184536139088bf`;
the glue retains the A6k hash. Each real run rechecks the full current manifest.
Generated artifacts remain ignored. The stage commits only this compatibility
fix, its tests, the real-browser fixture/native harness and this documentation;
the three pre-existing dirty files remain untouched. Real phone enrollment,
browser-validated HTTPS/WSS certificates, public deployment, long-lived access
and seamless reconnect remain unverified separate gates, not implied by this
isolated ten-second session.

## A6m: bounded authenticated data after signing-key retirement

This internal mode is not the existing product switch. `NetworkSession` requires
its own final consent, freezes an actual parent deadline of at most 60 seconds,
and explicitly does not authorize reconnect or restart recovery. `Local` and
`ShortNetwork` keep their previous meanings. The production UI still offers the
previous short session; no real device, user key, public service or certificate
trust configuration is created or changed here.

Signing authorization still requires the exact original ready key identity,
parent cancellation roots, accepted attempt and original setup deadline. Its
`end` remains limited by the current child key cap. A separate `data_end` is
bounded by the original parent and network budget. It determines the initial
Grant policy and socket upper bound, not an extension of an already issued Grant.
The independent browser Grant may be shorter. Existing two-second initial IO
and setup checks remain unchanged.

Only the original socket task, after successful live Installed activation and
revalidation of the channel's original live fact and clock, publishes private
`InstalledData`. It retains the exact original attempt/channel and fixed budget.
It is not a persistent boolean authorization: every use checks current stop,
deadline, budget and channel authority. No-live success cannot issue it. Before
publication, the original current ready-key check still applies. After publication,
invalidity is a denial, never a transition back to ready. A retired key cannot
use the pre-publication path. The socket task sets its original stop before any
retirement await, and retains the published fact instead of resetting its state.

The existing App worker alone handles normal child signing retirement. The service
retains its original `Arc<HostKeyCustody>` while that worker advances the same
custody outside the service mutex; the worker does not introduce a second owner.
Publication checks the original Arc identity, parent and Installed data again.
Normal retirement does not revoke the parent; Unknown closes the parent and keeps
original custody and the process/OS ownership rules. The current data gate also
rejects a key-custody Unknown observation. No signer is rearmed or reused for a
new handshake. Reconnect remains unimplemented in this mode.

Data operations retain the original pairing candidate/CAS, durable Store.observe,
wall floor, captured-run and tail checks. There is no disk IO in GrantClock and no
Busy bypass. SnapshotNext remains an internal batch receipt under its original
fixed snapshot-operation budget, not a new top-level Store.observe. The signing
retirement transition performs one ordinary refresh/observe; it does not create
periodic journal ticks. The 4096-commit bound and reserved revocation pair remain.
A crash-safe checkpoint is still required before claiming practical long-lived
operation; no journal reset, floor weakening or larger capacity is introduced.

### Evidence and scope

The initial feasibility probe used an explicit test-only key-retirement call
while Installed TLS remained open. It proved original CNG/notification joins and
parent independence, not automatic production retirement or data usability.
`network-key-retirement-probe-live.log` records that limited result.
`network-data-after-signing-red.log` then rejected the actual next browser request
in the old short mode, with original cleanup complete. This is retained as the
old-mode no-expansion negative test, not changed into an authorization success.

The first new-mode run used the original App worker, waited eleven seconds after
Source.open without business keepalive, then completed a 1920-cell snapshot,
window, one-byte PTY input and settlement. The original signing/notification join
is separately observed without advancing it from the test. The isolated parent
close and 14-second parent-deadline cases reject subsequent business and deliver
zero remote PTY bytes. Test-process watchdogs for these longer cases are bounded
separately from, and do not extend, production authorization or handshake budgets.

`network-installed-race-red.log` preserves a genuine first-Data publication race:
with Installed fact publication delayed by the original activation hook, the
first browser request was rejected. The regression observes a real authorized
DataRecord before that hook returns; it does not inject a fabricated DataRecord.
The hook has a one-second test bound and does not alter production budgets.

All browser cases still use the same production Source, real module EntryWorker,
verified production WASM, virtual CTAP2 credential, original Relay and isolated
anonymous CNG test keys. Synthetic HTTPS routing and Node verification of fixture
WSS are not a browser certificate-trust test, a real phone test, public deployment,
30-minute availability, 30-day unattended access or cross-restart recovery.

Final validation: six targeted cases pass on the final formatted source in
`network-authenticated-formatted-final.log`; this count includes the limited
retirement probe and is not six separate production features. Native strict
Clippy also passes in `network-authenticated-strict-final.log`. Fifteen affected
regression groups passed 137 distinct tests before the final rustfmt-only cleanup;
`network-authenticated-format-proof.json` records the before/after source hashes
and formatting scope. The native test binary, six targeted cases and strict
Clippy were rebuilt/rerun after formatting. This is 143 distinct tests overall,
not a claim that every regression log used byte-identical final formatted source.
The exact source manifest and split ledger are
`network-authenticated-final-source.json` and
`network-authenticated-final-ledger.json`. Original three dirty files and ignored
runtime artifacts are excluded from this stage commit.

## A6n: bounded authenticated checkpoint (isolated implementation)

This stage adds a physical journal checkpoint under the original owner custody,
ControlLease and pinned journal FileIdentity. It does not change the sealed
state document, authority/revision, observed wall floor, target, cutoff or status.
The checkpoint operation accepts the original exact CAS and authorization fence;
it does not turn a LockedSnapshot into permission or introduce restart recovery.
There is no automatic production compaction or UI call in this stage. Applying
in-place history reclamation to real user state still requires a recoverable
backup or the applicable explicit approval for irreversible history removal.
Only isolated test roots are rewritten here.

The v2 journal prefix seals a domain-separated, strict-schema capsule containing
the complete anchor document, its canonical digest, the original sealed blob
digest, prior chain hash, absolute sequence, segment and transaction. Store lends
its own Sealer to the same transaction. Backend capacity and suffix processing
use only a privately verified capsule. The current document must match the
anchor exactly at the same revision; a later document must satisfy the original
continuous sequence/revision/authority transitions, exact store/target and a
nondecreasing floor. The fixed cutoff/status cannot change without an authority
revision. Checkpoint itself never advances these fields. Segment and absolute
sequence arithmetic are checked. New capacity subtracts the actual prefix bytes
and counts only complete 448-byte commit pairs; the original revocation reserve
remains in Store publication logic.

Publication writes an invalid Processing header and syncs, writes the bounded
capsule and exact length and syncs, then writes the committed header and syncs.
It finally rereads through the authenticated reader and verifies the original
blob FileIdentity. An uncertain write/flush/read poisons the original backend;
Store Unknown is sticky, including uncertainty before the first write. It cannot
append a rescue record. Fresh readers accept a complete verified new format or
fail closed; they never scan for an old suffix or silently fall back to v1.
The old v1 parser rejects the v2 magic. Repeated checkpoints support both empty
and nonempty suffixes without changing the original blob or resetting sequence.
The anchor transaction is retained in the new segment's uniqueness set; earlier
transaction UUIDs are not retained as an unbounded historical set. Absolute
sequence and the sealed prior-chain binding replace that historical retention.

The isolated tests use FakeSealer for format/state/IO verification, not evidence
of DPAPI cryptographic strength. Fault hooks perform real partial journal writes
or stop at actual publication boundaries, then independent child processes use
original Store authenticated reads. Their successful classification is not a
physical power-loss simulation. Processing/body/header partial writes, length
and sync boundaries, final reread uncertainty, cancellation before writes and
before final publication, truncated/corrupt capsules and appended old tails are
covered. Complete bytes after a late injected uncertainty can be read by a fresh
process while the originating instance remains Unknown. Both whole-filesystem
rollback of an old valid capsule plus its old blob and unobserved wall-clock
rollback remain outside the guarantee; DPAPI is not a monotonic external anchor.

Evidence before the final regression pass: `unattended-checkpoint-behavior-red.log`
contains the real near-capacity Capacity RED (the earlier first-red log is only a
visibility compilation error). `unattended-checkpoint-first-green.log` verifies
normal v1-to-v2 continuation; `unattended-checkpoint-cuts-second.log` records the
real write-cut/reopen matrix; `unattended-checkpoint-codec-first.log` adds anchor
structure and overflow checks. The final same-source pass in `checkpoint-final-UnattendedCheckpoint_.log`
passes all six primary cases. Four affected regression groups pass Store 29,
Capacity 10, Custody 5 and SingleRun 10, for 60 distinct primary tests total.
Dedicated ignored child helpers are actually invoked by their parent tests and
are not counted again. `checkpoint-final-ledger.json` lists the exact tests;
`checkpoint-final-source.json` records the eight source hashes and
`checkpoint-stage-files.txt` the nine-file commit scope. Strict Clippy passes in
`unattended-checkpoint-strict-final.log` (29.50 seconds). The earlier final-first
run preserves a test-only incorrect fixture path failure, subsequently corrected
to the original backend target path without changing the Unknown assertions. No result
here claims automatic reclamation, practical long-duration availability,
reconnection, real-device validation, or public deployment.

## A6o — 同一委托 Session 的有限正常关闭重连

本片新增独立的内部 ReconnectSession 最终批准模式，父授权仍不超过 60 秒，
绝对截止不重新锚定。Local、ShortNetwork 和 NetworkSession 均不隐式升级；
现有产品 UI 仍只提供原短会话入口，没有开放重连或长期无人值守。

原 App 唯一 worker 收到当前 socket 终态通知后，先消费原 Session 的完整
退休观察，再等待原 key custody（含 notifier）的确证退出；之后重新核父取消、
固定截止、精确配对 CAS、原 Store.observe 和 captured run，创建新短材料并经
同 Session.prepare_next 重新认证。每代 request gate 只随不可克隆的 Start →
Prepared → pump 移交，旧 pump 保留旧 gate，不将旧 attempt 改绑新代。
Condvar 通知仅唤醒，不授权；迟到旧代通知检查当前 socket，不额外打开第三代。

真实二代测试发现原 wire 每连接随机生成 Host activation/incarnation，而原
NativeHost rebind 必须保持同一 Host 生命周期身份。现私有 HostLifetimeIdentity
在原 Session 登记前以 fallible 系统随机源生成并绑定 Session brand，只稳定这
两个 Host 生命周期字段。Start 仍核原 operation/admission/预算；TLS exporter、
Grant、nonce、lease 和 attempt 每代独立，未放宽原 same_host 校验。
生产随机源经现有 rustls ring provider 获取，不新增依赖。首次 strict 暴露直接
引用仅 dev-dependency ring 的非 test 构建问题，已保留失败日志并修正。

验收使用真实 Chrome 隔离虚拟认证器、生产 Source/EntryWorker/WASM、原 Relay、
真实 Host 和原 PTY。合成 HTTPS origin 与 Node fixture-CA 外层转发不证明浏览器
可信 TLS、真实手机或公网可用。正常两代各完成认证、Installed、1920 cells 快照、
窗口与一帧输入；第二代使用新 operation 和连续输入序号。fixture 等原第二 Host
Accepted 后才启动第二 Source，这不是生产浏览器自动重试或退避实现。

五个定向场景分别覆盖原非重连模式正常关闭收据、同 owner 两代完整链、收据后
父取消零新材料、私有 Host 身份/跨 Session brand 否定，以及收据消费后新材料
失败。身份单测不是两套独立真实跨 Session 握手。两代用例额外观察实际 worker
处理迟到 wake，确认仅两代 key open。材料失败沿原 Unknown 规则保留父关闭、
无第二 Installed、原任务及 OS custody，第二实例 claim 仍 Busy；独立子进程退出
只是测试清理，不是正常 App/PTY 退休。此失败状态仍是产品可用性限制。

本片不改变 socket 错误分类，不吞 wait_closed 的 Err，不承诺任意传输 EOF、
协议故障或时钟/授权失败可重连。没有调用真实 checkpoint、降低 observed floor
写频、重置 journal 或扩大撤销储备；长期预算与实际浏览器自动重连入口仍待验证。
首行为 RED 保存在 reconnect-two-red.log；native rebind 诊断明确定位生命周期
身份失配，临时 host/transport 诊断随后还原。格式化前五项通过记录为
reconnect-negative-final-first.log（5/5，27.60 秒），不冒充最终同字节源码证据。

收口回归另外发现 A6m 已存在的 Manual 兼容缺口：Socket setup 无条件调用
live_keepalive_fact，而该 API 对未协商 keepalive 返回 Role 错误，不是 None。
reconnect-final-DelegatedReceipt_.log 保留原 1 通过 / 3 失败。修复只按原 wire
请求模式分支：Manual 不签发 InstalledData；live 仍执行原角色、时钟、协商与
事实验证并原样拒绝错误，不把缺协商当作降级成功。旧 Manual 因此不会获得
新长数据权，原三份待处理 fixture 改动仍保持字节不变。

最终兼容修复后同源码验证：Receipt 4、Renewal 9、Reconnect 5、原 Network 数据
寿命 6，以及 Native 6、Network admission 5、Socket 8、HostReady 7、HostStages 12、
App 9、Final 2、Reservation 5、SingleRun 10，共 88 个不同测试。前四组日志前缀
reconnect-compat，后九组为 reconnect-verified；reconnect-final-ledger.json 按原
最终 harness 结果及去重名称核对，不重复计算子进程输出。严格 Clippy 最终通过
reconnect-strict-final.log（29.57 秒）。reconnect-final-source.json 记录 12 份源码
指纹，reconnect-stage-files.txt 限定 13 文件。两份临时 NativeHost 诊断源已与保存
原字节逐项核对相同；原三份待处理改动与生成产物均排除本片提交。


## A6p：原终因分类与异常截断恢复（内部有限会话）

本片先闭合 A6o 的恢复分类，不接浏览器自动重试，不改变旧批准、父绝对截止、
原两秒初始化窗口、每代完整认证或 journal 写入语义。只有独立 ReconnectSession
使用新分类；旧 Local/ShortNetwork/NetworkSession 不升级为重连批准。

原 RunningChannel.wait_closed_checked 返回 Result<DataError, DataError>：外层成功
只证明原 channel owner join 后返回了终因，不表示业务成功。原 socket 在 wire
关闭/join 后读取缓存终因，原材料收口仍独立执行；只有 Some(Ok(Io)) 是候选。
None、Closed、Codec、Grant、其他终因或外层错误先关闭父门，再发布 terminal 与
通知原 App worker。候选仍必须原 CompleteRetirement、key/notifier join、当前父门、
固定截止、配对 CAS、Store.observe 和新一代完整认证；分类不制造任何退休收据。
Closed 可由本机 stop 或队列关闭产生，因此不能作为正常 peer 断开的证明。

固定 tokio-rustls 0.26.4 common/mod.rs 将 process_new_packets 的协议错误包装为
io::ErrorKind::InvalidData；rustls 0.23.46 conn.rs 将没有 close_notify 的截断报告为
UnexpectedEof。原 security Engine 三处 TLS read/write/flush 将全部错误压成 Io，
会把坏 TLS 认证记录也误认为恢复候选。现统一有限映射：InvalidData 为 Codec；
UnexpectedEof、ConnectionReset、ConnectionAborted、BrokenPipe、NotConnected 为 Io；
其余（包括 Other、WriteZero、未知 kind）为 Uncertain，写入返回零也走同一映射。
不改原 owner 的 stop 优先级；并发错误可能被观察为 Closed，但该结果仍拒绝恢复。
UnexpectedEof 可以来自恶意截断，绝不等于善意断网证明。WASM 适配器的 Other
同样保守为 Uncertain，不保证任意浏览器传输失败均能恢复。

新增真实异常截断用例在 Installed、快照、输入与 settle 后终止原隔离代理 socket，
观察原 Io、完整原收据及同 Session 第二次认证/Installed，两次输入各执行一次。
这是受控传输截断，不是所有平台 TCP RST 的穷尽验证。另有测试终因 Codec 注入，
只证明 socket 分类与父关先于 wake，不冒充真实 TLS 验证。

坏 TLS 用例在真实生产 Source/EntryWorker/WASM 已 Installed 且输入 settle 为 Idle
后，仅翻转下一条 TLS application ciphertext 的一个字节。RED 日志
reconnect-bad-tls-red.log 记录原 Host Io、错误开启下一代（opens=[2,2]）；
GREEN reconnect-bad-tls-green.log 记录实际翻转 192-byte TLS record、原 Host Codec、
父门关闭、opens=[1,1]，并收齐原 Source/Worker/enrollment/socket 与指定 App/PTY
退休事实。它不是通过超时、主动关闭父门或伪造终因获得通过。公开测试向量、
隔离虚拟认证器、合成 HTTPS origin 与 fixture-CA 转发的边界沿 A6l；不代表真实
手机、浏览器可信 TLS、公网或长期无人值守验收。原失败日志均保留。

当前生产 WASM 按 build-browser-runtime.mjs 从本片源码重建，脚本验证必需导出与
无 TEST_ONLY 导出，真实浏览器 fixture 再核 manifest 的 source/file/input SHA。
生成物保持 ignored，不与源码提交。单元映射表不替代上述实际坏 TLS 反例。
浏览器固定输入/provider 的父 lease、每代资源 lease、有限退避与不确定输入锁
尚未实现，本片不会把已撤销 enrollment 重新使用，也没有自动重放输入。


回归中保留一个未宣称已修复的前置竞争：首轮 DelegatedRenewal_ 为 8/9，003
在 phone install 之前因 Captured.validate 的 native_canonical.try_lock 返回 Busy
而失败（reconnect-tls-final-DelegatedRenewal_.log）。该组每例在独立子进程执行，
父进程确认前例退出后才继续，不共享前例 canonical 对象；尚未动态识别本例当时
具体持锁者。未改原 try_lock、预算或原 dirty fixture；独立复核 1/1（0.98 秒）及
限定一次整组复核 9/9（8.59 秒）。这些成功不抹掉首轮失败，也不证明竞争已消失。


最终同源码验证：Host 六组 8+1+4+9+5+6=33，security experimental lib 144，
去重共 177（reconnect-tls-final-ledger.json）；独立 ignored 注册 oracle 不计。
首次 security 默认 feature 为 0 tests，明确不计覆盖，已补 experimental。
Native strict 32.47 秒，security experimental strict 3.07 秒均退出 0。
五份源码指纹为 reconnect-tls-final-source.json；生产 manifest 65 个源文件及所有
input/file SHA 全部匹配，六个必需导出存在且无 TEST_ONLY。只提交六文件清单，
原三份 dirty 文件 SHA 不变，生成物与审计日志不纳提交。


## A6q — explicit bounded Browser retry (local candidate)

The default Browser source and Host UI remain unchanged. The new controlled
reconnect entry reads the original operator module and approved provider once,
then retains that same enrollment as the parent of per-generation resource leases.
Only explicitly approved `rendezvous-v1`, Live and SnapshotPull configuration is
eligible. Legacy configuration is rejected before any socket/Worker is created;
there is no implicit profile upgrade or fallback. Local retry policy is not Host
consent: the existing explicit ReconnectSession and its fixed deadline remain
mandatory on every independently authenticated Host generation.

The local monotonic budget is at most 60 seconds, fixed when the controlled read
creates the session. The initial controlled read retains its separate bounded
setup deadline. At most three replacement attempts use 250/500/1000 ms delays;
the attempt is charged before waiting and is never refunded. A failed new setup
ends this parent rather than repeatedly invoking authentication. No Host lifetime
is inferred or displayed from the local budget.

One original TerminalClient is retained. Only its exact generation's external
invalidation can nominate an idle/settled reconnect candidate. Manual close and
internal failures clear this candidate before invalidation callbacks can run.
Pending input, an active operation, uncertain renewal, or an unsettled receipt
precludes retry. A candidate is not a retirement receipt: the original connection
and per-generation enrollment must finish their original drains, followed by a
fresh check of the parent, candidate, fixed target and role. TLS binding changes
with the new authentication. Input is never replayed or cleared to permit retry.
Cancellation closes the parent first, cancels timers, and retains late opens until
the original resources are drained. A failed drain remains Unknown.

The first autonomous legacy experiment failed: a new Phone was admitted while the
old Host still occupied the static route; the new ClientHello caused the old TLS
owner to report Codec. `browser-retry-causal-real.log` preserves registration
order and length-only send observations. This is not solved by increasing backoff
or accepting Codec. The test had used the older Relay checkout, not a missing new
protocol implementation. The existing clean `feat/rendezvous-v1` worktree at
`6382ffe1fbde46ff9bb5586fb25d7c92e5617f2d` already provides mutually requested,
bounded JOIN/READY and closes the original pair on disconnect. It was built in an
isolated target and reused without Relay source changes, merge or deployment.
Its original 55 tests passed. Independent review confirmed single-key registration,
unique guard ownership, registry locking and distinct per-connection queues prevent
old Drop or queued data from migrating into the new pair. Existing in-flight writes
on the old socket may persist for their bounded write interval; this is not zero
in-flight delivery. Old Relay negotiation is rejected, never silently downgraded.

The first rendezvous test exposed a fixture-only circular wait: its initial GO
waited for Host Ready, which now requires the Browser Join. Only the new case now
starts the original Browser child after final approval and App reservation, in
parallel with the original first Ready observation. No budget is re-anchored.
The second generation is driven solely by the production Browser timer, with no
HostAccepted fixture notification. After both inputs are settled, the test explicitly
closes the original Host parent and then drains the Browser. This avoids counting
a legitimate third Host waiting attempt as a failure of the two-generation test.
Without that final parent close, source closure can cause the Host to prepare a
new generation whose bounded setup fails if no client arrives. This remains a
product availability limit, not a promise that a tab can return at any later time.

`browser-retry-parent-close-real.log` records one distinct passing real-chain case
(the nested process harness does not double its count): one page, original operator
reader, production Source/EntryWorker and production manifest/WASM, two real virtual
WebAuthn assertions, two 1920-cell snapshots, two settled one-byte PTY inputs,
exactly two opens of each original key and original Source/Worker/enrollment/socket
and specified App/managed-run retirement facts. The final Host close is only test
shutdown after the two generations; it does not coordinate either retry.
The browser origin is synthetic HTTPS, with a CA-verifying Node WSS proxy and
public fixture credentials. This does not establish browser certificate trust,
a real phone, public-network operation, long-term unattended usability, or a
production automatic-retry UI. The original three unrelated dirty files and all
ignored generated artifacts remain excluded from the stage commit.

Final verification on the recorded source: Node 51, Vitest 183, original short-network
4, original reconnect 8, and the new real same-page case 1: 247 distinct CC cases.
The reused Relay's 55 cases are separate, not included in that total. Project
TypeScript completed with exit 0 and native strict Clippy finished in 31.24 seconds.
The exact source manifest, nine-file stage list and log ledger are
`browser-retry-final-source.json`, `browser-retry-stage-files.txt` and
`browser-retry-final-ledger.json` under the local audit directory. The real case
requires the Relay root and executable from the explicitly built clean rendezvous
worktree above; the older default legacy fixture binary is intentionally unsuitable.
No Relay files or generated runtime binaries are part of this CC stage.

## A6r: bounded waiting before endpoint TLS (internal, default off)

The existing explicit ReconnectSession can now replace an uninstalled attempt
whose original local rendezvous deadline expires after Admission and JOIN have
completed, but before a socket can be handed to endpoint TLS. This is not an
Installed receipt. The connector retains a private observer identity and exact
original deadline; observed EOF, malformed READY and other failures are sticky.
A late valid READY is not delivered to TLS. The phase observation alone cannot
establish resource retirement or authorize a replacement.

The original SocketCustody must first finish its task and material cleanup, with
no handoff, channel or Installed fact. A separate non-Clone uninstalled receipt
then owns the original Start, checked against the current Session, operation,
attempt and socket. The original App worker joins the old key owners, checks the
unchanged parent, pairing/CAS and durable policy, and consumes that receipt once.
It does not manufacture CompleteRetirement or rebuild the delegated owner. If an
Installed receipt was already consumed before this waiting attempt, its original
RetiredInstalledChannel is carried forward for the later rebind. Successful first
adoption after waiting also completes NextStarting; foreign attempts cannot clear
that state, and initial Empty remains a no-op.

At most three waiting replacements are allowed, charged without refund. The
original parent deadline (at most 60 seconds), native setup deadline, Relay's
pending timeout and initial I/O window are unchanged. The local eight-second
setup deadline is frozen before the outer connection and Relay registration;
the Relay's own eight-second pending interval starts later. Only the exact local
phase expiry is eligible. Earlier Relay policy expiry or EOF closes the parent,
not a retry. This is bounded availability, not waiting indefinitely or restoring
across an App restart. Old Local/ShortNetwork modes and default production UI do
not gain waiting or reconnect permission. Ordinary Store.observe/floor writes and
revocation capacity remain unchanged; no automatic checkpoint is enabled.

Evidence retained in the audit directory includes the original late-client RED,
the real PendingReady expiry probe, and the first composite failure that exposed
NextStarting not being completed after a first adoption following a wait. The
final four real cases cover: a client arriving after nine seconds followed by the
original autonomous browser retry; three waiting replacements and closure before
the parent deadline; an Installed generation followed by an absent client and a
later rebind of the original Host; and cancellation after the waiting receipt with
no next key open. The second rebind case uses a timer, not a second HostAccepted
fixture signal. Its initial missing test callback was a fixture failure, retained
separately from the corrected result.

The original no-client fixture used a 30-second Relay test policy. It closed
safely before four full eight-second waits, so four observed opens alone did not
prove the replacement-limit branch. The strengthened assertion correctly failed.
Only that case now uses the existing isolated fixture policy lifetime of 45
seconds; production timeouts, the fixed parent deadline and the 40-second child
process watchdog were not changed. The final log must contain the exact
WAITING_RETRY_LIMIT_REACHED marker, four opens per key and the original cleanup
facts. The earlier 30-second result remains evidence of earlier external policy
expiry failing closed, not evidence of counter exhaustion.

The browser cases use public fixture credentials, anonymous nonpersistent CNG,
virtual WebAuthn, a synthetic HTTPS origin and the existing CA-verifying Node WSS
proxy. They do not establish browser TLS trust, a real phone or public deployment.
Retirement claims are limited to the named original Source/Worker/enrollment,
network/material/key and App managed-run facts, not an assertion about every
platform resource. No real journal, registration or key was modified.
Final verification is deliberately split by source revision. After the two
semantically equivalent Clippy collapsible-if rewrites in wss_admission, the four
real waiting cases and all 70 transport cases (43 unit plus 27 integration) passed
on the recorded final source. Transport and native strict Clippy finished in
1.96 seconds and 31.69 seconds. The 41 original native regression cases passed
before those two lint-only rewrites; they are not described as byte-identical
final-source runs. Combined distinct coverage is 115, not counting nested process
harness duplication, earlier repeats, or zero doc-tests. The final real chain log
is waiting-final2-four.log; the source, stage list and layered evidence ledger are
waiting-final-source.json, waiting-stage-files.txt and waiting-final-ledger.json
in the audit directory. The original three dirty files and generated artifacts
remain outside the proposed 15-file stage. No stage or commit is implied here.