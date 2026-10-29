# Host journal: first E07 component, not a running remote Host

This backend-only crate stores bounded operation metadata, never terminal input,
argv/env, session titles or model output. It has no listener, endpoint enrollment,
grant validation, automatic retry or desktop integration. The journal owns no PTY;
the default-off writer adapter accepts a backend-owned IO stream. `run_once` requires a
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
Resize and the user-facing raw terminal-reply entry remain Unsupported; there is no remote grant,
network listener or active-desktop migration. Seal requests refusal and cannot undo
an already-started write. Drop requests seal but does not certify thread retirement;
the backend must stop its owned process and observe actual thread/IO completion.
This single-journal writer is not the future shared multi-session actor and does
not establish cross-session progress or a general synchronous-write deadline.

### Restricted internal Primary DA replies

The private start_terminal factory mints one non-Clone output capability for the
immutable target and a fresh opaque parser activation. Its feed_at(&mut self,offset,bytes) requires
an exclusive producer borrow; the reader receives no PTY writer and cannot submit
reply bytes. Actual output becomes a typed Primary DA event through the persistent
patched-vte adapter. The single worker validates activation, complete batch and
budgets, then encodes static ESC[?6c at its atomic commit point. This matches the
locked Alacritty primary-identity response, not a full VT compatibility attestation.

Limits:16384 bytes/feed,32 queries/batch,32 logical reply commits/rolling second,
5 response bytes/query. Input/output share8 slots and9 active/result credits.
These do not mean32 native Write calls or physical160B/s. Internal replies have no
user operation, sequence, journal fsync pair or ReceiptEnvelope. Lost results never
authorize retry. Parser/rate exhaustion or partial/unknown IO seals the writer.
Stop discards uncommitted replies; stop after part of a batch reports Partial.
Already committed IO still needs owned termination/observation before any new
generation is admitted. The shared write loop permits at most16 Interrupted retries
per logical write, then Unknown/Partial without repeating already written bytes.
This bounds retry-loop CPU, not a blocked native Write/flush or Windows destruction.

A blocked user write can still delay a queued DA reply. Full-pipe startup/global
progress, color/size replies, canonical display projection and complete
E04/E09 remain OPEN. This reply-only parser executes no clipboard, title, URI, file
or unknown native events. The default-off remote/control gate is unchanged.

### Experimental canonical-prefix replies

The additional default-off `experimental-canonical-replies` feature offers a private
`start_canonical_terminal` factory. A non-clone fresh `CanonicalStart` validates
startup geometry (1..100 rows,1..240 columns) before worker creation. The backend
must use that exact PTY geometry; resize and mode-epoch publication are not yet
implemented. It does not replace or silently enable the static reply factory.

The single writer owns the locked emulator, validates the entire output batch,
checks the same activation and stop commit point, and only then consumes a borrowed
query encoder at its exact canonical prefix. It supports Primary DA, status DSR5
and cursor DSR6, with at most10 bytes/reply and32 logical reply commits/rolling
second (up to320 logical response bytes, not a physical throughput guarantee).
Malformed/unsupported tails and batch/rate overflow emit no replies from that batch.
Stop between replies reports Partial; partial/unknown IO seals without replay or
a user receipt. A callback failure or panic permanently degrades that core.

This is still the restricted emulator input profile, not arbitrary CLI VT support.
It rejects unsupported modes/strings/combining characters, retains zero history,
and always reports remote_control_eligible=false. Unit fake-IO evidence establishes
prefix encoding and writer guards, not real CLI compatibility, renderer fidelity,
multi-client resize authority, reconnect or remote production acceptance.
The trusted single PTY reader supplies its native byte offset through the only
`feed_at` entry. The SAME worker checks activation,seal,exact next offset and checked
end before parser or reply IO. Only a fully Processed batch advances the cursor.
Duplicate/overlap/gap/overflow seals the activation; dropped feedback cannot authorize
resending. Old activation rejection does not consume or alter the current cursor.
This is an ephemeral trusted-reader provenance check, not persistent state, content
authentication, a remote Projection ACK or a complete reconnect protocol. It cannot
identify old bytes deliberately relabeled with a fresh next offset by a malicious
backend. Network/subscriber replay has no access to this capability and must use a
separate projection path; full actor/source/attach integration remains required.

A Windows10 fixed public Rust fixture also traverses this canonical factory and
the actual ConPTY reader/same-writer path. Its duplicate CR-line operation has one
independent effect; root stop, output EOF and reader/writer joins are observed.
The startup DA response traverses the private encoder. Stateful CPR prefix accuracy
is separately tested with synthetic output; the native fixture does not establish
real CLI CPR/full-screen behavior. Precise startup1t/1004 and declined9001 handling
is described in the terminal README; this is legacy VT qualification, not complete
Win32 key-event compatibility. An explicit ignored installed-Codex `--help` test
uses a reviewed executable hash and fresh empty private profile with no account
credentials; it qualifies offline help/startup only, never an AI session.

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
