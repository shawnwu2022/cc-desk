# Internal fake Attach completion candidate

No native backend, wire change, real database migration or remote enablement.
New candidate test stores use schema2 with atomic typed result persistence.

A private bounded mailbox holds one exact result. Captured binding includes the
full request (operation, sequence, generation, before-fence/epochs), resource
identity, request HMAC and adapter-instance capability. Wrong binding rejects.
The fake successor preserves generation/mode/dimension and increments fence/sync
exactly once with overflow checks. It models installed peer and required fresh
sync, not input eligibility, a new grant or verified native rebind semantics.

Sender reporting only deposits local evidence, not a durable acknowledgement.
It may precede the Accepted write. Owner polls after dispatch, checks current
backend identity, and commits Accepted -> Completed through the original ticket
before changing router cache/state/admission. Duplicate evidence is idempotent;
conflicts cannot replace the first fact. Uncertain cannot be completed by callback.
Store failure leaves pending/cache closed. Result and Completed are written in
one transaction. Input receipt interpretation is unchanged.

An interruption after commit/before cache can be retried on the same live owner.
Historical fact recording is separate from attached() disclosure, which requires
current live authority. New admission clears previous result and retires its
mailbox; Receiver Drop rejects old cloned senders. No wire delivery is implemented.

## Typed result recovery: P2 closed for fakeHost only

Schema0x4c4a0002 adds results: bounded ATC2 payload contains the full canonical
Attach request and all five after-state fields; backend32 and a domain-separated
HMAC bind payload to resource scope, operation, sequence and request digest.
Both historical reads and new admission authenticate prior Completed results.
Corrupt results block the resource; they never become retry eligibility.

Exact replay on reconstruction recovers the same authorized internal Applied
result with no backend call/mailbox, then permits next-sequence progress. Wire
reply stays historical queue Accepted; typed result is internal attached() only.
Live source/scope, per-operation Rights, backend/resource identity and before/after
state checks remain required; disclosure rechecks authority. Old capabilities
remain stale and cannot complete the reconstructed Store.

Schema0x4c4a0001 is recognized by exact SQL fingerprint and opened read-only.
No DDL or automatic upgrade occurs; old incomplete completion remains Uncertain
and cannot admit new work. New schema2 Completed without a result is demoted to
Uncertain on reopen. Unknown schema rejects before writable opening. No production
database was read/migrated, and no migration command is provided.

Stable backend ID is an owner-supplied identifier for the SAME surviving or
independently reconciled queue, never a library/CLI version or regenerated ID.
Public fake ID is not an access credential. Persisted history is not proof a real
PTY survived. Real registration needs owner-derived runtime state, persistent
identity/key policy, reviewed migration/backup recovery and platform crash testing.
Tests simulate drop/reopen, including commit-before-cache; no process/power cut.

Create/Resize/Resume/Close completion are outside this channel. Native runtime is
still only a Host dev-dependency. Desktop WebView CallerIdentity cannot be forged
for remote use. Wire128x256 versus native/TerminalCore100x240 bounds and u64/u32
generation conversion remain explicit future gates; no clamp or truncation.
Close root exit/writer join is not full process-tree/IO retirement.

Tests: fakeHost, public bounded-memory TLS fixtures, temporary SQLite; early/late
results, duplicates/conflicts, all request/resource/state bindings, instance/digest,
owner drop, replaced mailbox, Uncertain, commit failure, same-owner commit/cache
cut, revoked disclosure and persisted/reconstructed states. No real PTY/process,
CLI/account, external network, permissions, persistent keys or unknown gate reruns.
