# Default-off lifecycle durability candidate

Scope: the existing Create/Attach/Resize/Resume/Close router, a fake trusted Host
queue and task-owned temporary SQLite. No native lifecycle backend, persistent
credential provisioning, migration, listener or remote-control enablement.

## Identity and authorization

The production constructor requires the durable adapter; the memory constructor
is test-only. Existing live channel/scope/fence, local per-kind Rights and Expected
checks remain. Host::resource_identity defaults to None and is denied. Its exact
host/session/run/generation/incarnation/principal must match the proof before any
journal mutation. A native owner must supply this immutable identity from its
actual resource, not copy a request or proof to manufacture a match. Aliasing an
unretired queue under a new generation/incarnation/principal is forbidden; this
native integration obligation is not validated by the fake fixture.

A domain-separated HMAC-SHA256 scope covers the stable resource and principal.
The request HMAC covers scope, device and the canonical entire request, including
action, operation, sequence, generation, fence, mode/dimension/sync epochs and size.
Ephemeral TLS binding IDs are excluded: reconnect cannot reset deduplication.
An independent fixed-domain HMAC witness pins the supplied digest key to a singleton
database row. Mismatched key, or missing witness with existing records, fails closed
before admission. There is no automatic key rotation, lost-key reset or migration.
Production key storage/recovery remains OPEN; tests use public TEST_ONLY constants.
Live authorization is checked again after durable writes immediately before the
trusted nonblocking queue call. Actual mutation-commit revalidation remains the
responsibility of the future native owner; this is not atomic distributed revocation.

## Durable states and effects

| Boundary | Durable decision | Execution/recovery |
| --- | --- | --- |
| Prepare | request plus monotone resource floor in one IMMEDIATE transaction | Ticket only after successful commit |
| Before backend | InFlight committed | No SQLite transaction held while calling Host |
| Queue admitted | Accepted committed | Queue admission only, never lifecycle completion |
| Trusted no-effect rejection | Rejected(code) committed | Exact replay returns rejection; next sequence can be considered |
| Effect uncertain, panic or storage error | Unknown response / unresolved durable row | Never auto-retry or infer no effect |
| Store reopen | Prepared/InFlight/Accepted become Uncertain | Exact replay returns Unknown; distinct work remains blocked |
| Trusted completion | Completed terminal state | Internal fake Attach channel only; see ATTACH-COMPLETION.md; no native backend admitted |

Exact operation/sequence/payload replays never mint a second execution ticket.
Same operation or sequence with altered content conflicts. One unresolved record
per resource blocks later work, including after elapsed time or store recreation.
Ticket ownership pins the native directory lock even after Store drop. Transactions
are short; readback witnesses verify exact identity/digest/state before commit.
Commit failure or injected commit-cut errors poison that Store; callers reconcile by reopening instead
of continuing an uncertain connection. No record deletion, compaction, floor reset
or timeout-based unblocking is implemented.

An internal fake Attach completion channel is described in
[ATTACH-COMPLETION.md](ATTACH-COMPLETION.md). The Accepted gate remains incomplete for native use: there is currently
no production completion/reconciliation path, so successful queue admission may
keep the resource blocked indefinitely. A durable authenticated completion contract
and recovery policy are required before wiring any real lifecycle backend. The
wire reply still has no Completed status; historical queue admission must never
be displayed as process/CLI completion.

## Local storage contract

Reuse is limited to the existing unchanged Windows path/reparse/identity/pinning
guards, directory owner lock, SQLite qualification and digest primitive. Existing
PTY HostWritten/receipt semantics are not repurposed as lifecycle completion.
The guard uses one owner lock per directory: lifecycle metadata needs a separate
trusted private metadata subdirectory from a simultaneously open input journal.
Same-directory access fails OwnerBusy; the guard was not weakened.

Application-family ID is 0x43434452; new lifecycle schema version0x4c4a0002 adds
sealed typed Attach results. Legacy0x4c4a0001 is exact-fingerprint/read-only and
unresolved, never auto-migrated. Both differ from input-journal version2. Foreign
or unknown schema rejects before writable PRAGMAs. No production database is
opened or migrated by the tests. WAL + synchronous FULL, page4096,
max4096 pages, 4096 operations, 1024 resource floors, and 16MiB DB/WAL checks bound
this candidate. Capacity exhaustion fails closed; retention/retirement is OPEN.
Private trusted local storage is required; restoring old DB backups, cloning or
hostile same-user storage edits are not solved by these records or the key witness.
No terminal text, argv, credentials or digest key are stored in this metadata.

## Verification and limits

Tests fault-inject before/after prepare/transition commits, drop/reopen connections
and reconstruct routers. They cover all states, post-effect fake panic, exact replay,
conflicting concurrent payloads, duplicate concurrent IDs, key mismatch, identity
mismatch/default deny, expiry/revocation during flush, no backend-held DB transaction,
owner pinning, update witness corruption, foreign-schema byte preservation and
separate-directory ownership. This is simulated interruption with real local SQLite,
not abrupt process termination, power-cut, native lifecycle or storage-hardware proof.

Run the independent slice only (task-owned TEMP/TMP supplied by the harness):

```powershell
cargo +1.98.1 test --manifest-path remote/Cargo.toml -p cc-desk-remote-host --all-features --locked --offline --lib lifecycle_
cargo +1.98.1 test --manifest-path protocol/remote/inner/v1/Cargo.toml --locked --offline
cargo +1.98.1 clippy --manifest-path remote/Cargo.toml -p cc-desk-remote-host --all-targets --all-features --locked --offline -- -D warnings
```

The historical real PTY concurrent failure, handle growth, default-root OS5,
WebView2, process/IO retirement, actual reply delivery, real CLI, signed phones,
multi-Host capacity and operating recovery gates remain OPEN and were not rerun.
These checks cannot authorize deployment or support a production-delivery claim.
