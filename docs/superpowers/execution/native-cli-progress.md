# Execution ledger — native CLI workspace v3

## Continuation point

D06 implementation and the existing CI gates passed for code commit `bd7646b0cee65ea415c0168c453bdfe3775849d0`, in draft PR #15 / `feat/native-cli-profile-storage`, stacked on D05 #14 at `c2a19af5ecc2a219fe570b58bf70856997cc6a4c`. The next planned task is D07, independent project registration. Do not reconstruct or reapply D01–D06 from downloadable patches.

D06 is implemented and CI-verified, not merged. User-machine baseline, live WebView IPC, native CLI interactions, non-Windows storage behavior and final installer certification are not certified here. No main merge, release, native-config write or user-machine mutation was performed.

## Observed evidence

| Run | Observation |
|---|---|
| [CI #136](https://github.com/shawnwu2022/cc-desk/actions/runs/35712286816) | Formatting failed before the new tests ran; not behavioral RED. |
| [CI #137](https://github.com/shawnwu2022/cc-desk/actions/runs/35712448627) | Missing profiles/storage modules; preliminary compile RED only. |
| [CI #138](https://github.com/shawnwu2022/cc-desk/actions/runs/35712869423) | At `9b53d3e1c5c1df5cb60a39bc7a79691782a6715e`, 350 Rust library tests discovered: 335 passed, 2 failed, 13 ignored. The actual failures were explicit false resolving to legacy true in `D06_Override_FalseUnsetEmpty_01` and `D06_Legacy_IsClaudeOnly_03`. |
| [CI #139](https://github.com/shawnwu2022/cc-desk/actions/runs/35713471501) | Corrected override behavior and expanded storage tests passed; remaining formatting and API work prevented a green workflow. |
| [CI #140](https://github.com/shawnwu2022/cc-desk/actions/runs/35713683910) | Added API tests before their modules: frontend missing `api/cli`, backend missing profile service. Preliminary module RED, not a simulated native failure. |
| [CI #141](https://github.com/shawnwu2022/cc-desk/actions/runs/35714563895) | Code head `bd7646b0cee65ea415c0168c453bdfe3775849d0`, tested PR merge `d78bf8a257fc15cb0e3e7443082ac4a4d231426e`: both jobs completed/success. Frontend typecheck, 611 Vitest tests, 3 Node policy tests, Vite build; Rust library 347 passed/0 failed/14 ignored, main 6 passed, paste launch-config 2 passed/3 ignored, transport 8 passed; fmt and Clippy `-D warnings` passed. |

Two independent subprocess workers were explicitly invoked by the D06 concurrency parent test, each ran one test and passed. The ignored worker marker in the ordinary library enumeration is not an unexecuted concurrency test. Other ignored native/real-history tests are not converted to PASS. Doc-tests contained zero tests and provide no additional behavioral evidence.

The current documentation-only follow-up records these observations; any later commit must use its own CI result before being described as green.

## Implemented interfaces and boundaries

- Profile `Override<T>`: `inherit`, `set(value)`, `unset`; explicit false/empty values stay authoritative.
- `WorkspaceRepository`: bounded workspace file, independent lock, revision check, field-level profile patches, synchronized same-directory temporary write and atomic replacement. Unknown top-level data is preserved; malformed/future-schema data is not replaced with defaults.
- Legacy resolution is backend-only and applies only to reserved profile `legacyClaude` with CLI `claude`. Codex does not open the legacy file. Resolved legacy/host values are not saved into the new file or returned through profile APIs.
- `cli_list_profiles` and `cli_patch_profile`: actual calling window injected by Tauri; main label checked before storage I/O; no frontend-selected storage path, no raw serde errors containing submitted values, no automatic mutation retry.
- `COMMIT_STATE_UNKNOWN` is non-retryable: reread the revision after ambiguous replacement, never blindly repeat the patch.

See [profile storage contract](../../native-cli-profiles.md).

## Rulings

- Remote feature branch plus actual GitHub Actions is the execution environment. It is not the user's local checkout, and its Windows Server 2022 results are not Win10/macOS/Linux native certification.
- D05 had no `Override` type. D06 defines it in `cli/profiles.rs`; TypeScript profile DTOs live in `src/types/profile.ts` and use the same mode/value shape.
- D06 patch `expectedRevision` is the workspace revision returned by `cli_list_profiles`. Each profile additionally carries its own last-change revision for later launch snapshots. These are different concurrency scopes and must not be interchanged by D08/D11.
- New profile commands are grouped in `cli/commands.rs`, registered in `lib.rs`; frontend wrappers live in `api/cli.ts`. This avoids replacing unrelated existing command facades while preserving the approved endpoint names.
- The CI gates were reordered to expose test failures before formatting. Formatting and Clippy still execute after test failure when runtime preparation succeeds; neither is waived or marked continue-on-error.
- This is self-review and CI, not independent review. No user-facing profile editor or launch behavior is claimed at this stage; those remain assigned to the later tasks.

## Remaining validation / observed baseline warnings

- Unix-specific persistence, real WebView caller authorization, actual old-package rollback and real CLI/installer tests still need their planned target-environment evidence.
- `npm ci` in CI #141 reported 11 dependency advisories (4 moderate, 6 high, 1 critical). These are dependency audit notices, not 11 reproduced application exploits. No package/lockfile changed in D06; do not silently run `npm audit fix --force` or call this a clean security audit. Dependency triage remains an explicit follow-up in the planned security/dependency work.
- PTY, clipboard, input handling, native launcher, app version and release workflow remain unchanged from the D05 base in this PR. The release guard is still confined to the unmerged PR stack.
