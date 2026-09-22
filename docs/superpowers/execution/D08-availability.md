# D08 availability continuation — approved v3 W2/D08

Base: `e7c11e1ca384f4ee9cda0b3022053ad6fce87e5d`, existing PR #17; CI #154 was verified green. Continue this branch, never recreate D01-D07 or the snapshot work.

## Work and rulings

- Add per-profile read-only preflight, strict IPC request parsing, a minimal host PTY allocation probe, and a typed frontend wrapper. Availability is not CLI execution or native certification.
- The supplied v3 W2 source was read from the existing planning ZIP after two Files searches returned no indexed match. Plan/spec paths remain supplied artifacts, not assumed remote files.
- Existing isolation is the PR #17 feature branch. Container clone again failed DNS and Rust is unavailable. Use GitHub tree/commit/ref writes and Actions validation; do not claim a local full checkout or user-machine tests.
- Ruling: keep the profile API in `src/api/cli.ts` and its small implementation module, matching D06/D07 modular APIs instead of growing the legacy PTY wrapper. `cli_get_availability` takes one strict `request` object containing profileId/expectedRevision; no cwd, program path, host health or environment comes from the WebView.
- Ruling: explicit host preflight allocates and drops one native PTY without launching a selected CLI. Damaged pinned Windows runtime still fails before Tauri as main.rs requires; no fallback is added. Host allocation and CLI file/configuration status remain separate.
- Ruling: discovery remains backend-only until the later trusted selection UI can supply authoritative exclusion roots. Do not expose arbitrary path discovery through this endpoint.
- Ruling: existing global startup/PATH refresh belongs to the old route and is not removed here; migration remains D10/W7. The new endpoint must not invoke that route or mutate process environment.

## Checkpoint

Availability service and API placeholders intentionally fail behavioral tests. Observe actual RED before implementing. Existing passing tests stay enabled. Status: IN_PROGRESS; no independent review or full D08 completion claim.
