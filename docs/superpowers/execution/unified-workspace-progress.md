# Unified Workspace UX execution ledger

## Durable execution policy

This branch is an append-only remote checkpoint for the unified workspace implementation.

For every implementation task:

1. Write and run the task's failing tests.
2. Implement the smallest passing change.
3. Run the task's targeted verification locally.
4. Commit the complete task as an atomic commit.
5. Push the commit to `feat/unified-workspace-ux` immediately.
6. Verify that the remote branch HEAD equals the local commit SHA.
7. Record the task, test command, result, and remote SHA in this ledger.
8. Only then begin the next task.

Rules:

- Do not force-push or rewrite remotely checkpointed task commits.
- Do not open a pull request until the implementation and visual design are frozen.
- Feature-branch checkpoint pushes must not be used to trigger CI.
- If remote synchronization fails, create a Git bundle and source snapshot, retain the workspace, mark the task `BLOCKED_SYNC`, and do not count it as remotely complete.
- Final CI and the Windows installer workflow run only after Task 25 freeze.

## Current verified remote state

- Branch: `feat/unified-workspace-ux`
- Verified upstream before this repair: `bd65fe1a0dce9e2cf90d73397ac762c9073d1bc8`
- Design specification and implementation plan: complete
- Task 1: present at `04370fdc22e41fa07ed61546d9b6e5d15ac69e35`
- Tasks 2–4: source/tests restored remotely at `545c8cfce746076d891dfd1f97303011fac92bbd`; previous pending-sync entries were stale
- Task 5: initial implementation at `bd65fe1a0dce9e2cf90d73397ac762c9073d1bc8`; correctness repair described below
- Next task after repair checkpoint verification: Task 6 — shared UI primitives and notifications
- Cloud checkout: September 30, 2026. No desktop work or real-CLI certification is implied

## Task checkpoints

| Task | Status | Verification | Remote commit |
|---|---|---|---|
| 1 | source complete | current cloud helper regressions pass | `04370fdc22e41fa07ed61546d9b6e5d15ac69e35` |
| 2 | frontend verified; Rust pending final toolchain gate | current cloud projects-state tests pass | `545c8cfce746076d891dfd1f97303011fac92bbd` |
| 3 | frontend verified | current cloud Legacy adapter tests pass | `545c8cfce746076d891dfd1f97303011fac92bbd` |
| 4 | frontend verified, identity repair in Task 5 checkpoint | current cloud Native adapter/history/tabs/attempt tests pass | `545c8cfce746076d891dfd1f97303011fac92bbd` |
| 5 | repaired and locally verified; checkpoint SHA to be recorded after publication | 68 targeted tests across Tasks 1–5; typecheck passes | initial `bd65fe1`; repair pending readback |

## Task 5 review repair — September 30, 2026

- Reproduced scoped refresh deleting other projects/selection; project-scoped replacement now preserves other scopes. Distinct scopes and full/scoped request interleavings have independent ownership
- Reproduced wrong Claude routing. New Claude/Codex use Native; history resume resolves catalog origin, with explicit runtime available for ambiguity
- Reproduced unusable archive/restore. Archived history remains addressable in the catalog but is excluded from normal project groups
- Reproduced Native raw-ID archive collisions across CLI/source roots. Native archive keys are complete catalog identities; Legacy raw IDs remain compatible
- Reproduced profile-A/revision-7 tab reuse for profile-B/revision-8 history and source-root over-deduplication. Native reuse respects launch configuration/source identity; active claims only suppress exact history
- RED evidence: 6 original failures, concurrent-scope failure, Native archive collision, wrong-profile resume, and source-claim failure. All targeted final tests pass
- Verification: `npm test -- tests/utils/relativeTime.test.ts tests/utils/sessionPresentation.test.ts tests/utils/userError.test.ts tests/stores/projectsState.test.ts tests/stores/legacyClaudeAdapter.test.ts tests/stores/nativeCliAdapter.test.ts tests/stores/nativeHistory.test.ts tests/stores/nativeTabs.test.ts tests/native-cli/nativeAttemptIsolation.test.ts tests/stores/unifiedSessions.test.ts tests/stores/attention.test.ts && npm run typecheck` → 68 tests pass, typecheck pass
- Independent focused review: approved this repair snapshot after inspecting archive namespace, resume context and full/scoped ownership; reviewer did not run tests. Task 11 must preserve resumed source metadata through real runtime ports.
- Rust was not run: Cargo is unavailable in this cloud checkout. Existing Task 2 Rust gate remains pending; no CI/package run requested

## Rulings

- Task 2: added typed IPC mutations (`upsert_session_ui_record`, `remove_session_ui_record`, `set_project_launch_preference`) and their frontend wrappers. The shared writer cannot safely persist the new fields through the existing legacy pin/archive/display-name mutations alone. Cost if wrong: a broader Tauri command surface requiring final Rust review.
- Task 2: local environment has no Rust toolchain and outbound DNS is unavailable, so filtered Rust tests cannot run in this workspace. Frontend gates are green; the checkpoint remains explicitly Rust-pending until the final Windows/Rust gate runs. Cost if wrong: a Rust compile/test defect may be discovered at final CI rather than this checkpoint.

- Task 3: Legacy runtime operations are injected through `LegacyClaudeRuntimePort` rather than implemented inside the Pinia store. Process ownership remains in the terminal host, while the adapter owns projection and routing. Cost if wrong: Task 11 must supply a complete runtime bridge before the adapter can be used in production.
- Task 3: Windows-style project paths are normalized case-insensitively even when tests execute on Linux, because persisted Windows identities must remain stable across build/test hosts. Cost if wrong: a case-sensitive Windows-like path on Linux would be merged.

- Task 5 Ruling: `sessions` retains archived records; `projectGroups` is the normal visible projection — restore requires a stable addressable ID — cost if wrong: future archive consumers must filter the catalog deliberately.
- Task 5 Ruling: Native archive uses the existing catalog ID in the shared archive array, while Legacy preserves raw IDs — runtime/CLI/source collisions must not cross-hide sessions — cost if wrong: unreleased historical Native raw-ID archive marks are not guessed/migrated.
- Task 5 Ruling: active Native archive requires one exact cached history origin before stopping — absent/ambiguous roots cannot be guessed — cost if wrong: the user must refresh/select an explicit historical item before archiving.
