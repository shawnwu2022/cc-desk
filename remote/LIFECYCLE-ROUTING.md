# Independent lifecycle dispatch candidate — memory qualification only

This module closes a bounded codec/dispatch seam. The default-off durable candidate
is specified in [LIFECYCLE-DURABILITY.md](LIFECYCLE-DURABILITY.md). It registers no native backend,
listener or phone entry. Existing writer-router WIP and real PTY/handle gates are
not accepted by these tests. No CLI version is constrained; profile1 is a wire
schema profile. Complete RemoteMutation/protocol freeze remains OPEN.

| Request | Internal Host method | Memory qualification | Native release gate |
| --- | --- | --- | --- |
| Create | create(context, request) | exact kind/identity; duplicate does not requeue | trusted approved launch recipe, durable launch receipt and actual custody OPEN |
| Attach | attach(context, request) | exact expected binding/state and local right | native attach/takeover and snapshot/ACK integration OPEN |
| Resize | resize(context, request) | bounded dimensions, exact generation/fence/epochs | owned resizer, actual single-writer ordering and epoch publication OPEN |
| Resume | resume(context, request) | calls Resume, never translates to Create | durable reconciliation across reconnect/crash OPEN |
| Close | close(context, request) | exact identity; Unknown freezes new admission | owned process-tree/IO retirement and durable close result OPEN |
| Reply | encode_reply/decode_reply | preserves request kind/op/seq/state and typed status | transport reply dispatch/mobile consumer OPEN |

## Explicit authority and behavior

Constructor requires an actual live Host-side Controller BoundDataAuthority,
plus the local resource owner's trusted Expected snapshot: full AppBind context,
principal, exact session/run/generation scope and current fence/mode/dimension/
sync state. Expected must come from that resource owner, never the request or a
copy made from an untrusted DTO. Full context equality includes Host/device key
identities/epochs, activation/incarnation, policy/issuer and origin. Constructor
and every dispatch check live proof. The received record must retain that exact
native channel and scope, with the current fence. ManagementOnly has no such proof.

Controller does not imply lifecycle permissions. A separate local Rights value
defaults to deny all, is not deserializable and has no production allow-all
constructor. Native integration must derive rights from local per-operation
policy/approved capabilities, and revalidate live proof at actual mutation commit.
No arbitrary argv, path, account or recipe is supplied by the wire request.

The trusted Host interface defaults each operation to Unsupported. Implementations
must be nonblocking queue-admission methods; this router does not protect against
a malicious/blocking implementation. No actual native implementation is registered
or certified. Installing one remains blocked on finite queue/IO/custody tests.
Accepted means only queue admission, never started/resized/resumed/closed or CLI
completion. Unknown is never translated to success/no-effect/retry permission.

One cached reply and monotone sequence provide bounded deduplication within this
router lifetime. Same op or sequence with altered contents conflicts. Exact retry
returns its cached reply after live authorization checks. Accepted/Unknown allow
no distinct request. Unknown/pending is reserved before calling the backend; a
backend unwind cannot reopen admission or lose the exact retry's Unknown state.
Rejected is a trusted no-effect result; only then may a new sequence be admitted.
No operation is automatically retried and no current epoch replaces an old one.

The memory-only constructor is test-only. Product construction now requires the
durable adapter, its distinct local metadata store and backend-owned immutable
ResourceIdentity. The cache itself remains per-router; the separate journal
prevents a recreated fake router from requeueing the same unresolved operation.
Reopened Prepared/InFlight/Accepted become Uncertain, not retryable. Accepted
is queue admission only; native completion/reconciliation is still unimplemented.
Only the internal fake Attach channel in ATTACH-COMPLETION.md can release the
same live router's pending gate after durable completion. Other accepted operations
continue to block new work. Resume cannot bypass this.
No real process-crash, power-loss or native at-most-once claim is made.

## Strict candidate wire contract

Requests are flat strict UTF8 JSON: message Create/Attach/Resize/Resume/Close,
profile1, nonzero lowercase16-byte hex operation, canonical decimal strings for
sequence/generation/fence/sync (nonzero) and modeEpoch/dimensionEpoch (zero allowed).
Rows/columns are present only for Resize, non-null integers, rows1..128,
columns1..256 and at most16384 cells. These are candidate payload limits, not
measured terminal/device support. Wire identities/target are supplied by the
enclosing authenticated scope and trusted Expected snapshot, never caller claims.

JSON is capped at16270 bytes before parsing, retaining114 bytes for the existing
bound envelope. Typed decoding rejects unknown/duplicate fields. Semantic shape
validation rejects explicit null dimensions and serde's alternate enum maps, for
both requests/replies; ordinary whitespace and key ordering are valid. Reply is
LifecycleReply/profile1 with the exact request and Accepted, Unknown or a closed
Rejected code. No raw backend error or terminal content is returned.

Tests use only real TLS/AppBind in bounded memory streams, public test keys and a
fake Host. They check each mapping, round-trip identity/errors, default deny and
Unsupported, wrong principal/client role/session/run/device/activation/policy,
all state mismatches, different source binding, revoke, duplicate/conflict,
Unknown/Pending and backend-panic ordering. No PTY, CLI, process, network service,
credential, ACL or unknown failure gate is exercised. Shared public test identities
are loaded once for existing and new fixtures; this import consolidation does not
include the remaining writer-router WIP.

Run only this independent slice:

```powershell
cargo +1.98.1 test --manifest-path remote/Cargo.toml -p cc-desk-remote-host --all-features --locked --offline --lib lifecycle_
cargo +1.98.1 test --manifest-path protocol/remote/inner/v1/Cargo.toml --locked --offline
```

Real PTY concurrent-root-unknown, cold-spawn/handle138→150, default-root OS5,
WebView, signed phones, real CLI, actual reply delivery, multi-Host capacity and
operational acceptance remain OPEN. Do not enable production remote control from
this candidate or treat successful fake calls as closing those gates.
