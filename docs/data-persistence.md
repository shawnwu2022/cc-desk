# Data persistence

CC Desk currently carries two storage generations: the legacy Claude-compatible workspace and Native CLI v3.

## Principles

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

`config.json` retains old/future keys through the existing raw incremental writer.
Optional read DTO fields add GUI mode/density/sidebar width/startup destination/default
CLI, terminal font/line-height/cursor/renderer, and configurable shortcut bindings.
They do not replace `cli-workspace.v1.json` or import Legacy env values into Codex.

`app.ts` serializes simple-setting writes with initial migration. Per-field intent and
confirmed-commit ownership plus underlying read sequence prevent stale hydration or
an older failure from replacing a newer choice. Known failures roll back the current
field to its confirmed baseline. An uncertain acknowledgement only rereads inside
the same lane; failed recovery prevents subsequent writes until a fresh read succeeds.

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

`projectsState` is the sole frontend writer for `projects.json`, including optional
`sessionRecords` and per-project launch preferences. Both adapters consume saved
names when projecting discovered history or an existing terminal. Saving a name
does not change a native Session ID, write CLI history, send terminal input, or
restart a process. The normal Workspace history row supports the same rename
operation as a live row.

Native history metadata uses the full `native-history-v2` catalog identity,
including CLI, launch configuration ID/revision, registered project ID/path and
authenticated source session key. A resumed terminal keeps that identity for its
display name, so refresh, close and a fresh application-store load retain the name.
Legacy metadata binds the normalized project path and Legacy session ID. Records
with a key whose runtime, CLI, project or session fields disagree are not applied.
Raw IDs and ambiguous old Native keys are never guessed into another origin.

A new or raw Native terminal without a known authenticated history association
can only save its name under its exact tab identity. It retains the name across a
restart of that tab, but independently discovered CLI history after application
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
