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

## Project/session data

Claude Code's own session/history data remains native CLI-owned data. Legacy UI readers may project it for compatibility.

Native CLI v3 accesses supported native resources through authenticated backend scopes. The frontend does not receive arbitrary filesystem authority simply because it knows a path.

Projection kinds include the supported history/config-resource categories implemented by `cli/native_projection`. Returned DTOs are bounded and kind-specific.

Raw values that may contain credentials—environment values, headers, arbitrary configuration blobs—must not be projected.

## Project registry

Native project registration records the stable project identity and selected path needed by the Native CLI workspace.

A project path received from the frontend is not, by itself, read authority. Resource access requires the authenticated document/project/scope chain held by the backend.

## Legacy project UI state

The legacy workspace still persists CC Desk-owned project presentation state such as:

- pinned/archived project state;
- display names;
- last-opened project;
- other compatibility UI preferences.

Concurrent project-state mutations use the existing lock + read-latest + canonicalize + atomic-write path.

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
