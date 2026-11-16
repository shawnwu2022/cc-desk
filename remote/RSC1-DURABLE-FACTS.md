# RSC1 durable facts: fake-only candidate

This slice adds a private completion owner and journal support. Its owner and v3
creation factories are test-only. Tests use synthetic TerminalCore, public
memory TLS fixtures and new temporary SQLite. There is no native MasterPty,
production route, persistent grant, database migration or Create/Close path.
The existing writer actor is unchanged; this is fact-contract qualification,
not admission of a durable Resize route to that actor or a production runtime.

## Format and schema

ATC2 bytes and Attach successor semantics remain unchanged. RSC1 is a separate
stored format, not a wire message or a new wire Completed response. Its bounded
binary encoding contains the canonical complete Resize request, backend ID,
request digest, full resource identity, before/after size, before/after complete
TerminalEpochs, after wire state and Applied/NotApplied discriminator. The
existing lifecycle result MAC authenticates the entire tagged payload plus row
scope/op/sequence/digest/backend. Maximum payload remains 4096 bytes.

Applied requires the exact requested size. Changed size increments dimension
and structure once; mode/generation/fence/sync do not change. Same size changes
no epochs. NotApplied is a trusted backend assertion of no size/core mutation,
not an inference from HRESULT, missing reply or timeout. There is no serialized
Unknown result: Unknown remains an unresolved Uncertain operation.

Requested geometry must meet wire128x256/cells16384 plus core100x240/min-columns.
Before geometry records actual core size, so an existing100x240 core can be
represented even though a new Resize request cannot target that cell count.
Stored resource strings and canonical integers are validated; unknown tags,
wrong kind, trailing/truncated/noncanonical data and invalid transitions reject.

Store::open keeps existing v2 behavior and rejects v3. The private test factory
creates v3 (0x4c4a0003) only for a new path. Existing v1/v2/v3 are opened read-only
through that factory. There is no automatic version update or DDL on old files.
v1 has no complete facts; v2 supports ATC2 only, even if an RSC1 row has a valid
MAC. v3 dispatches ATC2/RSC1 strictly. Unknown versions/fingerprints reject.
Existing path/reparse/identity/pinning, directory ownership, key witness,
sequence floors, unresolved index and database size limits remain in force.

## Completion order and uncertainty

An explicitly permitted fake owner checks its held live Controller proof and
trusted resource. It durably prepares/InFlight/Accepts before minting a bounded
completion capability. The fixture invokes its fake only after that capability
is returned. SQL transactions do not span the fake effect. The mailbox checks
the full expected fact, request, backend and resource; identical callbacks are
idempotent, conflicts reject, and dropping the owner invalidates held senders.

The owner observes known completion within the original monotonic budget
(0<budget<=1s), atomically commits result+Completed, then updates cache, then
publishes its private result. Publication rechecks live authority and deadline.
Prepared/accepted uncertainty, cancellation, expiry or store failure freezes the
owner; late callbacks cannot clear it or reexecute the operation. Raw completed
historical facts never imply snapshot/ACK/window or current input permission.

SQLite filesystem commit is not preemptible here. If commit is confirmed only
after the observation deadline, or cache/publication fails after commit, a known
historical fact may exist while live publication is denied and the owner is
quarantined. No rollback, second effect or hard storage-cancellation guarantee
is claimed. A failed attempt to mark uncertainty never permits further execution.

## Recovery and evidence limits

Reopening the candidate yields only a read-only historical query owner. Complete
facts are authenticated against the full request/digest/backend/resource and
current proof before and after lookup. Missing/incomplete results remain unknown;
exact history replay mints no completion capability, does not overwrite newer
trusted state, and does not restore execution/publication/ACK/windows or admit
the next mutation. Grant expiry/revocation denies history disclosure as well.

Tests cover atomic commit/cache/publication cuts; all pre-admission journal cuts;
Applied/NotApplied/same-size validation; pending owner Drop; unknown/late callback;
full identity/request/digest conflicts; altered rows/seals/key/schema; v1/v2/v3
read-only reopening; valid-MAC wrong-format rejection; unchanged ATC2 payload;
and history retaining a later trusted mode state. These are fixture/database
reopen tests, not real process-crash, disk-power-loss or backup-rollback proof.

The prior synchronous native Resize blocking, master/reader/writer ownership,
independent drain, old IO/tree retirement, PTY concurrency/handle growth, OS5,
WebView, real CLI, Windows11/mobile and production gates remain OPEN. No CLI
software-version restriction is introduced. No deployment/push/release occurs.
