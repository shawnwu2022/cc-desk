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
- Core module instructions are recorded in `src-tauri/src/cli/AGENTS.md`; the broad legacy root description is not rewritten as part of this bounded task.

## Observed RED

Commit `5b005e3136c2aab535cc9a627b8dc45454de1dc4`, CI #155 (`35753111261`), PR test merge `4c75cf981f1e174db6209e34c601236ff101a91c`:

- Frontend job `106832026853`: typecheck passed; all six new API cases failed against AVAILABILITY_NOT_IMPLEMENTED, while 622 existing cases passed. Full log read before frontend implementation. Node policy/build were skipped after the expected test failure, not claimed passing.
- Rust job `106832026465`: compilation passed; all eleven new availability cases failed against the scaffold; 387 passed / 11 failed / 14 ignored. The extra six passes relative to the old 381 baseline are the bundled runtime's existing tests included by this test module, not six additional availability scenarios. Full log read before backend implementation.
- Strict Clippy passed. Rustfmt requested layout-only changes to the new test file; these were applied separately without weakening behavior assertions.

## Implementation checkpoint

The new service authenticates the real window label before storage/probing, validates profile identity and its own revision, reuses the snapshot selected-path resolvers and the environment builder, and returns only a small status DTO with sanitized errors. The legacy config is read only for legacyClaude. A missing program/runner or host reference reports a profile failure; host allocation is separate. Storage/auth/revision errors reject the request, without automatic retry.

The Tauri command is registered in lib.rs and exported by the modular frontend API. JS validates input before invoke, checks receipt identity/revision and known states, rejects invented certification, and reconstructs an allowlisted response rather than copying env/argv extras. The command does not launch any selected agent or invoke the legacy startup check.

Status: implementation submitted for CI; not yet a GREEN or independent-review claim. Old startup integration, actual spawning, trusted discovery UI, real CLI/WebView/Unix/installer certification remain outside this checkpoint.
