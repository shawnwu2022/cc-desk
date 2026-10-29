# Host journal: first E07 component, not a running remote Host

## Default-off E09 writer resource owner

`experimental-host-owner` adds a private `WriterHostOwner` over the actual
canonical LocalWriter factory. One process-lifetime admission covers all its
factories and their shared two-view budget. IO custody, capture-pool handles and retained view credits
hold that activation; dropping/sealing the owner cannot create a new owner while
an old write or view is still live (`Busy`). Once those references are gone,
reconstruction remains permanently `Closed` in that process, including a changed
Host/incarnation argument. Dropping the owner never resets tombstones/admission
capacity for another owner. A new Host process and its separately proven locked
startup/enrollment contract are required; no automatic restart is performed.
A thread join is checked nonblocking; panic
or a still-running write is never reported as a completed retirement.

Admission checks exact Host/incarnation and unique session ID, bounds active
writers to12 and lifetime session IDs to4096 without eviction. Pending input
and output remain on the existing serialized IO/journal path. Retired session
IDs cannot be reused within that process even at a higher generation until a
separate true runtime retirement protocol exists. Exhaustion refuses admission; it does not rotate
namespaces, restart tasks or erase journal uncertainty. These are bounded
resource prerequisites, not a measured concurrency/capacity guarantee.

This owner does not own the Job/reader, persist startup identity, enroll a peer,
validate a grant or issue an input lease. The guarded native ACK prerequisite
below supplies synchronization evidence only. The owner does not
make an IO join into whole process-tree/old-reader retirement evidence. Those
E08/E09 integration and policy gates remain OPEN. Backend raw identifiers and
digest fixture bytes are not endpoint credentials or authorization. No public
remote/session factory, listener or ordinary desktop migration is enabled.
The singleton constrains owner-managed canonical writers; pre-existing static
and non-owner local qualification paths are not globally enrolled by this flag.

Tests exercise held frames, an actually blocked Write, finite slot refusal,
cross-target admission and a contained real self-owned pipe: duplicate operation
receipts cause one fixture effect, then sealed handles reject further input.
The fixed pipe fixture is not an AI CLI or PTY/phone qualification.

## Journal and existing local writer boundary

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
startup geometry (1..100 rows,2..240 columns) before worker creation. The backend
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

### Serialized read-only projection capture

CanonicalStart additionally requires an explicit shared HostViewBudget. The
private resource owner now creates one pool for its session factories; it is
resource accounting, not Host identity/authorization. Each shared pool admits two
snapshot leases across writers. The fixed-cell view is statically bounded below
4MiB logical allocation/frame, hence8MiB/pool; this is not a process RSS claim.
Requests,pending feedback and consumed-but-retained frames hold the SAME credit
until their last owner drops. Queue refusal/closed delivery returns its lease.

The canonical factory alone mints a private capture handle. Command::Projection
uses the same bounded queue and worker as output and input. It checks activation
and seal at its atomic commit point, then copies the SAME Core at that barrier with
the worker's real applied-through native offset,target,writer_epoch and local batch
revision. Only a completely Processed output batch increments revision; capture
itself does not. Overflow freezes before parsing/IO. No second parser or user
operation/receipt/fsync pair is introduced. A semantic-view failure seals the worker.

A wait timeout is Uncertain, not cancellation. A capture committed before seal can
return an earlier historical read-only view after seal; one still queued at seal
is refused. Neither result grants input or permission to send data to a remote peer.
Actual Win10 fixed-PTY tests capture a valid output prefix before stopping and then
observe root exit/EOF/join. The prefix is not represented as the entire EOF tail.

The view exposes exact Core mode/dimension/structure epochs. Ordinary capture
still has no Host sync/barrier/ACK authority; writer_epoch is its separate local
writer scope. Shared128-cell/32KiB encoding and the trial TS consumer now exist.
Authenticated observer transport, full E05 and the E09 actor remain required.

### Guarded native snapshot/ACK prerequisite

`WriterHostOwner::start_guarded` is opt-in/default-off over the same queue, IO and
journal. It accepts only a trusted backend's local target/device/channel context;
there is no network admission factory. The ordinary raw submit handle refuses
guarded input, and the worker independently refuses missing or stale view proof.
The existing non-guarded local/desktop prerequisites are not migrated or remotely
selectable. No CLI version gate is introduced: protocol minors and fixed manual
CLI fixture hashes are separate from native CLI launch/version compatibility.

A queued guarded capture issues an exact new barrier/sync counter for its native
peer activation. Consuming the captured frame through the existing async
RecordWriter is the only way to mark it delivered: all parts and flushes must
finish under one absolute transfer deadline, capped at30s. Once native sending
begins, timeout, failure or future cancellation closes the peer scope and drops the supplied writer adapter;
already encoded/partially written data is uncertain, never a successful install.
Native close wakes blocked sends and invalidates queued input independently of
the potentially blocked stdin writer. Shared/split IO still requires its enclosing
authenticated channel owner to cancel both physical directions.
Never-polled sends or pre-IO budget/minor refusal remain unconfirmed and cannot
be ACKed. The enclosing channel owner must serialize sends; before replacing a
barrier during a send, cancel and await that send. Replacement alone revokes the
old record but does not wake blocked IO immediately; its eventual failure closes
that peer scope. Concurrent lossless view replacement is not implemented.

The ACK is a bounded shared control codec with mandatory ProjectionAck kind and
the full exact stamp. It enters the SAME writer queue and must match its issued,
fully delivered record, native peer activation and current Core epochs. Then an
opaque local installed-view proof can accompany input. This proof is checked on
dequeue and again after durable intent immediately before the actual write.
Revocation before that commit produces durable no-effect Rejected when an intent
already exists; an already committed IO may finish. Early native policy failures
are not falsely presented as durable operation receipts. Full remote admission/
rejection/sequence normalization is still a later authorization-actor gate.

Ordinary contiguous cell output does not demand an always-latest grid ACK. Actual
mode changes, structural replacement, a new barrier or disconnect invalidate old
proofs. Current recovery uses a fresh full snapshot; a correctly coalesced Host
delta/mode-only publisher, leases/fences/InputWindows and rebind remain OPEN.
The keyed input digest includes the exact installed context, so a new view cannot
reuse an old operation with substituted context and call it the same request.

Native peer IDs and a raw RecordWriter do NOT authenticate an endpoint or prove
that transport belongs to that peer. Before network dispatch exists, a verified
E2E data-channel factory must bind IO, identity, authorization and this native
scope; ManagementOnly cannot be substituted. These tests use public synthetic
data and memory pipes plus self-owned fixture processes, not real TLS/phone/AI
CLI acceptance. Nothing here enables remote data or permission merely from ACK.

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
