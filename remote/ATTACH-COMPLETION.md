# Internal fake Attach completion candidate

No native backend, wire change, database schema change/migration or remote
enablement. Existing lifecycle Store::transition is reused unchanged.

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
Store failure leaves pending/cache closed. Existing journal transition enforces
these states; no schema or input receipt interpretation changes.

An interruption after commit/before cache can be retried on the same live owner.
Historical fact recording is separate from attached() disclosure, which requires
current live authority. New admission clears previous result and retires its
mailbox; Receiver Drop rejects old cloned senders. No wire delivery is implemented.

## Recovery limit: OPEN P2

This is not full crash recovery. SQL stores Completed without a full typed result
or current backend state. Exact completed replay on a reconstructed router retains
historical queue Accepted, calls no backend, creates no mailbox, and stays pending
without an attached result. A specific test pins this fail-closed limitation.
Future owner-derived state plus durable reconciliation/query is required; no
automatic epoch reconstruction, migration or relaunch is introduced here.

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
