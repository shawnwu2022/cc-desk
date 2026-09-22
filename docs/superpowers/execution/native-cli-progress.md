# Execution ledger — native CLI workspace v3

## Current branch and scope

D06 is isolated in `feat/native-cli-profile-storage`, PR #15, stacked on D05 #14 at `c2a19af5ecc2a219fe570b58bf70856997cc6a4c`. No main merge, release, native-config write, or user-machine certification has occurred.

## Evidence

- CI #136 / 35712286816: formatting failed before the new tests ran; discarded as behavioral RED.
- CI #137 / 35712448627: missing profiles/storage modules; preliminary compile RED only.
- CI #138 / 35712869423 / head `9b53d3e1c5c1df5cb60a39bc7a79691782a6715e`: 350 Rust tests discovered, 335 passed, 2 failed, 13 ignored. Actual failing assertions: explicit false resolved to legacy true in `D06_Override_FalseUnsetEmpty_01` and `D06_Legacy_IsClaudeOnly_03`. All six initial storage tests passed. Frontend checks passed.
- The current change implements the three-state resolution and adds fault-boundary, legacy isolation, true subprocess and frontend API tests. Results have not yet been obtained for this change.

## Rulings

- Remote isolated branch plus GitHub Actions is the execution environment: the local container has no Rust and cannot resolve GitHub. Do not present Actions as user-local or native-CLI certification.
- D05 did not introduce Override; D06 defines it in `cli/profiles.rs` rather than duplicating a phantom existing type. Its wire shape is mode/value and is tested.
- D06 API expectedRevision means the workspace revision returned by cli_list_profiles. Each profile additionally carries its own last-change revision for later launch snapshots. Never interchange the two.
- New profile IPC will be grouped in `cli/commands.rs`, registered in lib.rs; frontend wrappers in `api/cli.ts`. This avoids unrelated replacement of the large existing command facade while keeping the approved endpoint names.
- Failures after replacement may mean the new state committed; return COMMIT_STATE_UNKNOWN with retryable=false. Callers re-read instead of blindly reapplying mutations.

## Remaining D06 gates

Complete API wiring, format and lint checks, expanded runtime tests, and self-review. Real CLI interaction and cross-platform installer certification remain NOT_RUN. This ledger is not a claim that D06 or the overall plan is finished.
