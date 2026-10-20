# Data persistence

CC Desk currently carries two storage generations: the legacy Claude-compatible workspace and Native CLI v3.

## Principles

### Rebuilding display indexes

Settings → General → Rebuild session index scans already-authorized configured sources into an isolated reader and backend staging generation. It does not delete or edit original JSONL/ZST history, CLI `session_index.jsonl`, archive state, saved names/pins, `projects.json`, configuration or workspace/profile files. Explicit read/enumeration failures cancel publication; ordinary bounded metadata alone does not imply missing sessions. Cancellation waits for the current source read before discarding staging. Existing visible lists remain available.

The initial `native-history-metadata.json` and `session-name-index.json` remain the initial generation. Rebuilds stage only those regenerable files under `session-index-generations/<backend id>/`; an atomic `session-index-generation.json` manifest publishes the pair. Cache-only transaction/publish locks and generation checks reject stale writer completion, including another process. Cancelling or failing before publication preserves the old manifest, disk indexes and published Native memory. Frontend reload occurs after commit. An uncertain receipt triggers a read-only status check of the same document/transaction: confirmed staging may be cancelled, confirmed publication is refreshed, absence requires an explicit same-ID cancellation fence before releasing the identity. Cancelled IDs remain fenced for the document lifetime so a delayed begin cannot install a lease later; the bounded receipt table rejects new IDs when full rather than evicting a live fence. If status cannot be verified, the UI retains the transaction ID and offers “Check rebuild result”, without replaying begin/commit or guessing rollback. A failed post-commit refresh retains the published outcome and retries only the current runtime's refresh. Corrupt generation manifests degrade normal history to uncached reads with writes disabled; they never authorize fallback writes to an old generation. A manifest larger than 1 KiB is conservatively rejected by maintenance and requires separate diagnosis; old and cancelled generation directories are retained.

1. Native CLI resources are **read-only projections** unless a specific CC Desk-owned workspace/profile operation is being performed.
2. CC Desk does not own Provider/API-key configuration.
3. Codex must never inherit legacy Claude secrets.
4. Native writes use revision checks and atomic replacement; ambiguous commits are reconciled by reread, not blind replay.
5. Existing legacy files are retained for compatibility and rollback safety.

## Native CLI workspace

The authoritative Native CLI workspace file is:

```text
~/.cc-box/cli-workspace.v1.json
```

(The base compatibility directory remains `~/.cc-box/` for existing users.)

The native workspace contains CC Desk-owned metadata such as:

- schema version;
- workspace revision;
- Claude/Codex profiles;
- profile revisions;
- registered projects and selected paths;
- forward-compatible unknown schema-v1 extensions.

It must not become a dump of resolved secrets, ambient environment values, raw CLI config, or credentials.

### Override semantics

Profile values use explicit modes:

- `inherit`;
- `set(value)`;
- `unset`.

False, empty and unset values are authoritative states. Later legacy writes must not revive them.

### Concurrency and commit semantics

Native storage uses:

- an independent workspace lock;
- expected-revision/CAS checks;
- synchronized temporary writes;
- atomic replacement;
- fail-closed malformed/future-schema handling.

`COMMIT_STATE_UNKNOWN` is non-retryable. The caller rereads the workspace revision to determine whether the write committed; it does not replay the mutation automatically.

## Legacy compatibility state

Legacy Claude behavior still uses existing `~/.cc-box/` compatibility files such as `config.json` and `projects.json`.

Important separation rules:

- native workspace writes do not mutate legacy files;
- old-package writes to legacy files cannot mutate `cli-workspace.v1.json`;
- legacy Claude resolution is restricted to the explicit compatibility path;
- Codex never reads legacy Claude secret/env values as its profile state;
- legacy user files are preserved rather than deleted during native migration.

## Compatible application preferences

CLI support is a GUI-only preference in localStorage `cc-desk-cli-support-v1`.
New installations enable Claude Code and disable Codex CLI. Missing or invalid
fields fall back independently to those defaults; saved booleans, including both
tools disabled, are retained. Loading does not rewrite this preference. It has no
backend AppConfig counterpart or separate reset command.

Claude launch-configuration `observer` defaults to enabled when missing or
`inherit`, matching the new-configuration editor and safe-template creation.
Explicit `set(false)` and `unset` stay disabled; loading, editing, copying and
refreshing never migrate saved overrides. There is no observer reset command;
choosing `inherit` explicitly restores the default. Codex and Shell do not receive
the Claude observer overlay.

`config.json` retains old/future keys through the existing raw incremental writer.
Optional read DTO fields add GUI mode/density/sidebar width/startup destination/default
CLI, terminal font/line-height/cursor/renderer, and configurable shortcut bindings.
They do not replace `cli-workspace.v1.json` or import Legacy env values into Codex.

`app.ts` serializes simple-setting writes with initial migration. Per-field intent and
confirmed-commit ownership plus underlying read sequence prevent stale hydration or
an older failure from replacing a newer choice. Known failures roll back the current
field to its confirmed baseline. An uncertain acknowledgement only rereads inside
the same lane; failed recovery prevents subsequent writes until a fresh read succeeds.

Legacy environment and default Claude option writes copy each request's values
before entering that lane. Later edits and caller mutations cannot replace an
earlier request's payload; each Promise acknowledges only its own queued write.

Compatibility keys `theme`, `terminalTheme`, `fontSize`, `webglRenderer` remain. Missing
terminal theme may use old GUI state during initial migration only; later GUI changes
never redefine it. Existing palette IDs retain their values and invalid settings use
bounded defaults. Shortcut hydration validates the complete five-action map and falls
back to defaults for malformed/duplicate bindings; explicit conflict replacement is
confirmed in the UI. Startup destination restores only Workspace or Projects and does
not resume a process. Detailed preference behavior is in [terminal preferences](terminal-preferences.md).

## Project/session data

Claude Code's own session/history data remains native CLI-owned data. Legacy UI readers may project it for compatibility.

Native CLI v3 accesses supported native resources through authenticated backend scopes. The frontend does not receive arbitrary filesystem authority simply because it knows a path.

Projection kinds include the supported history/config-resource categories implemented by `cli/native_projection`. Returned DTOs are bounded and kind-specific.

Codex history accepts both `.jsonl` and Zstandard-compressed `.jsonl.zst` rollouts
under `sessions/` and `archived_sessions/`. Compression does not mean archival;
the representative rollout's container still supplies the archive flag. Both
formats use the same session identity, project association, filtering and thread
deduplication. Reads never unpack files onto disk or change native history.

Compressed reads stream through the held source capability, charge compressed
input and decoded output against the existing aggregate budget, cap each side at
the existing per-file limit, and limit the decoder window to 8 MiB. History reads
stop after bounded identity/title observation (normally at most 192 KiB of decoded
prefix; a longer first identity record may use the per-file limit). They cannot
seek to a compressed tail: timestamps come only from observed complete records,
and partial observation remains explicitly incomplete on cold and cached reads.
Full message/search reads retain the decoded per-file limit. Decoding failures
observed in corrupt or truncated entries are reported as individual history read
failures; a partial read does not certify unread frame tails.
The frontend may retain display rows after a whole-source failure or same-root
rows omitted by a later partial failed scan.
Those cached rows retain identity/title/time, but an exact `readFailure` must be
emitted by the current response; an aggregate failure alone cannot identify that
cached session as a current recovery target, even when the error code matches.

Raw values that may contain credentials—environment values, headers, arbitrary configuration blobs—must not be projected.

## Project registry

Native project registration records the stable project identity and selected path needed by the Native CLI workspace.

A project path received from the frontend is not, by itself, read authority. Resource access requires the authenticated document/project/scope chain held by the backend.

## Shared project UI state

The compatibility `projects.json` file now supplies both adapters with CC Desk-owned project presentation state such as:

- pinned projects and per-project archived session keys;
- display names;
- last-opened project;
- other compatibility UI preferences.

Concurrent project-state mutations use the existing lock + read-latest + canonicalize + atomic-write path.

### Unified display names

Claude title readers use the same precedence: the latest valid `custom-title`,
then the latest valid `ai-title`, then the first user prompt. Explicit title records
must match the transcript session ID; old custom-title records without an ID remain
compatible. Legacy list/search/details share one parser. Name-index parser version
2 invalidates old prompt-only cache entries automatically; it does not rewrite CLI
history or require deleting the index. Opened rows follow freshly read CLI titles
within their proven source identity; existing explicit Desk aliases still win.

`projectsState` is the sole frontend writer for `projects.json`, including optional
`sessionRecords` and per-project launch preferences. Both adapters consume saved
names when projecting discovered history or an existing terminal. These records
remain compatible presentation metadata. New GUI rename does not silently save an
alias: supported Claude/Codex history and explicitly labeled opened source/record
targets use the native authenticated metadata API, then clear exact Desk overrides
only after official native readback. Failure or unsupported states preserve old
metadata. Unknown outcomes never replay the write. Claude uses pinned public SDK
0.3.301 with an existing Node >=22.13 and an isolated exact-transcript hardlink;
no default root or model call is used. Busy-writer coordination remains unverified.

Native history metadata uses the full `native-history-v2` catalog identity,
including CLI, launch configuration ID/revision, registered project ID/path and
authenticated source session key. A resumed terminal keeps that identity for its
display name, so refresh, close and a fresh application-store load retain the name.
Legacy metadata binds the normalized project path and Legacy session ID. Records
with a key whose runtime, CLI, project or session fields disagree are not applied.
Raw IDs and ambiguous old Native keys are never guessed into another origin.

Adapters carry an explicitly matched Desk name as the display-only
`savedDisplayName` projection. Folding consumes that resolved name before the
Legacy exact-key fallback; it never scans saved records by UUID/project or
reconstructs another profile, revision, root or registration's history identity.

Display-only folding across profiles requires full CLI/root/registered-project/path/
native-ID/session-key identity. Closed archived projections take precedence over
ordinary closed twins; opened owners remain separate. Explicit archive restore clears
all exact full-key aliases after rechecking current source identity in the metadata
writer queue. Loading or failed retained entries cannot authorize those writes.
Native rename clears aliases and refreshes all exact physical projections;
each opened companion must still own that same record. A Legacy historical alias
clears only after fresh proof that its root is the renamed physical Claude root.

Legacy folding revalidates the runtime's initial physical root key for each
Claude catalog projection, Legacy history admission/restore and Native bare-ID archive restore. A changed, unavailable or
unverifiable root retires that folding lease until a fresh runtime is created;
existing terminal and cached-history proofs are never rebound to a new directory.
The production workspace supplies a root validator; an initially unknown or failed
proof rejects that history operation even before any lease can be retired.
Closed Legacy cache rows stop publishing after lease retirement; positive Native
archive display proof remains marked unavailable and cannot authorize a restore.
Bare Legacy archive IDs apply only to Claude, with complete physical aliases counted
as one source; unknown or distinct physical sources remain ambiguous.
Multiple Native candidates or an ambiguous replacement keep the Legacy archive
restore owner visible. A new Legacy PTY generation clears its previous proof
before publishing the new generation; only its own PTY receipt can set it again.
Windows noninteractive Git Bash can preserve a clean environment proof. Unix
interactive Bash, Windows PowerShell profiles, nonempty `BASH_ENV` and a configured
Windows `USERPROFILE` override leave the actual root unknown without changing
their existing launch behavior or environment.

A new or raw Native terminal without a known authenticated history association
can only save its name under its exact tab identity. It retains the name across a
restart of that tab and when authenticated identity/history association appears
later. Exact history display names take priority. Tab records are not migrated;
independently discovered CLI history after application
restart keeps its own title. No safe association can be inferred from a raw ID,
title, current configuration or default root. Optional metadata alone never
creates a catalog row or restores a process.

Rename saves freeze the source and current attempt. Both the adapter and the
canonical writer revalidate ownership after their queues, immediately before IPC;
an invalidated historical row or replaced Native/Legacy attempt cannot submit the
old name. A cancellation before submission is not an uncertain write and triggers
no reconciliation. Conflicting or uncertain issued writes use the existing
read-only reconciliation and require a fresh explicit action, without replay.

The frontend tolerates malformed optional containers and skips invalid individual
records/preferences while retaining valid siblings. It uses the existing backend
limits (10,000 records, 200-character session titles, bounded identity fields),
with no new storage schema, tombstones or migration write during catalog bootstrap.
The Task 24 migration tests exercise the frontend store boundary with host I/O
fixtures; they do not execute or certify Rust deserialization or real disk writes.

## Native observer data

Observer capabilities and leases are runtime-scoped. They are not durable credentials and must not be persisted into general workspace/config files.

Only bounded allowlisted metadata may reach the owner WebView. Prompt/response bodies, environment values, capability tokens and raw error payloads are excluded.

## Files CC Desk must not create as configuration authorities

Native v3 must not reintroduce:

- `providers.json` as a Provider/API-key authority;
- an independent MCP configuration database;
- mirrored Claude/Codex installation/version state;
- cached resolved secrets copied from CLI or host configuration.

## Migration/rollback

Migration is additive and fail-closed:

- keep legacy compatibility files;
- create/use the native workspace independently;
- never delete user CLI data as part of migration;
- never infer that a successful old-version write should update native state;
- preserve unknown native schema-v1 extensions across native writes.

See [native-cli-v3.md](native-cli-v3.md) and the D25 execution ledger for mixed-version rollback guarantees.


## Unified migration evidence and remaining disk gate

Optional `sessionRecords` and `launchPreferences` are additive. Pin/archive/display-name
operations and their typed metadata mutations use one frontend writer and the backend
`projects.json.lock` read-latest/atomic path. Archive hides a Desk catalog record, not
CLI files. Project removal preserves archive/name/preferences/history while applying
visibility and existing registration changes under admission guards. A failure after
one confirmed step may leave a partial state; no automatic compensating mutation occurs.

Task 25's full frontend run initially exposed two old session-tree fixtures whose
failure readback returned empty disk data despite a previous successful pin/archive.
They now provide the persisted snapshot and assert one readback plus exactly one
mutation. Production reconciliation remains unchanged. Store tests cover malformed
optional entries, valid sibling retention, multi-request ordering and no replay; they
are not disk/Rust/multi-process execution evidence.

The optional Rust DTO and metadata command tests remain NOT RUN in the current cloud
checkout. Final Windows Rust CI and package testing must validate deserialization,
atomic writes and old user data on the exact tested commit. See [U01–U10](superpowers/execution/U01-U10.md)
for the unperformed gate inventory; no migration deletes old files or certifies D20.
