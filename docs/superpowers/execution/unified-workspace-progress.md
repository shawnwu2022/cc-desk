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
- Next task: Task 12 — quick/advanced new-session flow
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
