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

### Verified Task 5 repair checkpoint

- Remote commit: `97fa67e5791a8238d8ea28e17e1407015456a65b`
- Published via authenticated GitHub Git-data connector, fast-forward only; no shell credential configured
- Verified local staged tree equals remote commit tree, fetched HEAD equals `ls-remote` branch SHA, and working tree was clean before this readback entry
- Next: Task 6

## Task 6 — Shared UI primitives and notifications — September 30, 2026

- Status: implemented and locally verified; controller review/publication pending. Base remains `97fa67e5791a8238d8ea28e17e1407015456a65b`; no new commit, push, PR, CI, package, version or release operation was performed by the implementer.
- Added all 13 planned common Vue primitives: buttons, input/select, tooltip, menu, dialog/drawer, toast host, inline notice, empty/loading state and safe error details. Existing product surfaces are not migrated during this foundational task.
- Menu contracts: hidden actions omitted, danger actions last, disabled actions skipped, Arrow/Home/End/Enter/Space/Escape/Tab behavior, outside close and focus return. Danger-only menus focus their container until explicit keyboard navigation.
- Modal contracts: labelled teleported dialog, safe initial focus, no danger autofocus, focus trap/return, hidden controls skipped and top-overlay ownership for nested modals. Drawer reuses the dialog contract.
- Notifications are runtime-validated locale-key DTOs with bounded opaque dedupe identity; raw Error objects, unknown message keys, unknown kinds and extra transport fields never enter state. Latest-three capacity, five-second expiry, close/clear/dispose cleanup and pointer/focus pause-resume are covered.
- Central shared styles use existing GUI tokens, 28/32/36px control geometry, 2px ink-blue focus, amber selection, flat static surfaces and existing menu/dialog shadow tokens. No transition-all or new runtime dependency. `@vue/test-utils` 2.5.1 is a development-only dependency.
- RED: first required command failed on both missing new module imports. Importable skeletal components/store then produced 24 behavioral failures of 26 tests. Additional focused RED reproduced danger-only menu autofocus, hidden modal focus and prototype-key diagnostic fallback before their fixes. Instance IDs use module counters compatible with the declared Vue 3.4 API baseline; no Vue minimum/version change.
- Final local regression: `npm test` → 98 files / 964 tests pass, exit 0. Existing app/sidebar/disposal failure-path tests intentionally emit stderr; no failing test. This is not actual CLI certification.
- GREEN: `npm test -- tests/components/uiPrimitives.test.ts tests/stores/notifications.test.ts tests/designTokens.test.ts && npm run typecheck` → 40 tests pass (23 component, 8 notification, 9 token), typecheck exit 0. `git diff --check` passes.
- The collapsed-details test was corrected to check native `open` state; DOM textContent includes collapsed content and is not a visibility assertion. One nullable test attribute access was corrected for strict TypeScript.
- Windows 100%/125%/150% scaling, actual rendered 1024×640 layout, screen-reader behavior and final screenshots remain final visual/platform gates; Linux jsdom behavior is not that certification.

### Task 6 rulings

- Toast message keys are checked against the existing locale catalogue; downstream Task 16 must add its approved feedback keys to locales before emitting them. Unknown keys are rejected instead of falling back to raw text. Cost if wrong: callers get no toast until their catalogue key exists.
- Short toasts expire after five seconds and pause while hovered/focused; dedupe returns the existing ID without extending it. Cost if wrong: downstream callers must choose stable action identities deliberately and use persistent inline notices for unresolved work.
- Menu positioning remains the consumer's anchor responsibility; dialog/drawer modality and focus ownership are shared. Cost if wrong: final layout consumers must account for viewport edges when positioning their shared menus.

### Task 6 independent review repair — native summary focus

- Reproduced omitted native `<summary>` controls preventing keyboard access to ErrorDetails inside AppDialog. The modal focusable selector now includes the first direct summary in each details element.
- RED: new `Dialog_SummaryTabCycle_024` failed because Tab from Close was intercepted. GREEN: required UI/notifications/tokens command now passes 41 tests (24/8/9), typecheck exit 0; log retained in the controller's Task 6 report.
- The regression checks forward reachability and both focus-trap wrap directions. Prior staged task snapshot remains intact; controller review/publication still pending. Earlier full-suite 964-pass evidence predates this narrow repair.

### Task 6 independent review repair — tooltip mixed input

- Reproduced focus lost on pointer leave, hover lost on blur, and Escape dismissal reopened by an already-active focus event. Tooltip now derives visibility from independent focus/hover state with a dismissal latch reset only by a fresh inactive-to-active interaction.
- RED: three new mixed-input regressions failed. GREEN: required UI/notifications/tokens command now passes 44 tests (27/8/9), typecheck exit 0; both review repairs are included.
- Existing descriptions, actual native focus/blur and deliberate keyboard/pointer reentry after Escape are covered. Previous staged work is preserved; review/publication remains pending, with no new full-suite claim for this narrow repaired snapshot.

### Task 6 review gate

- Independent spec/quality review approved the final snapshot with both interaction repairs included; no remaining critical/important Task 6 finding
- Final exact local gate: 44 targeted tests pass and typecheck passes. Earlier full frontend regression: 964 tests, before the two narrow interaction repairs; full final system verification remains Task 25
- Publication uses one atomic task commit; remote SHA recorded after readback
