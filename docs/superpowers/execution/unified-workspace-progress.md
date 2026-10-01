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
- Task 6: complete at `31223e131396d8c899660ad8d47fabbb5d574dcc`
- Task 7: complete at `d10fc5a637589de95875e61e1b1d48c332595c32`
- Task 8: complete at `aeb644965aa214fbca3885169c1132ed2d5d5ff5`
- Task 9: complete at `f2764aa4728983fbdf8989f9c4bb9881a9069d8b`
- Task 10: complete at `8683f6b9e56c2375a2520508f104754c8fd4553c`
- Task 11: complete at `b1309bd294f153e99449b0d3413542ba923a294e`
- Task 12: complete at `72644819200a298f7b6689dfa512289036d37177`
- Task 13: complete at `181c6f51c509468e3f6c3f2a844131dcd3975e65`
- Task 14: complete at `d1a6dc6d6fbb22057245e628c0b6eedcf83fe36a`
- Task 15: complete at `6ae851a3a3ee9de789a5085b5a0e1d03b551eb44`
- Task 16: complete at `c0a1c231426b599c30ec52ea2740f44d10334e96`
- Task 17: complete at `bb8c2c999370e4988b96d2917bd10f66154bda51`
- Next task: Task 18 — unified terminal preferences and preview
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

### Verified Task 6 checkpoint

- Remote commit: `31223e131396d8c899660ad8d47fabbb5d574dcc`
- Atomic connector publication with no force; fetched commit tree exactly matched staged tree; local HEAD and remote ls-remote SHA matched; working tree clean before this readback entry
- 44 targeted tests + typecheck; independent review clean; no CI/PR/package/release
- Next: Task 7

## Task 7 — Session status and CLI application icons — September 30, 2026

- Status: implemented and locally verified; controller review/publication pending. Base remains `31223e131396d8c899660ad8d47fabbb5d574dcc`. Existing Task 6 readback is preserved; no commit, push, PR, CI, package, version or release operation was performed by the implementer.
- Added `SessionStatusIcon` and `CliAppIcon`, eight self-owned MIT SVG assets with ownership notes, and English/Chinese status accessibility keys. Existing rows are not migrated during this primitive task.
- Status shapes are separately identified gapped ring, circle/play, conversation bubble/dot, diamond/question, stop square and triangle/exclamation. No inline status text. One actual shared `AppTooltip` trigger supplies a localized accessible name, keyboard focus, pointer tooltip and 2px token focus ring.
- Starting rotates slowly; confirming breathes weakly; needs-user has a one-shot entry cue. Stable state/locale changes do not recreate its shape. Reduced-motion overrides animation with matching specificity; running/ended/failed remain static.
- CLI marks are neutral CC Desk-created conversation-terminal/code-bracket geometry, not copied official logos. Both use fixed low-saturation ink and 16px size. Full names are accessible/Tooltip-only. `CC`/`CX` appear only after the current SVG image fails; CLI change clears failure, stale detached-image errors are rejected, and fallback retains trigger focus identity.
- RED: first exact required command failed on missing new component imports. The importable skeletal components then produced 25 failures of 25 tests before implementation. A strengthened existing reduced-motion assertion reproduced the specificity mismatch and failed before the CSS fix. No unrelated test expansion or full suite was run.
- GREEN: `npm test -- tests/components/sessionIcons.test.ts && npm run typecheck` → 25 tests pass; typecheck exit 0. `git diff --check` passes. Logs are retained in the controller's Task 7 report.
- Verification corrections: animation-rule lookup now skips color-only selectors; the test i18n instance uses inferred factory types rather than incorrect overload generics. These were test selection/typing fixes, not relaxed requirements.
- Windows 100%/125%/150% scaling, actual rendered 1024×640 layout, final screenshots and screen-reader behavior remain final platform/visual gates. Linux jsdom CSS-rule and keyboard tests do not certify those gates.

### Task 7 rulings

- Status SVGs are imported as bundled static raw text and selected only from the exhaustive `SessionVisualState` mapping, allowing theme-token `currentColor` without a new SVG runtime dependency. No caller-provided markup enters the sink. Cost if wrong: future edits must preserve this strict asset allowlist rather than accepting arbitrary SVG strings.
- Neutral CLI SVGs use fixed `#667587` and an explicit local `filter: none` rule that outranks the existing dark-image filter. Cost if wrong: future GUI palettes must retain contrast for that neutral ink; final visual gates remain required.
- The needs-user cue runs once per entry into that visual state; locale/tooltip re-renders do not replay it. Cost if wrong: downstream row identity must remain stable to avoid treating row recreation as a new entry.

### Task 7 independent review repair — icon contrast

- Reviewer found fixed neutral CLI ink below 3:1 on supported dark tertiary/hover/selected backgrounds, and confirming's .65 opacity trough below 3:1 on light row surfaces. The controller verified the reported values; the initial fixed-across-GUI-themes ruling above is superseded by this accessibility repair.
- Added two token-derived contrast regressions before changing production styles. Both failed: CLI selected-over-dark-primary at 2.5370:1 and confirming trough selected-over-light-primary at 2.9198:1 with 8-bit compositing. Tests evaluate both GUI themes, primary/secondary/tertiary/hover backgrounds, and selected overlays over each.
- Minimal production change: same neutral CLI assets use `brightness(1.4)` only under GUI dark theme; session status never determines application-icon treatment. Confirming's weak breath now has .75 minimum opacity. Geometry, labels, tooltip/focus, reduced-motion and image-error fallback contracts remain intact.
- The CLI regression compiles the production scoped stylesheet and uses the actual mounted image's computed filter alongside global tokens. Its emitted selector remains correctly scoped to the CLI image, so inherited/legacy dark filters cannot silently satisfy the test. Both neutral assets retain their common ink and existing original ownership.
- GREEN exact gate: `npm test -- tests/components/sessionIcons.test.ts && npm run typecheck` → 27 tests pass (original 25 plus 2 regressions), typecheck exit 0. `git diff --check` passes. Evidence is retained in the controller's appended Task 7 report.
- Status remains controller-review/publication pending. No commit/push, unrelated full suite, CI, build, packaging or release. Actual rendered/platform/scaling accessibility gates remain pending.

### Task 7 review gate

- Independent spec review passed; scoped quality re-review approved both contrast repairs
- Worst supported-surface contrast: CLI marks 3.1746:1, confirming breath trough 3.2484:1
- Final exact verification: 27 Task 7 tests pass; typecheck pass; diff check clean
- Atomic task publication pending remote readback

### Verified Task 7 checkpoint

- Remote commit `d10fc5a637589de95875e61e1b1d48c332595c32`; fetched tree equals staged tree, local/remote SHA match, clean working tree before this readback
- 27 tests + typecheck, independent spec and quality approval, no CI/PR/package/release
- Next: Task 8

### Task 8 preflight ruling

- Keep SessionItem strictly UnifiedSession-based. SessionList may temporarily adapt its legacy callers to UnifiedSession and old events until Task 9 migrates ProjectNode; do not keep a second visual row implementation. This maintains a buildable functional checkpoint between sequential tasks. Cost if wrong: the temporary compatibility mapping must be removed when Task 9 adopts unified project groups.

## Task 8 — Unified session row and secondary menu — September 30, 2026

- Status: implemented and locally verified; controller review/publication pending. Base remains `d10fc5a637589de95875e61e1b1d48c332595c32`. Existing Task 7 readback and Task 8 preflight ruling are preserved. No commit, remote write, PR, CI, build, package, version or release operation was performed by the implementer.
- Replaced the legacy visual row with strict `UnifiedSession` `SessionItem`: fixed `16px 18px minmax(0, 1fr) 38px 20px` grid, 6px gaps, 38px height, single-line title ellipsis, 3px amber selection marker, Task 7 status/CLI icons and default compact activity age. No inline status/runtime/configuration fields. One state-owned quick control overlays its fixed age slot on hover/keyboard focus; no-action states keep the age.
- Added the shared typed menu-action array in `sessionPresentation.ts` and `SessionOverflowMenu` using Task 6 `AppMenu`. Overflow, pointer context and Shift+F10/ContextMenu entry points use identical definitions and state/visibility rules. Running archive explicitly says “Stop and archive”; unknown/starting cannot offer restart/archive; missing native session IDs disable copy rather than guessing an ID. The teleported menu clamps to the viewport and shares the primitive's keyboard, outside-click and focus-return behavior.
- F2/menu rename uses Task 6 `AppInput` in the same title column. Enter/save emits trimmed `rename-commit`, Escape emits `rename-cancel`, blank/control-character names remain invalid in the editor, blur does not race the explicit save, saving disables repeated submission, and a changed session ID discards the old draft. Rows emit typed action requests only; they do not perform process, clipboard, directory or diagnostic operations.
- `SessionList` accepts unified sessions and forwards unified events without initializing the legacy attention store. Its temporary old tabs/history boundary preserves supported activation/resume, active-tab rename, stopped-tab restart, tab close and history archive using the same row; unsupported legacy actions remain hidden. Task 9 removes the boundary when ProjectNode adopts unified groups.
- RED: exact initial command `npm test -- tests/components/sessionItem.test.ts` failed on the missing `SessionOverflowMenu.vue` import before production changes. No skeletal-component rerun was required by the approved task scope. The first integrated run had 2 failures: test selector incorrectly assumed AppInput's class lived on a wrapper, and deferred menu rename needed an extra render turn. The selector was corrected to its actual input; synchronous editor entry with next-tick focus resolved the timing.
- A later focused pointer regression failed before its fix: reopening the overflow trigger while its menu was open caused AppMenu's outside pointer listener to close it before click, then the click reopened it. The trigger now stops its own pointerdown propagation so repeat click closes normally; outside pointers elsewhere still use the shared close behavior.
- GREEN exact required gate: `npm test -- tests/components/sessionItem.test.ts tests/i18n/translations.test.ts && npm run typecheck` → 31 tests pass (27 row/menu/list + 4 i18n); typecheck exit 0.
- Narrow affected regressions: `npm test -- tests/components/sessionIcons.test.ts tests/components/uiPrimitives.test.ts tests/utils/sessionPresentation.test.ts tests/utils/relativeTime.test.ts && git diff --check` → 61 tests pass (27/27/4/3), diff check exit 0. No unrelated full suite was run. Existing npm http-proxy configuration and Vite CJS deprecation warnings remain; no component warnings or unhandled errors occurred in the final runs.
- Test evidence covers state-owned quick actions, strict unified events, no visible state words, exact production CSS declarations and stable geometry rules, compact age/localized full-time tooltip, shared-clock refresh and final-consumer cleanup, activation, F2/save/cancel/identity ownership, complete menu state sets, localized stop-and-archive, shared context/overflow definitions, viewport clamp, focus return, repeat toggle and legacy supported event routing.
- Actual rendered 1024×640 font geometry, Windows 100%/125%/150% scaling, platform screenshots and screen-reader behavior remain final visual/platform gates. jsdom/CSS-rule verification does not certify them.

### Task 8 rulings

- The Task 1 `relativeTime.ts` had only its compact formatter, not a clock. Added one reference-counted minute clock in that shared utility, with cleanup after the final mounted consumer, rather than per-row timers. Types and `sessionPresentation.ts` were also added to Task 8's file map to own the typed shared action model. Cost if wrong: future consumers must subscribe through this helper rather than introduce another refresh loop.
- Fixed trailing grid columns require row-specific `IconButton` geometry of 20px wide × 28px high; all menu items and other shared controls retain Task 6 sizes. Calendar-form ages use a smaller 9px numeric treatment rather than truncation in the fixed 38px age column. Cost if wrong: final font/scaling review may require a local readable size/layout adjustment while preserving the frozen grid and untruncated time strings.
- Unknown/starting rows expose confirmation/cancel and close requests but never restart/archive requests. Runtime ownership, confirmations for running close/archive/restart, exact-ID copy and diagnostic redaction remain caller duties in subsequent migration tasks; this row layer must not bypass authenticated runtime adapters. Cost if wrong: callers must validate state again when handling a typed request.

### Task 8 independent review repair — cross-row menu dismissal

- Reviewer reproduced two simultaneously open menus when clicking another row's overflow opener. The earlier unconditional pointerdown stop fixed own-opener toggle but also suppressed another row's outside-dismiss signal; that overbroad interception is superseded.
- Added `Row_AnotherOverflowDismissesPrevious_028` before changing production code. RED: 1 failure of 28 row tests, expected one menu but got two. The test also asserts opener expanded states, focus inside the replacement menu, and a second click closing that same opener.
- Minimal fix: overflow pointerdown stops propagation only while that row's own menu is open. An inactive other-row opener reaches the shared AppMenu document listener, closing the preceding menu without taking focus, then opens its own menu. Existing `Row_OverflowToggle_027` still verifies own-opener close behavior.
- GREEN exact command: `npm test -- tests/components/sessionItem.test.ts tests/i18n/translations.test.ts && npm run typecheck && git diff --check` → 32 tests pass (28 row/menu/list + 4 i18n); typecheck and diff check exit 0. No unrelated full suite, commit or remote action.
- Controller's previously staged snapshot is preserved; this repair modifies SessionItem, its test and this ledger on top of it. Review/publication remain pending.

### Task 8 review gate

- Independent spec/quality review approved the final cross-row menu dismissal repair; no remaining Task 8 blocker
- Final exact gate: 32 row/i18n tests, typecheck and diff check pass; earlier narrow affected regression gate61 passed
- Atomic task publication pending remote readback

### Verified Task 8 checkpoint

- Remote commit `aeb644965aa214fbca3885169c1132ed2d5d5ff5`; exact staged/fetched tree and local/remote SHA match; clean working tree before this readback
- Final32 tests/typecheck; independent spec/quality review approved; no CI/PR/package/release
- Next: Task 9

## Task 9 — Unified project tree and archived session drawer — September 30, 2026

- Status: implemented and locally verified; controller review/publication pending. Base remains `aeb644965aa214fbca3885169c1132ed2d5d5ff5`. The existing Task 8 remote readback is preserved. No commit, remote write, PR, CI, build, package, version or release operation was performed by this implementer.
- Migrated `ProjectNode` and `SessionsPanel` to `UnifiedProjectGroup[]` and unified typed requests. Claude Code and Codex CLI rows are directly mixed below their project through the same `SessionItem`. `SessionList`'s Task 8 temporary legacy tabs/history, attention-store mapping and old events are removed; its obsolete legacy-boundary test is replaced by strict unified menu/rename/event coverage.
- Project rows use a stable 40px grid with reserved arrow/title/attention/new-session/overflow columns, single-line title ellipsis and full project-path tooltip. The only project quick action is new-session. Pin/unpin, rename, archive view, directory open and removal share one Task 6 `AppMenu` across overflow/context entry points. Collapsed needs-user markers use unified counts. Tree groups are nested inside their project treeitem.
- Added `ArchivedSessionsDrawer` using shared `AppDrawer` and the existing unified row/list. Archived records remain in the Task 5 catalog and outside normal groups. Archive-only projects receive one empty UI project shell for their archive menu; a panel-wide archive entry remains usable through search/no-results. Restore emits a catalog-ID `restore-request`, leaves mutation/publication to the caller, and does not launch a CLI or delete CLI native history.
- Running archive is intercepted as `{ kind: 'stop-and-archive', sessionId, projectKey, projectPath }` through `confirmation-request`, rather than emitting an immediately executable archive action or calling stop. Unknown/starting live states refuse archive. Adapter initialization and runtime dispatch remain Task 11; full confirmations remain Task 16. Project administration and restore are typed requests for their integrating callers.
- Removed the entire global Legacy skip-permissions/custom-args footer. Search uses display name/basename/path/session title, temporarily expands matches and does not modify explicit expansion. Nested project controls, session rename, menu Escape and drawer focus stay isolated from parent toggle/close behavior.
- RED exact initial gate: `npm test -- tests/components/projectSessionTree.test.ts` failed on the missing `ArchivedSessionsDrawer.vue` import before production changes. The approved missing-import RED was sufficient; no skeletal-component loop was introduced. First integrated tree/keyboard run passed 12 tests; typecheck exposed the expected obsolete SessionList test and older SidebarPanel listener parameter inference. Removed the obsolete test and added only explicit parameter types to those old listener lambdas.
- Additional TDD regressions: drawer-menu focus boundary failed before its opt-out fix (menu teleported outside the modal); old-container create isolation failed before renaming the unified event (the old `newSession` event was incorrectly emitted); project/session ARIA hierarchy failed before moving the project treeitem boundary around its nested group. Each subsequently passed. A source-only keyboard assertion initially expected adjacent `.self.prevent` modifiers; reordered equivalent modifiers to match the shared keyboard gate.
- Final exact required gate: `npm test -- tests/components/projectSessionTree.test.ts tests/sidebarKeyboardHandlers.test.ts && npm run typecheck` → 14 tests pass (13 tree + 1 keyboard), typecheck exit 0.
- Final narrow affected gate: `npm test -- tests/components/sessionItem.test.ts tests/stores/unifiedSessions.test.ts tests/i18n/translations.test.ts tests/components/uiPrimitives.test.ts && git diff --check` → 71 tests pass (28/12/4/27), diff check exit 0. No full suite or unrelated platform validation was run. Existing npm http-proxy configuration and Vite CJS deprecation warnings remain; no component warnings or unhandled errors occurred in the final runs.
- Updated `AGENTS.md` and `docs/components.md` with the unified tree, request ownership, retained archive reachability and shared modal/menu topology. Actual Windows 1024×640 font geometry, 100%/125%/150% scaling and rendered screen-reader checks remain final platform gates.

### Task 9 rulings

- Task 5 `projectGroups` intentionally omit archived sessions and therefore omit all-archived projects. UI-only empty shells plus a global archive entry avoid stranding their records without altering the store's normal grouping contract. Cost if wrong: upcoming project management/container integration must reconcile registered/empty/hidden projects with the same single tree, not add a second archive tree or mutate normal groups to include archived rows.
- `new-session-request` is deliberately distinct from the older SidebarPanel's Legacy `new-session` launch handler. A regression proves the intermediate container cannot interpret the unified plus click as a Legacy PTY launch. Only four old listener lambda annotations change in SidebarPanel to retain typecheck; they do not implement runtime routing or expose a duplicate tree. Task 10/11 must adopt the new typed event model.
- `SessionOverflowMenu` normally teleports to body. `menuTeleport=false` is passed only in the archived drawer, retaining the same fixed-position shared menu inside AppDrawer's DOM/focus boundary. The shared modal focus trap is unchanged. Cost if wrong: other modal consumers must explicitly keep their menus inside the same focus boundary rather than weakening the trap.
- Tree components do not permanently remove Native history or map native IDs into old delete APIs. Project rename/removal and other administrative menu actions are typed requests; integrating surfaces own edits, persistence and the appropriate confirmation. Only list-only archive restoration is requested at this stage; restore-and-open can be provided by the later unified resume flow.

### Task 9 review gate

- Independent spec and quality review approved the staged snapshot with no actionable blocker
- Controller reran exact required gate:14 tests and typecheck pass; implementer narrow affected gate71 passed
- Runtime/action dispatch remains Task10/11 and confirmation UI Task16; this checkpoint adds no Legacy fallback
- Atomic publication pending remote readback

### Verified Task 9 checkpoint

- Remote commit `f2764aa4728983fbdf8989f9c4bb9881a9069d8b`; exact staged/fetched tree and local/remote SHA match; working tree clean before this readback
-14 required tests/typecheck,71 narrow regressions, independent spec/quality approval; no CI/PR/package/release
- Next: Task 10

### Task 10 preflight ruling

- Existing product-boundary assertions demand a first-class Native top-level entry and literal old overlay conditions. These conflict with the frozen unified-shell spec. Replace those old presentation assertions with equivalent unified dual-CLI/authentication/partial-availability boundary checks; do not retain dead strings to appease tests. Cost if wrong: any old navigation regression must be detected by the new behavioral shell tests instead.
- Old automatic Legacy startup and Native/Legacy product views may execute only behind the temporary development-only compatibility flag; the normal shell must not start a Legacy PTY as an implicit default. Actual unified runtime boot/terminal wiring belongs Task 11. Cost if wrong: this intermediate shell checkpoint has no terminal host until the next task.

## Task 10 — Unified application shell and primary navigation — September 30, 2026

- Status: implemented and locally verified; controller review/publication pending. Base remains `f2764aa4728983fbdf8989f9c4bb9881a9069d8b`. Existing Task 9 remote readback and both Task 10 preflight rulings are preserved. No commit, remote write, PR, CI, build, package, version or release operation was performed by this implementer.
- Added `useShellStore`, `AppShell`, `PrimaryNav`, `WorkspaceView` and `WorkspaceHeader`. Primary sections are exactly Workspace/Projects/Settings. The shell owns four columns (44px/288px/flexible/optional344px) with bounded session/resource widths, min-width zero and global overflow containment. Context uses shared `AppDrawer` below1180 logical CSS pixels; sessions default collapsed below900 with separate compact/desktop choices.
- Migrated normal `App.vue` to unified routing and exact Task 9 typed tree requests. The workspace and single future terminal-host slot stay mounted across section changes; settings stays mounted after first activation. Project management currently has a content-only landing for Task 14. SidebarPanel forwards unified requests without mapping `new-session-request` to old Legacy events. No second tab strip or independent Native product entry is exposed.
- Moved the old App into `LegacyCompatibilityApp.vue`, activated only by BOTH Vite DEV and explicit `VITE_CC_DESK_COMPATIBILITY=1`. Production cannot enter this path by setting the flag alone. Normal initialization loads config/GUI preferences independently of CLI availability, never runs the old Claude-only check/startup decision or mounts either old runtime product page. Native authenticated bridge, terminal and adapters remain unchanged.
- Preserved Windows minimize/maximize/close and macOS traffic-light space; TitleBar accepts the unified project/session title, ellipsizes long titles and supplies localized control names/focus. Safe OS Settings/Shortcuts menu navigation remains; directory/restart events produce typed requests only. GUI theme changes preserve terminal-theme preference and unified selection. Per-CLI unavailable presentation never blocks the other CLI or navigation.
- Necessary file-map expansions: TitleBar to remove the old Native top-level toggle while preserving window controls; LegacyCompatibilityApp to isolate old startup; English/Chinese locale keys; productBoundary assertions under the approved preflight ruling; and only the window geometry fields in `src-tauri/tauri.conf.json` to align actual default/minimum with1024×640. Read root and src-tauri AGENTS before edits. No Rust, version, bundle, security or updater metadata was changed.
- RED: `npm test -- tests/components/appShell.test.ts` failed on the missing AppShell import before production changes. The approved missing-module RED was sufficient. First integrated run had26 passing/2 failing tests: one test compared recreated Vue Test Utils public proxies rather than the retained DOM host, and another assumed a data attribute wrapped AppButton instead of being on its actual button. Those test observations were corrected without changing expected behavior.
- Additional TDD boundary regressions: hidden session-panel Escape changed desktop visibility during Settings, and the header's default-project request carried an entire group rather than exact project identity. Both tests failed before minimal fixes: App now ignores hidden-panel close intents and projects the header identity to only projectKey/projectPath. Their final tests pass.
- Final exact required gate: `npm test -- tests/components/appShell.test.ts tests/productBoundary.test.ts && npm run typecheck` →30 tests pass (20 shell/10 product boundary), typecheck exit0. `git diff --check` passes. Native bridge safeguard assertions remain intact; obsolete Native-entry/old-overlay assertions are replaced with unified dual-CLI, partial-availability, DEV-boundary and single-host checks.
- Narrow affected gate: `npm test -- tests/components/projectSessionTree.test.ts tests/sidebarKeyboardHandlers.test.ts tests/components/uiPrimitives.test.ts tests/i18n/translations.test.ts tests/stores/app.test.ts tests/stores/unifiedSessions.test.ts tests/designTokens.test.ts` →109 tests pass (13/1/27/4/43/12/9). Existing app-store negative tests print their expected config/home-read errors; existing npm http-proxy and Vite CJS deprecation notices remain. The final exact shell gate has no component warnings or unhandled errors. No unrelated full suite was run.
- Updated AGENTS and component architecture documentation. Linux jsdom DOM/event/CSS-contract checks do not certify rendered1024×640 geometry, Windows100%/125%/150% scaling, actual OS controls or screen-reader behavior; those remain final platform gates.

### Task 10 rulings and downstream interfaces

- `WorkspaceRequest` is a typed presentation-only intent union, also emitted by App as `workspace-request`. Shell stores only the latest ephemeral intent plus monotonic requestSequence; an owner clears only its matching sequence. It is not a persistent queue and never admits, launches or replays an operation. Cost if wrong: Task 11 must explicitly route each current request, revalidate runtime state/authority and confirmations, and must not interpret a frontend path as Native filesystem authorization.
- Task 11 configures unified adapters and fills WorkspaceView's single `terminal` slot while preserving mounted host/selection across section and GUI-theme changes. Task 12 owns new-session dialogs, Task 13 resume and Task 16 confirmation dialogs/consequential action admission. The shell's waiting notice is an honest intermediate checkpoint, not a successful runtime operation. Cost if wrong: requests remain inert until integration; do not add Legacy fallbacks to make the checkpoint appear functional.
- Context remains read-only placeholder content until Task 15 authenticated resource integration, with a `context` slot owned by AppShell. WorkspaceView accepts per-CLI `unknown | available | unavailable` observations; unavailable notices are scoped, never global gates. Cost if wrong: the runtime owner must supply actual observed availability without guessing from old Claude checks.
- The temporary DEV compatibility App, IconBar adapter and old caller type declarations are removal targets for Task 21. The old views are retained only there; normal content must never gain its own IconBar/global shell. The current existing SettingsView shell remains for Task 17 settings migration; terminal preferences belong to Task 18.

### Task 10 independent review repair — active surface ownership

- Reviewer reproduced the persistent SessionsPanel's teleported archive modal/focus trap surviving navigation to Settings. ProjectNode and SessionItem teleported menus had the same inactive-surface ownership gap. AppShell's resource drawer was already section-scoped.
- Added four archive navigation tests before the fix: OS Settings, OS Shortcuts, direct Settings and direct Projects. RED:4 failures/20 passes. Added one project-menu and one row-menu navigation regression before extending ownership to those owners. RED:2 failures/24 passes. Logs are retained with the Task 10 report.
- Minimal ownership chain: App supplies Workspace-and-visible `active` to SidebarPanel/SessionsPanel; SessionsPanel closes its archived modal and clears only its modal project scope on inactivity. `surfaceActive` propagates through ProjectNode/SessionList/SessionItem, closing only their controlled menus and preventing hidden-surface delayed rename focus. Search, explicit expansion, inline rename state, unified selection and the workspace/terminal-host DOM are retained. Shared menu/modal/overflow primitives are unchanged.
- The first narrow row/tree rerun found four regressions (`Tree_ArchivedDrawerRestore_005`, `Tree_DrawerMenuFocusBoundary_012`, `Row_AnotherOverflowDismissesPrevious_028`, `List_UnifiedMenuAndRenameOnly_026`): absent optional Boolean surfaceActive on SessionList became Vue false and disabled standalone/drawer row menus. Added the explicit list default true; all existing assertions remain unchanged.
- Final superseding verification: `npm test -- tests/components/appShell.test.ts tests/productBoundary.test.ts && npm run typecheck && npm test -- tests/components/projectSessionTree.test.ts tests/sidebarKeyboardHandlers.test.ts tests/components/sessionItem.test.ts && git diff --check` →36 shell/product tests (26/10), typecheck exit0,42 narrow tree/keyboard/row tests (13/1/28), diff check exit0. No component warning or unhandled error occurred. Original109-test narrow evidence remains historical; its affected tree/row subset was freshly rerun after the repair.
- Four additional production paths (SessionsPanel, ProjectNode, SessionList, SessionItem) are necessary active-surface boundary changes; the full Task10 path list is now22. Controller's already-staged snapshot is preserved with these repairs unstaged on top. Updated AGENTS/components/report and preserved all readbacks/preflight entries. Review/publication remain pending; no commit, push, CI, build, package, release or unrelated full suite.

### Task 10 review gate

- Independent spec/quality review approved the consolidated active-surface repair: archive drawer and project/session menus close when Workspace is inactive, while host/tree state remains mounted
- Final exact snapshot:36 shell/boundary tests, typecheck,42 affected tree/row tests and diff check pass
- Earlier109-test broader narrow gate is historical before the ownership repair; full final validation remains Task25
- Atomic publication pending remote readback

### Verified Task 10 checkpoint

- Remote commit `8683f6b9e56c2375a2520508f104754c8fd4553c`; exact staged/fetched tree and local/remote SHA match; working tree clean before this readback
- Final36 shell/boundary tests, typecheck,42 affected regressions; independent spec/quality review approved; no CI/PR/package/release
- Next: Task 11

### Task 11 integration preflight

- The normal App now publishes typed ephemeral WorkspaceRequest intents but deliberately has no adapter initialization or connected terminal host. Task 11 must provide real runtime-port admission/dispatch; a mocked-only host would not complete this task.
- XTermTerminal already owns a map of Legacy terminals. Reuse that ownership rather than mounting duplicate aggregators per Legacy descriptor; remove embedded legacy chrome from the unified content path.
- Preserve Native sourceSessionKey/profile/revision/project identity from Task 5 repairs and capture exact attempt ownership before asynchronous lifecycle actions.
- Task 12/13 own create/resume dialogs, Task14 registration UI, Task15 resources and Task16 consequential confirmation UI. An unhandled or confirmation-requiring request must not silently execute or fall back to Legacy.

## Task 11 — Unified terminal host/runtime integration

- Normal App now configures actual adapters/bootstrap and one stable host: one Legacy aggregator plus one Native terminal per open Native descriptor; historical records never instantiate terminals
- Visibility guards separate focus/fit/user input from background protocol replies/output ACK; hidden terminals retain buffers and defer fits
- Exact Native source context is preserved before creation; queued lifecycle actions capture request/run/generation or Legacy local PTY ownership before waiting
- RED→GREEN covered hidden Legacy input/clipboard, stale restart after close, unknown launch reentry, stale completed close selection, exact source mismatch, removed registration during restart and real binding user/protocol separation
- Final required gate:26 host/identity/binding/attempt tests pass. Affected gate:216 tests across runtime, Legacy visibility, shell, adapters/stores, input/paste and safety boundaries pass. Typecheck and diff check pass
- No Native backend command/schema/bridge/input queue protocol changes; optional user-input admission predicate preserves protocol/ACK behavior and paused unknown/partial queues
- Runtime bootstrap reads sources independently and starts no process, creates no default profile and performs no implicit registration. Known-source failures remain isolated
- Create/resume UI12/13, project UI14, resources15 and confirmations16 remain pending owners; unhandled/consequential requests are not synthesized into approved lifecycle actions
- Independent review/publication pending. All checks are mocked host/unit integration; no real CLI/account or platform/D20 certification

### Task 11 rulings

- Added an optional captureOwnership adapter contract so facade queue admission freezes the actual attempt before awaits; otherwise a queued close could recapture a newer run. Cost if wrong: future adapters must supply equivalent ownership capture before enabling lifecycle mutations.
- Added an optional isUserInputAllowed binding predicate and frontend Legacy PTY generation bookkeeping without changing the wire protocol. Cost if wrong: future renderer upgrades must preserve proven input provenance and background protocol replies.
- Known Native attempts fail closed if a remount loses the in-memory binding, rather than relaunching implicitly. Cost if wrong: explicit recovery may be needed after abnormal host loss, but unknown launch is not replayed.

### Task 11 independent review repairs and gate

- R1: real installed xterm parser proved visibility-driven disableStdin suppressed DSR/DA replies before provenance. Removed parser-wide suppression; actual component/parser/Native binding tests retain hidden protocol/ACK while blocking hidden user input
- R2: real Legacy store ordinary history selector filtered archives before adapter projection. Added explicit unfiltered getCatalogHistoryFor port; real store/adapter/drawer archive-refresh-restore round trip passes
- R3: first Legacy launch could precede process-event registration because optional drag/drop was awaited first. Core output/exit receipts now gate spawn independently; closed/replaced ownership is rechecked after readiness and late disposal cleans subscriptions
- All three had behavioral RED→GREEN evidence. Final exact-source gate:26 required tests,223 affected tests across17 files, typecheck and diff check pass; these supersede the216-test pre-review claim
- Independent scoped review approved spec and quality and independently ran all7 repair regressions successfully
- Installed parser/store/component integration is not real CLI/account/platform/D20 certification; those gates remain unperformed
- Atomic publication pending remote readback

### Verified Task 11 checkpoint

- Remote commit `b1309bd294f153e99449b0d3413542ba923a294e`; exact staged/fetched tree and local/remote SHA match; working tree clean before this readback
-26 required tests,223 affected tests, typecheck and independent approval; seven review-repair regressions independently rerun successfully
- No CI/PR/package/release or real CLI/platform/D20 acceptance; next Task12

### Task 12 preflight rulings

- Frozen spec12.1 requires automatic registration within the two-click create flow, but the plan's registration helper is listed under14. Implement the minimal reusable registration prerequisite now using the existing authenticated registered-project store; Task14 will reuse it for management UI. No registration occurs during read-only bootstrap. Cost if wrong:14 must adopt this helper rather than create a second writer.
- Safe default launch configuration may be created only as part of the explicit create flow, reusing existing profile patch/CAS contracts; no background profile creation or Legacy fallback. Conflicts/unknown commits reload but never automatically repeat a mutation or launch. Cost if wrong: the user sees a retry/configuration guidance step when preparation cannot complete safely.
- Native tab creation is admission, not proof that the CLI started. Starting feedback must cover async preparation; failed rows remain addressable, unknown launches require status recovery, and last-successful preferences must not be saved as though an unconfirmed launch succeeded.


## Task 12 — Quick new-session flow

- Project plus and ordinary workspace header open the same quick chooser; Claude/Codex selection reaches actual normal App preparation/catalog/Native admission and existing unified host. More Options opens the shared advanced dialog
- Immediate starting placeholder precedes asynchronous registration/configuration preparation. Failure remains addressable; cancellation blocks late admission; unknown mutation/admission is never automatically replayed
- Exact raw argv stays string-array based: per-line arguments preserve blanks, JSON is explicit, unrepresentable line conversions retain JSON. No shell splitting or new backend launch protocol
- CLI availability uses revision-scoped read-only observations, not account/startup certification; one unavailable CLI does not disable the other
- Permission presentation describes existing selected/inherited configuration; no unsupported per-launch override or silent saved-profile mutation is introduced. Restore requests remain explicit pending Task13 intents
- Controller caught and repaired initial project-history localStorage authority and header-to-Advanced routing. Canonical project+CLI history now reads/writes projectsState/projects.json, only after an exact matching running receipt; separate global CLI defaults remain optional local UI storage
- Canonical preference setter computes both fields inside its serialized mutation, preserving concurrent Claude/Codex successes. Failed metadata saves issue safe feedback/read-only recovery without failing or replaying the running process or repeating the write
- Final exact snapshot before independent review:22 required tests,205 affected tests across22 files, typecheck, diff check and28-file manifest readback pass. Behavioral RED→GREEN repairs documented in the task report
- Independent review and atomic publication pending. No real CLI/account/platform acceptance, CI, PR, package or release performed

### Task 12 independent review repairs and gate

- R1: pre-ready cancel could wait behind unrelated bootstrap and admit late. Locally owned preparation/open-session requests now dispatch before global readiness; cancellation invalidates preparation immediately
- R2: reselecting a placeholder, or closing a different ended row, could lose selected identity on admission. Separate selection-intent ownership from lifecycle invalidation; only the still-selected current preparation transfers, and newer selection/activation remains protected
- R3: committed-but-rejected preference acknowledgement released the writer before recovery, allowing the next CLI update to erase the committed field. Read-only recovery now remains inside canonical serialization; failed readback marks state unverified and later writes must load or fail closed. No mutation/process replay
- R4: permission copy now describes Desk flag injection only and defers effective permissions to saved arguments and CLI settings; no unsupported assurance, flag parser or configuration mutation
- All findings have behavioral RED→GREEN regressions. Final exact snapshot:25 required tests,210 affected tests across22 files, typecheck, diff check and29-file manifest pass
- Independent scoped review approved all four findings; original four reproductions, six repair regressions, both selection reproductions and four focused selection/activation checks passed across review rounds
- Earlier22/205 and25/208 counts are historical, superseded by final25/210. No actual CLI/account/platform/D20 certification or CI/PR/package/release
- Atomic publication pending remote readback

### Verified Task 12 checkpoint

- Remote commit `72644819200a298f7b6689dfa512289036d37177`; exact staged/fetched tree and local/remote SHA match; working tree clean before this readback
-25 required tests,210 affected tests, typecheck, manifest and independent spec/quality approval; all review findings repaired
- No CI/PR/package/release or real CLI/platform acceptance; next Task13

### Task 13 preflight ruling

- Ruling: verified missing-history Remove Record evicts only exact cached/catalog identity and matching optional sessionRecords via canonical writer. Existing deleteSessions deletes real history files and is prohibited for this action. Do not add permanent tombstones/schema; a later successful authenticated scan may rediscover a restored source record. Why: the approved action removes an unavailable app record, not real CLI history or a durable ignore rule. Cost if wrong: rediscovered history can reappear; a permanent suppression preference would require a separately explicit data contract.
- Ruling: legacy Native archive keys remain recognized only when uniquely mapped to the new full-origin catalog identity; explicit restore may clear that exact old key. Ambiguous mappings retain metadata and produce safe ambiguity feedback without guessed mutations; new archives use full-origin keys. Why: expanding identity must not orphan unambiguous saved archives or accidentally affect another source. Cost if wrong: formerly colliding archives require explicit origin clarification rather than automatic restoration.
- Ruling: paginated offset history discovery may establish positive matches but cannot certify absence when the backend supplies no stable-snapshot evidence. Missing/removal proof requires a complete ready single response bound to the original authenticated sourceRootKey/sessionKey; changed root or uncertain multi-page negative reads fail closed. Why: concurrent reorder/root replacement is not proof a session disappeared. Cost if wrong: some large-history missing records cannot be removed through this UI until authoritative backend absence evidence exists; no snapshot protocol is invented in this task.

## Task 13 — Unified resume/history search

- Normal App quick Restore, historical row activation and advanced history/direct-ID/native-picker intents use one shared dialog, explicit confirmation and real runtime/catalog/adapter/host wiring
- Search supports title/ID/CLI/time/current-or-all scope, read-only paginated source discovery, partial-source feedback, stale-response rejection and active-surface cancellation
- Native catalog/admission identity now includes exact CLI/profile/revision/project/sourceSessionKey/session identity; concurrent same-origin restore coalesces and rechecks claims. Legacy claims/deduplication are project-scoped
- Historical origin remains frozen; changed/missing configuration or authenticated revision conflicts fail closed with safe guidance, never substitute defaults or invoke new-session preparation
- Direct modes freeze explicit existing configuration and registration, use existing backend actions and bypass new-session preparation; no new registration/profile mutation
- Explicit missing cleanup revalidates absence and only removes exact optional canonical metadata/cache identity; history files remain untouched. Uniquely resolvable old Native archive keys remain compatible; ambiguous keys preserve metadata and fail closed
- Independent review R1/R2 repaired absence proof: original authenticated root plus one complete ready response is required; root replacement or unstable multi-page negative discovery cannot authorize removal
- Review R3 repaired Native/Legacy shared admission ownership: individual caller guards preserve fresh reconfirmation, canceled callers cannot authorize or claim success, all-canceled work cannot admit late
- Final exact-source gate:34 required tests,289 affected tests across19 files, typecheck, diff check and20-file manifest pass. Independent scoped spec/quality approval reran original3 reproductions and6 focused Native/Legacy/App regressions successfully
- Earlier29-test required count superseded. No actual CLI/account/platform/scaling/D20 acceptance, full suite, CI/PR/package/release; atomic publication pending readback

### Verified Task 13 checkpoint

- Remote commit `181c6f51c509468e3f6c3f2a844131dcd3975e65`; exact staged/fetched tree and local/remote SHA match; working tree clean before this readback
-34 required tests,289 affected tests, typecheck, manifest and independent approval; all3 original review reproductions and6 focused regressions independently pass
- No CI/PR/package/release or actual CLI/platform/D20 acceptance; next Task14

### Task 14 preflight rulings

- Ruling: reuse the existing trusted-main-window profile-independent Native registration contract. The named helper validates profile context when supplied/needed for launch; it does not invent a profile-bound registration API or create configuration. Add registers once and projects that path into unified management/Legacy discovery presentation; no nonexistent Legacy add command is introduced. Why: filesystem registration and launch/resource authorization are separate existing contracts. Cost if wrong: a project without any CLI configuration is manageable but needs explicit launch preparation later.
- Ruling: Hide retains Native registration; Remove unregisters Native and hides rediscovered Legacy list entries while preserving CLI files and archive/display/preference metadata for re-add. Clear obsolete pin/list membership via existing commands when safe; no backend purge command. Explain removal as removing from CC Desk list rather than erasing every saved preference. Block removal while any open/live/unknown/preparing session owns the project; do not implicitly stop/close. Partial/unknown multi-store writes require safe feedback/read-only reconciliation, never automatic mutation replay. Cost if wrong: re-added projects retain prior display/preferences, and users must explicitly close sessions before removal.
- Ruling refinement: never register Legacy-only projects merely to Hide or reinterpret old hidden/unregistered rows as permanently removed. Preserve hiddenProjects semantics and provide explicit Show hidden access; Remove suppresses ordinary list visibility and additionally unregisters Native when present. Why: no existing removed-vs-hidden persistence marker exists, and old hidden data must stay recoverable. Cost if wrong: Legacy-only Hide and Remove share persisted visibility semantics; explicitly showing hidden/history or re-adding can rediscover removed entries.

## Task 14 — Unified project management

- Normal Projects landing replaced by compact merged discovery/registration/catalog list with search, pinned-first recent/name sort,50-project coverage, active counts and long-path middle ellipsis
- Real add/open/new-session/pin/rename/open-folder/hide/show/remove actions reuse shared controls and canonical writers. Add uses one normalized registration/adoption flow; no second visible registration or new backend/schema/CLI installation
- Removal explicitly confirms ordinary-list removal, preserves files/history/display/archive/preferences, unpins/unregisters when applicable, and blocks any authoritative Native/Legacy/preparing ownership. Show hidden/re-add preserves recovery
- Project-open invalidates old active session context without stopping processes; hidden archived-only shells cannot reappear in ordinary tree. Fixed menus escape scrolling-list clipping
- Review R1: visibility writer rechecks ownership after initial read and queue waits, with per-path barrier spanning mutation; no hide after a late owner appears
- Review R2: publication versions fence delayed startup/visibility reads against newer acknowledged hidden state; subsequent writes preserve prior successful hides
- Review R3: Legacy restore freezes per-caller admission/barrier ownership across historical awaits, rejects during/after project mutation, and cannot create/start an orphaned tab. Independent cancellation/coalescing remains intact
- Final exact gate28 tests+typecheck; final repair-affected167 tests across9 files. Earlier185-test14-file gate is pre-repair historical evidence; affected subset was rerun. Diff/manifest/reverse-apply checks pass
- Independent spec/quality approval resolved all3 findings; reviewer reran original3 reproductions,10 targeted repair/ownership/cancellation checks and4 Legacy/coalescing checks successfully
- No actual CLI/backend/platform/scaling/D20 certification, full suite, CI/PR/package/release; atomic publication pending readback

### Verified Task 14 checkpoint

- Remote commit `d1a6dc6d6fbb22057245e628c0b6eedcf83fe36a`; exact staged/fetched tree and local/remote SHA match; working tree clean before this readback
-28 required tests/typecheck and167 repair-affected tests; independent approval with3 original repros,10 repair checks and4 Legacy/coalescing checks passing
- No CI/PR/package/release or actual CLI/platform acceptance; next Task15

### Task 15 preflight rulings

- Ruling: Legacy instructions explicitly show unavailable because no existing authorized instruction-document reader exists; do not introduce arbitrary filesystem/default-root access. Other Legacy categories use only existing read-only scoped authority. Cost if wrong: Legacy instruction content is unavailable until a separately authorized reader contract exists.
- Ruling: existing authenticated resource response budgets remain enforced and hasMore is visibly partial; do not present offset pages as a complete stable snapshot when no snapshot token exists. Cost if wrong: large resource collections show bounded partial results rather than an invented completeness guarantee.
- Ruling: Legacy readers that mix ambient home data are projected only when records positively identify exact project/local scope; omit ambient/global records without selected-session root authority and label the view project-only/partial. Cost if wrong: globally inherited Legacy resources are intentionally omitted rather than falsely attributed to the selected session.

## Task 15 — Structured project resources

- Six typed read-only categories connect to normal App context slot; shell alone owns dock/overlay/focus. Same-owner refresh retains labeled stale content; identity/category changes clear it and reject late scope/read/error/finally publication
- Native reads pin exact current session/project/CLI/profile revision/request/run/generation and existing authenticated client; invalid submitted run authority never falls back to defaults. One existing bounded page is labeled partial when hasMore
- Safe display DTOs exclude raw transport fields, env/headers/credentials/arbitrary paths/errors; settings allowlist and conservative whole-field withholding protect structured views. Legacy instructions unavailable; other Legacy reads positively project-only/partial
- Independent review R1 repaired failed/null launchRevision ambiguity: private exact-attempt never-started proof is revoked before start preparation and on unknown/error evidence. Actual terminal malformed-receipt regression prevents profile fallback without changing retry semantics
- Review R2 repaired ancestor-mixing Legacy MCP labels: reuse existing getProjectConfig source.path DTO and verify exact fixed selected-project path internally; settings apply same check. Missing/ancestor/other/relative paths are omitted and never rendered
- Final required78 tests+typecheck;266 affected tests across23 suites; diff/manifest/reverse-patch checks pass. Earlier71/219 counts superseded
- Independent scoped spec/quality approval reran original2 repros and12 repair/positive-authority checks successfully
- No actual CLI/account/Legacy plugin execution/platform/scaling/D20 certification, full suite, CI/PR/package/release; atomic publication pending readback

### Verified Task 15 checkpoint

- Remote commit `6ae851a3a3ee9de789a5085b5a0e1d03b551eb44`; exact staged/fetched tree and local/remote SHA match; working tree clean before this readback
-78 required tests/typecheck and266 affected tests; independent approval with2 original repros and12 targeted authority checks passing
- No CI/PR/package/release or actual CLI/platform acceptance; next Task16

### Task 16 execution fallback

- Platform refused a fresh implementation agent with `agent thread limit reached`. Reused a completed engineering worker with a self-contained Task16 assignment; previous Task13 remains sealed and is not repeated. Independent review will use a separate completed reviewer. No environment switch, permission expansion or gate reduction.

## Task 16 — Safe interaction feedback and confirmations

- Normal App running close/stop-and-archive/unknown restart now use typed shared confirmation dialogs and exact Native/Legacy ownership. Cancellation/new selection/new intent prevents later steps and stale feedback without compensating already-issued writes/stops
- Unknown restart recovers exact status, requires definite ended/known receipt after stop, then admits restart; unresolved status remains fail-closed with no launch replay
- Project removal shared confirmation preserves registration/visibility/session barriers. Real launch-config deletion request/confirm/CAS contract and App binding support future Task19 editor; raw delete patch rejects confirmation bypass
- Safe own-property error allowlists prevent inherited-key templates; profile/workspace/canonical error state retains fixed safe codes. Per-CLI banners, local inline feedback, owned low-risk toasts and truly all-source workspace failure are distinct
- Behavioral regressions cover stale same-ID attempt/project replacement, cancellation, Legacy late rename, partial availability, fatal host visibility and configuration-delete/preparation barrier
- Independent review R1/R2 repaired post-queue cancellation: unregister, removal pin cleanup and both Native/Legacy archive invoke ownership guards inside actual serialized writers immediately before IPC. Canceled unissued writes bypass error/readback paths; genuine uncertain writes retain read-only recovery/no replay
- Final required43 tests+typecheck;290 affected tests across25 files plus37 real Legacy store tests;29-source manifest/diff checks pass. Earlier33 required count superseded
- Independent spec/quality approval reran original2 repros and10 targeted queue/ownership repairs successfully
- No actual CLI/account/OS stop/platform/scaling/D20 certification, full suite, CI/PR/package/release; atomic publication pending readback

### Task 16 BLOCKED_SYNC — approval evidence

- Reviewed staged tree `e78e88d36a47b9c974b944685d8e9c019b3f7ba5` is complete and independently approved. GitHub create_tree was rejected by automatic approval review because trusted user-authored authorization for source/doc payload and repository destination was not visible to that reviewer
- No Task16 remote commit/ref update occurred. Parent requested to supply exact transcript evidence; no alternate upload route used. Recovery bundle `/tmp/cc-desk-plan/task-16-blocked-sync.bundle` and staged binary patch `/tmp/cc-desk-plan/task-16-blocked-sync.patch` preserve local work
- Next-task implementation paused until authorized identical publication retry and remote readback succeed

### Verified Task 16 checkpoint — October 1, 2026

- User explicitly approved the pending source/doc upload and continued per-task branch pushes. Identical previously denied create_tree retried once successfully; exact staged/fetched tree `e78e88d36a47b9c974b944685d8e9c019b3f7ba5` and local/remote commit `c0a1c231426b599c30ec52ea2740f44d10334e96` match
-43 required tests/typecheck,290 affected tests plus37 Legacy store tests; independent approval with2 original repros and10 queue/ownership checks passing
- Only local unsent BLOCKED_SYNC/readback ledger annotation remained after adoption; source/index clean. BLOCKED_SYNC resolved; no CI/PR/package/release or actual CLI/platform certification; next Task17

### Task 17 preflight ruling

- Ruling: add minimal optional typed AppConfig fields for GUI theme mode/system, density, sidebar width, supported startup destination and default new CLI because existing Rust typed reserialization drops unknown fields. Preserve old keys/defaults and test serialization compatibility; record unperformed Rust gate if toolchain absent. Fixed ink-blue focus/amber selection remain, no custom accent editor. Default CLI only guides explicit chooser/form selection; startup destination never implicitly launches a CLI. Close behavior exposes only actually supported choices. Cost if wrong: older versions ignore optional new preferences while preserving their existing behavior; no new runtime/launch protocol is introduced.

## Task 17 — Settings shell, General and Appearance

- Real seven-section Settings shell; General language/startup destination/default explicit new CLI and supported close explanation; no fake tray or implicit launch. Future18–20 content remains honestly bounded
- Appearance GUI light/dark/system, standard/compact density and real session sidebar240–360/default288; fixed accents and terminal preferences/host/selection remain independent. Width uses editable draft committed by blur/Enter
- Immediate serialized field saves retain confirmed baselines, current intent and safe feedback; uncertain writes read back without replay and failed readback blocks queued mutation until authoritative recovery
- Optional typed AppConfig fields and real raw-object merge preserve old/future stored keys. Added3 focused Rust compatibility tests; cargo/rustc/rustfmt absent, attempted cargo gate exit127, compilation/formatting NOT RUN
- Review repairs bind hydration/publication to underlying read sequence plus field commit/intent watermarks, not later joining caller epochs; startup migration obeys same uncertainty barrier. Original stale shared-read/migration/typing cases and combined recovery-overlap all repaired
- Final66 exact tests+typecheck;152 affected tests,62 resources/errors and41 confirmations preserved. Manifest/diff/reverse-patch checks pass; earlier61/65 counts superseded
- Independent spec/quality approval reran original3 repros, remaining overlap and5 focused repair/rollback/migration/width tests successfully. Static Rust review found no concrete defect but is not compilation evidence
- No actual CLI/platform/scaling/system-appearance certification, full suite, CI/PR/package/release; atomic publication pending readback

### Verified Task 17 checkpoint

- Remote commit `bb8c2c999370e4988b96d2917bd10f66154bda51`; exact staged/fetched tree and local/remote SHA match; source/worktree clean before readback
-66 required tests/typecheck;152 affected+103 preservation tests; independent approval with original3+overlap1+5 focused checks passing
- Initial tree-only upload stalled without result; original cell disappeared and expected tree read returned404 with branch unchanged. Exact staged-content retry returned matching tree, then normal commit/ref/readback succeeded; no duplicate ref mutation
- Rust compilation/formatting and actual CLI/platform acceptance NOT RUN; final Windows CI route remains pending25. No CI/PR/package/release; next18

### Task 18 preflight ruling

- Ruling: renderer preference applies when a terminal is next created and is clearly labeled; do not recreate a running terminal solely to switch renderer. Live color/font/cursor options update in place using shared preferences. Add only necessary optional typed read DTO fields for already-existing persisted renderer/new preference compatibility; Rust tests remain NOT RUN until real toolchain gate. Cost if wrong: renderer changes need a newly opened terminal, preserving current process/buffer continuity.

## Task 18 — Terminal preferences and theme continuity

- Real Terminal settings and inert non-PTY preview expose themes/font/size/line-height/cursor/blink/renderer. One computed preferences object drives Legacy Claude, Native Claude and Native Codex through Task17 serialized settings writer
- Colors/cursors update xterm options only, no resize/recreate/process/input. Font metrics coalesce one visible fit and defer hidden work with exact lifetime checks; renderer choice frozen at creation, optional Native WebGL safely falls back with same current colors
- Existing16 palettes/IDs retained; old missing theme inference occurs once and later GUI recovery cannot recouple terminal colors. Numeric drafts commit blur/Enter and support rollback/cancel
- Optional Rust read DTO fields/2 compatibility tests added, raw config writer/protocol unchanged; Rust NOT RUN, finalWindows gate pending25
- Final65 exact tests+typecheck;238 affected18 suites. Independent spec/quality approval ran13 focused continuity/migration checks and a new delayed-WebGL/queued-fit-after-unmount test successfully; manifest/diff checks pass
- Actual GPU/fonts/CJK/CLI/PTY/platform/scaling/Native OS-minimize certification unperformed; no fullsuite/CI/PR/package/release. Atomic publication pending readback
