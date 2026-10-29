# Host journal: first E07 component, not a running remote Host

This backend-only crate stores bounded operation metadata, never terminal input,
argv/env, session titles or model output. It has no listener, endpoint enrollment,
grant validation, PTY, automatic retry or desktop integration. `run_once` requires a
trusted backend actor to supply an already-authorized exact OperationKey and keyed
request digest; its callback is not a network tool-execution API. HostWritten records
Host write acknowledgement, not CLI completion. Shared wire DTO freeze remains E01.

Before any target access, the actual linked SQLite must match3.53.2/source ID
`2026-06-03 19:12:13 d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24`,
thread mode and safe compile options. Pinned rusqlite0.40.2/libsqlite3-sys0.38.2 use
bundled source; build.rs rejects external SQLite override variables. Runtime evidence
is qualification of revision/configuration, not an audited binary or power-loss proof.

Current path profile is Windows local fixed storage. Native OPEN_REPARSE_POINT
attribute handles pin every ancestor without DELETE sharing, reject reparse/hardlink/
ambiguous path forms and retain the exact database name for the Connection lifetime.
Existing sidecars are pinned too. This does not rely on SQLite's NOFOLLOW flag on
Win32. It requires a private trusted backend directory, and is not a sandbox against
malicious code with the same user identity or a rollback-attestation mechanism.
Non-Windows returns UnsupportedPlatform before file creation; its Host support gate
is not passed. No ACLs, system keys or persistent access grants are created here.

Owned file admission checks before SQLite processing: main64MiB, WAL16MiB, SHM1MiB,
rollback-journal64MiB. These are checked thresholds; final-result space reservations,
strict live WAL-growth bounds and full-disk qualification remain OPEN. DB page cap
is64MiB, op cap4096; once full, no automatic eviction or lost-evidence compaction.
Existing databases require persisted app ID/schema version and exact tables/indexes,
read-only verification before mutable PRAGMAs, then revalidation under each write lock.
New initialization is atomic and checkpointed before admission. Interrupted setup may
leave an unusable owned empty/partial file requiring explicit recovery; never delete
an uncertain existing database or silently recreate its floors.

WAL/FULL intent+admission floor commit occurs before the callback. One unresolved
Pending/Partial/Unknown per device/target scope prevents a new sequence; operation ID,
full target/device/sequence and request digest match before returning prior status.
Sequences use8-byte big-endian blobs with checked advancement, not signed truncation.
Final outcome and CSPRNG32-byte receipt token share one transaction; nonfinal has no
token. The transaction verifies schema, floor and full readback before success. Any
uncertain post-effect persistence returns Unknown and poisons the current object.
Stored token is sensitive metadata and is never printed. Querying a key does not
attest arbitrary newly supplied bytes; authentication/scope policy remains upstream.

Actual tests use owned temporary SQLite files and harmless process exits at before-
effect/after-effect/after-result cuts, independently counted effect markers. Storage/
trigger corruption, malformed rows/proofs, foreign and false-schema files, cap limits,
real junction/hardlink and handle pinning are covered. Process exit is not power loss.
Tests need standard-user ancestor attribute access. On the current WSK-MSI candidate,
the default temporary-root probe returns Windows OS5 from CreateFileW requesting
FILE_READ_ATTRIBUTES on ancestor index3, before SQLite. This is an OS API failure
propagated by the guard, not a tool-approval refusal; its permission/environment cause
is unresolved. No ACL or alternate API bypass is used. Tests with process-local
TEMP/TMP under the writable task directory provide conditional fixture evidence only.
They do not qualify the default root or select a production storage directory.

The split API commits intent in `prepare`, releases the database transaction, then
accepts a consumed, non-cloneable `PreparedOperation` in `finalize`. Its token binds
the exact live Journal, operation scope/sequence/digest and retains the cooperative
directory owner lease. Dropping the Journal while an effect is in flight cannot
admit a replacement owner until the token is gone; reopening preserves Unknown
instead of repeating an uncertain effect. One checked empty owner sidecar locks the
trusted backend directory before any database is created. This is cooperative
single-owner protection, not a sandbox against malicious same-user code.

`run_once` remains a compatibility wrapper. A caller holding its own Mutex around
that wrapper still holds the Mutex during the callback; a Host actor must use short
prepare/finalize lock scopes and perform blocking IO outside that lock. No production
actor or PTY integration is established by the split journal tests.

## Default-off local input prerequisite

`experimental-local-writer` adds an internal, crate-only constructor for one owned
pipe writer and its Journal. This is not exported as a raw IO enrollment API. Each
submission binds the full target/run/generation/incarnation, device, operation,
sequence, epoch and payload to a keyed digest. The key supplied by a future trusted
backend is not endpoint identity or authorization; tests use explicitly public keys.
Inputs are nonempty and at most16384 bytes. Eight queue slots plus one active/result
credit bound retained requests, including pre-send copies and unconsumed feedback.
Unknown/Partial seals the writer; dropped feedback permits exact receipt lookup,
never automatic repetition. HostWritten still means only the Host write completed.

Fixed Windows10 pipe/SQLite tests cover duplicate/lost feedback, changed bytes,
identity/epoch/size refusal, seal, queue pressure and post-effect storage failure.
Resize and terminal replies remain Unsupported; there is no PTY, remote grant,
network listener or active-desktop migration. Seal requests refusal and cannot undo
an already-started write. Drop requests seal but does not certify thread retirement;
the backend must stop its owned process and observe actual thread/IO completion.
This single-journal writer is not the future shared multi-session actor and does
not establish cross-session progress or a general synchronous-write deadline.

The internal schema is now version2. New databases persist independent admission and
settled floors plus the latest bounded ACK witness. Existing version1 databases are
rejected read-only before mutable PRAGMAs; there is no silent migration or deletion.
Final receipts expose an opaque redacted token only from an exact durable row.
Settlement accepts1..32 contiguous exact final proofs for one full target/device scope,
persists the settled floor and latest-batch witness before deleting those final rows,
and never reduces the admission floor. The caller supplies an expected device obtained
from a future authenticated outer actor separately from the ACK's self-reported device;
this crate does not authenticate either value. A lost response can replay the latest
exact committed batch and receive AlreadySettled; older batches safely reject after a
later advance. TooOld says only that a target/device sequence is at or below the durable
settled prefix. It carries no proof and does not attest that a supplied operation ID
existed. Floor scopes and live operation rows are each capped at4096 without eviction.
Accepting32 backend proofs does not establish that a future JSON envelope fits the
16384-byte wire budget; the wire encoder must independently size and split/reject it.

Actual process-exit tests cover ACK-before, uncommitted settlement transaction and
committed-but-response-lost cuts. The independent effect marker remains single-shot.

OPEN before E07/Host production acceptance: authenticated receipt transport/wire DTO
adaptation, irreversible target retirement, rollback freshness/old-backup handling,
strict storage reservations/full-disk/fsync faults,
Windows11/NVMe latency, single Host session actor/fencing, true IO/tree quiescence,
native policy/keys and E08 admitted launch bridge. Do not enable remote control from
this component alone. journal-crash-fixture is test-only and must not be distributed
as a Host executable; release artifact allowlisting remains a separate gate.
