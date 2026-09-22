# Native CLI profile storage — D06

## Scope

This module stores **Desk launch preferences**, not native CLI authentication, model routing or conversation data. It does not start Claude/Codex or expose a new settings UI. Native launch integration consumes these preferences in the later launch tasks.

The storage file is `~/.cc-box/cli-workspace.v1.json`, schemaVersion 1. The `.lock` companion coordinates concurrent instances. A missing workspace reads as revision `"0"` with no profiles; reading may create the directory/lock file, but does not create a workspace document or import legacy credentials.

## Modules

| File | Responsibility |
|---|---|
| `src-tauri/src/cli/profiles.rs` | Tri-state preferences, validation and backend-only legacy resolution |
| `src-tauri/src/cli/storage.rs` | Bounded file access, workspace revision/CAS, atomic writes and safe error codes |
| `src-tauri/src/cli/profile_service.rs` | Caller-scoped list/mutation service and public response projection |
| `src-tauri/src/cli/commands.rs` | Tauri caller injection and blocking I/O dispatch |
| `src/types/profile.ts` / `src/api/cli.ts` | Matching frontend DTOs and thin invocation wrappers |

## Overrides

```json
{"mode":"inherit"}
{"mode":"set","value":false}
{"mode":"unset"}
```

`inherit` uses the permitted legacy/default source; `set` remains authoritative even for false, empty strings or empty arrays; `unset` blocks inheritance. For env, unset removes that key from the later child-process environment, not the application/global environment. Env patches merge individual keys; unrelated entries remain unchanged.

Only `legacyClaude` with CLI `claude` reads old `claudeEnvVars` or `defaultSkipPermissions`. A fresh Claude profile does not implicitly inherit those values, and a Codex profile never opens the legacy file. Literal env values require explicit `nonSecret: true`; secrets should be provided by native CLI configuration or host-env references. This is not a secret-detection or sandbox service.

## IPC

`cli_list_profiles()` returns `{ revision, profiles }`. Only stored profile preferences are returned; unknown workspace sections and resolved legacy/host env values are not projected.

`cli_patch_profile({ expectedRevision, patch })` accepts:

```json
{"op":"update","id":"legacyClaude","changes":{"skipPermissions":{"mode":"unset"}}}
```

Create uses `{op:"create", profile:<complete typed profile>}` with profile revision `"0"`; delete uses `{op:"delete", id:<profile ID>}`. Revisions are canonical unsigned 64-bit decimal strings. `expectedRevision` is the **workspace** revision from the last list response, not a single profile's last-change revision.

Identity fields (`id`, `cli`, `revision`) cannot be changed by update. To use another CLI, create another profile. The command receives the invoking WebView from Tauri; it does not trust an owner/path supplied in request JSON. Only the existing local main window is permitted. Blocking filesystem operations run through `spawn_blocking`.

## Persistence failures

All writes hold the independent lock, reread current state, check revision, apply validated changes and synchronize a unique same-directory temporary file before replacement. Windows uses `ReplaceFileW` for an existing target; Unix uses rename and parent-directory sync. Malformed/unsupported schema, oversized files, unsafe target types and exhausted revisions return safe errors without replacing the document with defaults.

`REVISION_CONFLICT` requires rereading and reconciling the user's change. `COMMIT_STATE_UNKNOWN` means a replacement might have committed and is explicitly non-retryable. A caller must reread; it must not replay the mutation. Deleting a stored profile does not mutate a profile snapshot already cloned for an existing run.

## Evidence boundaries

D06 tests include actual two-process file locking, Windows busy-file replacement, controlled faults before synchronization/replacement and after replacement, unknown-field preservation, corrupt files, overflow, explicit false/unset/empty, backend-only legacy env and no client-side retry. These tests are distinct from full native CLI, user OS, WebView or installer certification. Current results and outstanding target checks are in `docs/superpowers/execution/native-cli-progress.md`.
