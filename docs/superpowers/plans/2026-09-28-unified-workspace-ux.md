# CC Desk Unified Workspace UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the parallel Legacy/Native product surfaces with one human-readable project → session workspace, while preserving both runtime adapters, all terminal themes, Native CLI safety boundaries, and the existing user data.

**Architecture:** Add a durable shared presentation catalog, project Legacy Claude and Native CLI sessions into one `UnifiedSession` store, and make that store the only source for project/session navigation. A unified shell selects the correct terminal adapter internally; common UI primitives, structured resource drawers, and a reorganized settings center provide one interaction language across the application. The existing Native runtime, authenticated document bridge, input/output protocols, observer isolation, and fail-closed launch semantics remain unchanged.

**Tech Stack:** Vue 3, Pinia, TypeScript 5, Vitest/jsdom, Tauri 2, Rust, xterm.js, existing Native CLI bridge/projection APIs, optional Playwright visual-fixture tests added as a development-only dependency.

**Spec:** `docs/superpowers/specs/2026-09-28-unified-workspace-ux-design.md`

## Global Constraints

- Implementation branch: `feat/unified-workspace-ux`, based on `7e31cff37b199ba3b10cadf2fe038ed03fed45c2`.
- Primary platform: Windows; macOS and Linux must remain functionally correct.
- Minimum supported window: `1024×640`; verify Windows scaling at `100%`, `125%`, and `150%`.
- Primary navigation contains only Workspace, Projects, and Settings; Skills/Agents/MCP/Plugins/Instructions are contextual project resources.
- Project is the first level; Claude Code and Codex CLI sessions are mixed directly below it.
- The project/session tree is the only tab system; no second Native tab strip or independent Native product page.
- Session rows show status icon, CLI application icon, title, compact age, and trailing controls. They show no status text.
- A row exposes at most one state-dependent primary quick action; all other actions use the shared secondary menu/context menu.
- Compact time formats are exactly: zh `<1m = 刚刚`, en `<1m = now`, then `Nm`, `Nh`, `Nd`, current-year `M/D`, cross-year `YY/M/D`.
- User-facing terminology uses “启动配置”, never “Profile”; Native/Legacy/Revision/Generation/Run ID are diagnostics-only terms.
- GUI theme and terminal theme remain independent. Legacy Claude, Native Claude, and Native Codex share the same global terminal preferences.
- Theme changes must not restart a CLI, clear scrollback, change the current selection, or replay input.
- Native operations continue through the authenticated document bridge; no Native fallback to Legacy `ptySpawn`/`ptyInput`/`ptyKill`.
- Raw argv remains exact `string[]`; no shell splitting.
- Unknown launch state and partial/unknown input writes are never automatically replayed.
- Resource panels stay read-only and must not expose secrets, env values, headers, credential values, arbitrary native paths, or raw transport errors.
- Do not add Provider/API-key management, CLI installers, a new MCP runtime, version bumps, tags, GitHub Releases, or updater publication.
- No new runtime dependency is permitted without a separate justification. Development-only `@vue/test-utils` and `@playwright/test` are permitted only in the tasks that own component/visual verification.
- Development uses targeted local tests and commits. Do not open a PR until the branch is functionally and visually frozen; then trigger one ordinary CI and one required Windows package run.

## Review Focus

1. **Mixed runtime identity:** a Legacy and Native session in the same normalized project path, including duplicate native/legacy session IDs, must not collide or disappear. Task 4 owns the deduplication tests.
2. **Stale asynchronous completion:** launch, stop, recovery, resource, and save results finishing after session switch/restart/close must not mutate the new active context. Tasks 5, 11, and 16 own exact-attempt tests.
3. **Layout under real content:** 200-character titles, 80-character project names, English/Chinese text, and 150% scaling must preserve status/app/time/menu columns without horizontal page scrolling. Tasks 8, 10, 22, and 23 own these checks.
4. **Partial availability:** missing or unauthenticated Claude must not block Codex, and missing Codex must not block Claude. Tasks 12 and 16 own availability/error tests.
5. **Terminal continuity:** changing GUI layout, drawer width, terminal theme, font, or active session must preserve process ownership, scrollback, and input/output identity. Tasks 11, 18, and 24 own continuity tests.

## File and Responsibility Map

### New domain and state files

- `src/types/unifiedSession.ts` — canonical user-facing session/project types and adapter contracts.
- `src/stores/projectsState.ts` — the only frontend writer for `projects.json` view/catalog state.
- `src/session/adapters/legacyClaudeAdapter.ts` — Legacy store/history projection and lifecycle actions.
- `src/session/adapters/nativeCliAdapter.ts` — Native tab/history projection and lifecycle actions.
- `src/stores/unifiedSessions.ts` — merged project/session catalog, active selection, lifecycle facade.
- `src/stores/shell.ts` — Workspace/Projects/Settings navigation and context-drawer state.
- `src/stores/notifications.ts` — toast/inline/global feedback queue without payload logging.
- `src/utils/sessionPresentation.ts` — visual state, single quick action, labels, stable catalog keys.
- `src/utils/relativeTime.ts` — compact age formatting and shared refresh cadence.
- `src/utils/userError.ts` — safe code → user message/action mapping.

### New common UI files

- `src/components/ui/AppButton.vue`
- `src/components/ui/IconButton.vue`
- `src/components/ui/AppInput.vue`
- `src/components/ui/AppSelect.vue`
- `src/components/ui/AppTooltip.vue`
- `src/components/ui/AppMenu.vue`
- `src/components/ui/AppDialog.vue`
- `src/components/ui/AppDrawer.vue`
- `src/components/ui/AppToastHost.vue`
- `src/components/ui/InlineNotice.vue`
- `src/components/ui/EmptyState.vue`
- `src/components/ui/LoadingState.vue`
- `src/components/ui/ErrorDetails.vue`

### New workspace files

- `src/components/shell/AppShell.vue` — fixed application shell and responsive columns.
- `src/components/shell/PrimaryNav.vue` — Workspace/Projects/Settings navigation only.
- `src/components/workspace/WorkspaceView.vue` — project/session tree + terminal + context drawer.
- `src/components/workspace/UnifiedTerminalHost.vue` — Legacy/Native terminal selection and visibility.
- `src/components/workspace/WorkspaceHeader.vue` — current project/session and context actions.
- `src/components/workspace/ProjectResourcesDrawer.vue` — scoped structured read-only resources.
- `src/components/sessions/SessionStatusIcon.vue`
- `src/components/sessions/CliAppIcon.vue`
- `src/components/sessions/SessionOverflowMenu.vue`
- `src/components/sessions/NewSessionMenu.vue`
- `src/components/sessions/NewSessionDialog.vue`
- `src/components/sessions/ResumeSessionDialog.vue`
- `src/components/sessions/ArchivedSessionsDrawer.vue`
- `src/components/projects/ProjectsView.vue`

### Existing files with focused modifications

- `src/App.vue`, `src/components/TerminalView.vue`, `src/components/NativeCliTerminal.vue`
- `src/components/IconBar.vue`, `src/components/sidebar/SidebarPanel.vue`
- `src/components/sessions/{SessionsPanel,ProjectNode,SessionList,SessionItem}.vue`
- `src/stores/{session,nativeTabs,nativeWorkbench,cliProfiles,cliWorkspace,sidebar,app,config}.ts`
- `src/types/{app,session,profile}.ts`, `src/api/{tauri,cli}.ts`
- `src/components/settings/SettingsView.vue` and settings sections
- `src/config/terminalThemes.ts`, `src/styles/global.css`
- `src/i18n/locales/{en,zh}.ts`
- `src-tauri/src/store.rs`, `src-tauri/src/tests/store.rs`
- `tests/productBoundary.test.ts` and focused unit/component/integration tests.

---

### Task 1: Canonical Session Presentation Types and Pure Helpers

**Files:**
- Create: `src/types/unifiedSession.ts`
- Create: `src/utils/relativeTime.ts`
- Create: `src/utils/sessionPresentation.ts`
- Create: `src/utils/userError.ts`
- Test: `tests/utils/relativeTime.test.ts`
- Test: `tests/utils/sessionPresentation.test.ts`
- Test: `tests/utils/userError.test.ts`

**Interfaces:**
- Produces `UnifiedSession`, `UnifiedProjectGroup`, `SessionVisualState`, `SessionPrimaryAction`, `CreateUnifiedSessionInput`, `ResumeUnifiedSessionInput`, and `SessionAdapter`.
- Produces `makeSessionCatalogKey(...)`, `deriveSessionVisualState(session)`, `selectSessionPrimaryAction(session)`, `formatRelativeActivity(timestamp, now, locale)`, and `mapSafeUserError(code, context)`.

- [ ] **Step 1: Write failing helper tests**

```ts
expect(formatRelativeActivity(now - 30_000, now, 'zh')).toBe('刚刚')
expect(formatRelativeActivity(now - 6 * 60_000, now, 'en')).toBe('6m')
expect(formatRelativeActivity(now - 3 * 3_600_000, now, 'zh')).toBe('3h')
expect(formatRelativeActivity(now - 11 * 86_400_000, now, 'zh')).toBe('11d')
expect(deriveSessionVisualState(runningNeedsUser)).toBe('needs-user')
expect(selectSessionPrimaryAction(failedSession)).toBe('retry')
expect(selectSessionPrimaryAction(needsUserSession)).toBe(null)
expect(mapSafeUserError('REVISION_CONFLICT', 'workspace').messageKey)
  .toBe('errorRevisionConflict')
```

- [ ] **Step 2: Verify tests fail**

Run: `npm test -- tests/utils/relativeTime.test.ts tests/utils/sessionPresentation.test.ts tests/utils/userError.test.ts`

Expected: FAIL because modules do not exist.

- [ ] **Step 3: Implement exact domain types and pure functions**

Use these pinned unions:

```ts
export type SessionRuntimeKind = 'legacy-claude' | 'native-cli'
export type SessionProcessState = 'starting' | 'running' | 'unknown' | 'stopped' | 'failed'
export type SessionAttentionState = 'none' | 'needs-user'
export type SessionVisualState = 'starting' | 'running' | 'needs-user' | 'confirming' | 'ended' | 'failed'
export type SessionPrimaryAction = 'cancel-start' | 'stop' | 'confirm-status' | 'resume' | 'retry' | 'restore-archive' | 'save-rename'
```

`makeSessionCatalogKey` must include runtime, CLI, normalized project path, and native/adapter identity; it must reject NUL and empty identity fields.

- [ ] **Step 4: Run targeted tests and typecheck**

Run: `npm test -- tests/utils/relativeTime.test.ts tests/utils/sessionPresentation.test.ts tests/utils/userError.test.ts && npm run typecheck`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/types/unifiedSession.ts src/utils/relativeTime.ts src/utils/sessionPresentation.ts src/utils/userError.ts tests/utils
git commit -m "feat: define unified session presentation model"
```

### Task 2: Durable Shared Project and Session Catalog State

**Files:**
- Create: `src/stores/projectsState.ts`
- Modify: `src/types/app.ts`
- Modify: `src/stores/session.ts`
- Modify: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/tests/store.rs`
- Test: `tests/stores/projectsState.test.ts`

**Interfaces:**
- Produces `useProjectsStateStore()` with `load()`, `reload()`, `mutate()`, `pinProject()`, `unpinProject()`, `setProjectDisplayName()`, `archiveSession()`, `restoreSession()`, `upsertSessionRecord()`, `removeSessionRecord()`, and `setLaunchPreference()`.
- Persists optional JSON fields `sessionRecords` and `launchPreferences` alongside existing `pinnedProjects`, `archivedSessions`, and `displayNames`.
- Existing `useSessionStore()` temporarily re-exports compatibility getters/actions but no longer writes `projects.json` itself.

- [ ] **Step 1: Add failing Rust compatibility tests**

Add tests named:

```text
ProjectsState_OldFileDefaultsNewFields_001
ProjectsState_RoundTripsSessionRecords_002
ProjectsState_SkipsMalformedSessionRecord_003
ProjectsState_PreservesUnknownTopLevelDataOnlyWhereAlreadySupported_004
```

Assertions must cover old files, optional fields, title length ≤200 Unicode scalar values, no NUL, and a maximum of 10,000 session records.

- [ ] **Step 2: Add failing Pinia store tests**

Verify serialized mutation ordering, full-state round-trip, reload after conflict, and that a Legacy archive mutation does not erase Native session records.

- [ ] **Step 3: Run failing tests**

Run:

```bash
npm test -- tests/stores/projectsState.test.ts
cd src-tauri && cargo test --locked ProjectsState_ -- --nocapture
```

Expected: FAIL because the new fields/store do not exist.

- [ ] **Step 4: Implement tolerant Rust schema and single frontend writer**

`ProjectsState` adds:

```rust
#[serde(rename = "sessionRecords", default, deserialize_with = "deserialize_session_records")]
pub session_records: HashMap<String, SessionUiRecord>,
#[serde(rename = "launchPreferences", default, deserialize_with = "deserialize_launch_preferences")]
pub launch_preferences: HashMap<String, ProjectLaunchPreference>,
```

Reuse the existing locked read/atomic write path. `projectsState.ts` serializes all fields on every mutation and queues writes; a failed write leaves in-memory state unchanged. No automatic replay after an ambiguous commit.

- [ ] **Step 5: Migrate `session.ts` to the shared store without changing public behavior**

Replace its private `pinnedProjects`, `archivedSessions`, `displayNames`, load gate, and write lock with computed/delegated access to `useProjectsStateStore()` so existing components/tests remain green.

- [ ] **Step 6: Verify**

Run:

```bash
npm test -- tests/stores/projectsState.test.ts tests/stores/session.test.ts tests/stores/sessionTree.test.ts
cd src-tauri && cargo test --locked ProjectsState_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/stores/projectsState.ts src/types/app.ts src/stores/session.ts src-tauri/src/store.rs src-tauri/src/tests/store.rs tests/stores/projectsState.test.ts
git commit -m "feat: persist unified session catalog state"
```

### Task 3: Legacy Claude Session Adapter

**Files:**
- Create: `src/session/adapters/legacyClaudeAdapter.ts`
- Test: `tests/stores/legacyClaudeAdapter.test.ts`
- Modify: `src/stores/session.ts` only for missing typed lifecycle hooks required by the adapter.

**Interfaces:**
- Consumes `SessionAdapter` and shared catalog from Tasks 1–2.
- Produces `createLegacyClaudeAdapter(deps): SessionAdapter`.
- Active session IDs are `legacy-tab:<tabId>`; history IDs are `legacy-history:<normalizedProject>:<sessionId>`.

- [ ] **Step 1: Write failing projection tests**

Cover active tabs, unclaimed history, pending → `needs-user`, stopped resumable history, archived filtering, normalized Windows paths, and no duplicate history when a tab claims the same session ID.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/stores/legacyClaudeAdapter.test.ts`

- [ ] **Step 3: Implement projection and action delegation**

Adapter actions call the existing store’s exact typed methods for create/resume/activate/stop/restart/close/rename. Archive/restore updates the shared catalog only after the runtime action, if any, succeeds.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/stores/legacyClaudeAdapter.test.ts tests/stores/session.test.ts`

- [ ] **Step 5: Commit**

```bash
git add src/session/adapters/legacyClaudeAdapter.ts src/stores/session.ts tests/stores/legacyClaudeAdapter.test.ts
git commit -m "feat: adapt legacy Claude sessions to unified model"
```

### Task 4: Native CLI Session Adapter and Native History Cache

**Files:**
- Create: `src/session/adapters/nativeCliAdapter.ts`
- Create: `src/stores/nativeHistory.ts`
- Modify: `src/stores/nativeTabs.ts`
- Modify: `src/stores/cliWorkspace.ts`
- Test: `tests/stores/nativeCliAdapter.test.ts`
- Test: `tests/stores/nativeHistory.test.ts`

**Interfaces:**
- Produces `createNativeCliAdapter(deps): SessionAdapter`.
- Produces `useNativeHistoryStore().load({ cli, profileId, profileRevision, projectId, projectPath, force })` with per-context caches and exact request ownership.
- Native active IDs are `native-tab:<tabId>`; history IDs use backend-projected `sessionKey` through `makeSessionCatalogKey`.

- [ ] **Step 1: Write failing tests for mixed identity and stale reads**

Cover Claude/Codex history in one project, duplicate `nativeSessionId` across different source roots, a history record claimed by an active tab, old request completion after profile/session switch, and a malformed projection that must fail closed.

- [ ] **Step 2: Run failing tests**

Run: `npm test -- tests/stores/nativeCliAdapter.test.ts tests/stores/nativeHistory.test.ts`

- [ ] **Step 3: Add presentation metadata to `NativeCliTab`**

Add `title`, `createdAt`, and `lastActivityAt`. Do not add secrets or resolved env/config values. Update timestamps only for actual launch/status/input/output/attention events, not component rerenders.

- [ ] **Step 4: Implement a per-context Native history cache**

Use the authenticated projection client and the exact selected profile revision/project ID. Cache keys include CLI, profile ID/revision, project ID, and normalized project path. Owner tokens discard stale completion.

- [ ] **Step 5: Implement adapter lifecycle actions**

Create/resume/restart use existing Native tab/run semantics; unknown state rejects restart; same projected native session already open activates it instead of creating a duplicate.

- [ ] **Step 6: Verify**

Run:

```bash
npm test -- tests/stores/nativeCliAdapter.test.ts tests/stores/nativeHistory.test.ts tests/stores/nativeTabs.test.ts tests/native-cli/nativeAttemptIsolation.test.ts
npm run typecheck
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/session/adapters/nativeCliAdapter.ts src/stores/nativeHistory.ts src/stores/nativeTabs.ts src/stores/cliWorkspace.ts tests/stores/nativeCliAdapter.test.ts tests/stores/nativeHistory.test.ts
git commit -m "feat: adapt native CLI sessions to unified model"
```

### Task 5: Unified Session Store and Lifecycle Facade

**Files:**
- Create: `src/stores/unifiedSessions.ts`
- Test: `tests/stores/unifiedSessions.test.ts`
- Modify: `src/stores/attention.ts` only to expose a typed lookup/subscription needed for projection.

**Interfaces:**
- Consumes both adapters and `useProjectsStateStore()`.
- Produces `sessions`, `projectGroups`, `activeSessionId`, `activeSession`, `initialize()`, `activateSession()`, `createSession()`, `resumeSession()`, `stopSession()`, `restartSession()`, `closeSession()`, `renameSession()`, `archiveSession()`, and `restoreArchivedSession()`.

- [ ] **Step 1: Write failing store tests**

Test merged ordering, project grouping, CLI mixing, single active selection, same-session deduplication, attention override, archive visibility, close-vs-history semantics, and stale action completion after explicit restart/close.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/stores/unifiedSessions.test.ts`

- [ ] **Step 3: Implement merged read model**

Selection authority lives in this store. Activating a session first updates the exact adapter, then publishes `activeSessionId`; adapter failure leaves the old selection intact. Project keys use normalized paths.

- [ ] **Step 4: Implement serialized lifecycle mutations**

Maintain a per-session action tail so stop/restart/close/archive cannot race. Capture `{ unifiedId, adapter identity, runId/generation where available }` before async work; ignore stale completion.

- [ ] **Step 5: Verify**

Run: `npm test -- tests/stores/unifiedSessions.test.ts tests/stores/attention.test.ts && npm run typecheck`

- [ ] **Step 6: Commit**

```bash
git add src/stores/unifiedSessions.ts src/stores/attention.ts tests/stores/unifiedSessions.test.ts
git commit -m "feat: merge legacy and native sessions"
```

### Task 6: Common UI Primitives and Notification Store

**Files:**
- Create: `src/stores/notifications.ts`
- Create: all files under `src/components/ui/` listed in the file map.
- Modify: `src/styles/global.css`
- Modify: `package.json`, `package-lock.json` to add `@vue/test-utils` as a dev dependency.
- Test: `tests/components/uiPrimitives.test.ts`
- Test: `tests/stores/notifications.test.ts`

**Interfaces:**
- Produces consistent button/input/menu/dialog/drawer/tooltip/toast/notice/empty/loading/error-detail primitives.
- `AppMenu` and `AppDialog` expose keyboard/focus contracts; `useNotificationsStore().pushToast({ kind, messageKey, dedupeKey? })` never stores arbitrary Error messages.

- [ ] **Step 1: Add failing interaction tests**

Test Escape close, arrow/Enter menu navigation, dialog focus return, danger button not auto-focused, tooltip `aria-describedby`, toast dedupe/max-three, and no `transition: all`.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/uiPrimitives.test.ts tests/stores/notifications.test.ts`

- [ ] **Step 3: Implement primitives using existing design tokens**

Heights are 28/32/36px; focus is 2px ink-blue; selection remains amber; static surfaces have no shadow; menus/dialogs use existing shadow tokens.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/uiPrimitives.test.ts tests/stores/notifications.test.ts tests/designTokens.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add package.json package-lock.json src/components/ui src/stores/notifications.ts src/styles/global.css tests/components tests/stores/notifications.test.ts
git commit -m "feat: add shared interaction primitives"
```

### Task 7: Session Status and CLI Application Icons

**Files:**
- Create: `src/components/sessions/SessionStatusIcon.vue`
- Create: `src/components/sessions/CliAppIcon.vue`
- Create: licensed/self-owned SVG assets under `src/assets/icons/cli/` and `src/assets/icons/session-status/`
- Test: `tests/components/sessionIcons.test.ts`

**Interfaces:**
- `SessionStatusIcon` consumes `SessionVisualState` and renders no visible state text.
- `CliAppIcon` consumes `'claude' | 'codex'`; fallback text is `CC`/`CX` only if SVG loading fails.

- [ ] **Step 1: Write failing tests**

Assert six distinct SVG shape identifiers, tooltip/aria labels, no visible status label nodes, reduced-motion styles, CLI tooltip names, and fallback behavior.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/sessionIcons.test.ts`

- [ ] **Step 3: Implement icons**

Use self-owned neutral marks unless repository-cleared official assets are available. Application icon color never changes with session status.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/sessionIcons.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/sessions/SessionStatusIcon.vue src/components/sessions/CliAppIcon.vue src/assets/icons/cli src/assets/icons/session-status tests/components/sessionIcons.test.ts
git commit -m "feat: add accessible session and CLI icons"
```

### Task 8: Unified Session Row and Secondary Menu

**Files:**
- Modify: `src/components/sessions/SessionItem.vue`
- Modify: `src/components/sessions/SessionList.vue`
- Create: `src/components/sessions/SessionOverflowMenu.vue`
- Test: `tests/components/sessionItem.test.ts`
- Modify: `src/i18n/locales/en.ts`, `src/i18n/locales/zh.ts`

**Interfaces:**
- Session row consumes one `UnifiedSession` plus `selected`, `primaryAction`, and menu-action visibility.
- Emits `activate`, `primary-action`, and typed `menu-action`; rename mode emits `rename-commit`/`rename-cancel`.

- [ ] **Step 1: Write failing row tests**

Assert exact grid columns `16px 18px minmax(0, 1fr) 38px 20px`, single-line ellipsis, default compact time, at most one quick action on hover/focus, unchanged title geometry, F2 rename, and complete menu actions for running/stopped/failed/archived states.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/sessionItem.test.ts`

- [ ] **Step 3: Implement the row and menu**

Do not render state words. Context menu and overflow menu share the same action-definition array from `sessionPresentation.ts`.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/sessionItem.test.ts tests/i18n/translations.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/sessions/SessionItem.vue src/components/sessions/SessionList.vue src/components/sessions/SessionOverflowMenu.vue src/i18n/locales tests/components/sessionItem.test.ts
git commit -m "feat: unify session row interactions"
```

### Task 9: Project Tree and Archived Session Drawer

**Files:**
- Modify: `src/components/sessions/ProjectNode.vue`
- Modify: `src/components/sessions/SessionsPanel.vue`
- Create: `src/components/sessions/ArchivedSessionsDrawer.vue`
- Test: `tests/components/projectSessionTree.test.ts`

**Interfaces:**
- Consumes `UnifiedProjectGroup[]` from Task 5.
- Project row’s only inline high-frequency action is `new-session`; menu owns pin/rename/archive-view/open/remove operations.

- [ ] **Step 1: Write failing tests**

Cover Claude/Codex mixing, project collapse attention marker, single `+` action, archived drawer restoration, running-session “stop and archive” confirmation request, search across project/session names, and 80-character names without column loss.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/projectSessionTree.test.ts`

- [ ] **Step 3: Replace Legacy-only props/events with unified typed events**

Remove the footer’s global Claude-only skip-permissions/custom-args controls; they move to the new-session dialog/settings.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/projectSessionTree.test.ts tests/sidebarKeyboardHandlers.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/sessions/ProjectNode.vue src/components/sessions/SessionsPanel.vue src/components/sessions/ArchivedSessionsDrawer.vue tests/components/projectSessionTree.test.ts
git commit -m "feat: show unified sessions in project tree"
```

### Task 10: Unified Application Shell and Primary Navigation

**Files:**
- Create: `src/stores/shell.ts`
- Create: `src/components/shell/AppShell.vue`
- Create: `src/components/shell/PrimaryNav.vue`
- Create: `src/components/workspace/WorkspaceView.vue`
- Create: `src/components/workspace/WorkspaceHeader.vue`
- Modify: `src/App.vue`
- Modify: `src/components/IconBar.vue`
- Modify: `src/components/sidebar/SidebarPanel.vue`
- Test: `tests/components/appShell.test.ts`

**Interfaces:**
- `useShellStore()` exposes `section: 'workspace' | 'projects' | 'settings'`, sidebar/drawer visibility and width, and responsive mode.
- `AppShell` owns the four columns; content views never implement their own global shell.

- [ ] **Step 1: Write failing shell tests**

Assert only three primary destinations, `1024×640` minimum-safe layout contract, overlay drawer below 1180px, collapsible session panel below 900px, no global horizontal overflow, and 200-character context titles ellipsize.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/appShell.test.ts`

- [ ] **Step 3: Implement shell and migrate `App.vue` routing state**

Keep old views internally reachable behind a temporary development flag only until Task 21. Do not expose a Native CLI top-level button.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/appShell.test.ts tests/productBoundary.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/stores/shell.ts src/components/shell src/components/workspace/WorkspaceView.vue src/components/workspace/WorkspaceHeader.vue src/App.vue src/components/IconBar.vue src/components/sidebar/SidebarPanel.vue tests/components/appShell.test.ts
git commit -m "feat: introduce unified application shell"
```

### Task 11: Unified Terminal Host

**Files:**
- Create: `src/components/workspace/UnifiedTerminalHost.vue`
- Modify: `src/components/TerminalView.vue`
- Modify: `src/components/NativeCliTerminal.vue`
- Modify: `src/components/XTermTerminal.vue` only for explicit visibility/fit hooks if absent.
- Test: `tests/components/unifiedTerminalHost.test.ts`
- Test: `tests/native-cli/unifiedTerminalIdentity.test.ts`

**Interfaces:**
- Host consumes `activeSessionId` and all open sessions.
- Child terminals expose `focus()`, `fitVisible()`, `stop()`, `recover()`, and exact-attempt-safe lifecycle hooks.

- [ ] **Step 1: Write failing terminal continuity tests**

Test switching visibility without remount, background output retention, exact Native attempt guards, legacy/native isolation, drawer/sidebar resize fit behavior, and stale stop/recover completion after restart.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/unifiedTerminalHost.test.ts tests/native-cli/unifiedTerminalIdentity.test.ts`

- [ ] **Step 3: Implement host without changing Native transport APIs**

Keep active/open terminals mounted. Only the visible terminal fits immediately; hidden terminals mark `needsFit` and fit on activation. No adapter invokes another adapter’s PTY API.

- [ ] **Step 4: Verify**

Run:

```bash
npm test -- tests/components/unifiedTerminalHost.test.ts tests/native-cli/unifiedTerminalIdentity.test.ts tests/native-cli/nativeTerminalBinding.test.ts tests/native-cli/nativeAttemptIsolation.test.ts
npm run typecheck
```

- [ ] **Step 5: Commit**

```bash
git add src/components/workspace/UnifiedTerminalHost.vue src/components/TerminalView.vue src/components/NativeCliTerminal.vue src/components/XTermTerminal.vue tests/components/unifiedTerminalHost.test.ts tests/native-cli/unifiedTerminalIdentity.test.ts
git commit -m "feat: host legacy and native terminals together"
```

### Task 12: Quick New Session Menu and Advanced Dialog

**Files:**
- Create: `src/components/sessions/NewSessionMenu.vue`
- Create: `src/components/sessions/NewSessionDialog.vue`
- Create: `src/stores/newSessionDraft.ts`
- Modify: `src/components/sessions/ProjectNode.vue`
- Modify: `src/stores/unifiedSessions.ts`
- Test: `tests/components/newSessionFlow.test.ts`
- Test: `tests/stores/newSessionDraft.test.ts`

**Interfaces:**
- Quick menu offers Claude Code, Codex CLI, Restore, More Options.
- Draft store produces `CreateUnifiedSessionInput` with exact argv arrays and chosen/default launch configuration.

- [ ] **Step 1: Write failing tests**

Cover two-click normal create, project+CLI launch preference priority, one unavailable CLI while the other remains enabled, immediate starting placeholder, per-line argv conversion, raw JSON opt-in, and no shell splitting.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/newSessionFlow.test.ts tests/stores/newSessionDraft.test.ts`

- [ ] **Step 3: Implement quick and advanced flows**

Normal create must not display profile IDs, revision, session ID, or raw argv. Advanced fields are vertically grouped as Basic → More Options → Developer Options.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/newSessionFlow.test.ts tests/stores/newSessionDraft.test.ts tests/utils/nativeRawArgv.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/sessions/NewSessionMenu.vue src/components/sessions/NewSessionDialog.vue src/stores/newSessionDraft.ts src/components/sessions/ProjectNode.vue src/stores/unifiedSessions.ts tests/components/newSessionFlow.test.ts tests/stores/newSessionDraft.test.ts
git commit -m "feat: simplify new session creation"
```

### Task 13: Unified Resume and History Search

**Files:**
- Create: `src/components/sessions/ResumeSessionDialog.vue`
- Modify: `src/stores/unifiedSessions.ts`
- Modify: `src/stores/nativeHistory.ts`
- Modify: `src/stores/session.ts`
- Test: `tests/components/resumeSessionDialog.test.ts`
- Test: `tests/stores/unifiedResume.test.ts`

**Interfaces:**
- Dialog searches title and session ID, filters by CLI, defaults to current project, and can switch to all projects.

- [ ] **Step 1: Write failing tests**

Cover active-session activation instead of duplicate resume, Native `sessionKey` identity, Legacy claimed-session filtering, missing history removal, current/all project scopes, and stale search result rejection.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/resumeSessionDialog.test.ts tests/stores/unifiedResume.test.ts`

- [ ] **Step 3: Implement search/resume**

The same native session cannot be open twice in the same source identity. Missing sessions surface a natural-language notice and an explicit “remove record” action.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/resumeSessionDialog.test.ts tests/stores/unifiedResume.test.ts tests/native-cli/nativeProjectionStore.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/sessions/ResumeSessionDialog.vue src/stores/unifiedSessions.ts src/stores/nativeHistory.ts src/stores/session.ts tests/components/resumeSessionDialog.test.ts tests/stores/unifiedResume.test.ts
git commit -m "feat: unify session restore workflows"
```

### Task 14: Project Management Page

**Files:**
- Create: `src/components/projects/ProjectsView.vue`
- Create: `src/components/projects/ProjectRow.vue`
- Modify: `src/stores/app.ts`
- Modify: `src/stores/projectsState.ts`
- Modify: `src/stores/cliWorkspace.ts`
- Test: `tests/components/projectsView.test.ts`
- Test: `tests/stores/projectRegistrationFlow.test.ts`

**Interfaces:**
- Project rows expose open/new session/pin/rename/open folder/hide/remove actions.
- `ensureNativeProjectRegistration({ path, cli, profile })` returns the existing or newly registered project ID without exposing a second user action.

- [ ] **Step 1: Write failing tests**

Cover one-time add, duplicate normalized Windows path adoption, Native registration conflict reload without replay, safe remove wording, long path middle ellipsis, and 50-project search/sort.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/projectsView.test.ts tests/stores/projectRegistrationFlow.test.ts`

- [ ] **Step 3: Implement compact project list and automatic registration**

Removing a project only removes CC Desk records; it never deletes filesystem content. Native registration is internal and profile-bound.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/projectsView.test.ts tests/stores/projectRegistrationFlow.test.ts tests/stores/nativeProjectRegistration.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/projects src/stores/app.ts src/stores/projectsState.ts src/stores/cliWorkspace.ts tests/components/projectsView.test.ts tests/stores/projectRegistrationFlow.test.ts
git commit -m "feat: add unified project management"
```

### Task 15: Structured Project Resources Drawer

**Files:**
- Create: `src/components/workspace/ProjectResourcesDrawer.vue`
- Create: `src/components/resources/{InstructionsView,SettingsView,McpList,SkillList,AgentList,PluginList}.vue`
- Create: `src/stores/projectResources.ts`
- Modify: `src/stores/nativeProjection.ts`
- Test: `tests/components/projectResourcesDrawer.test.ts`
- Test: `tests/stores/projectResources.test.ts`

**Interfaces:**
- Resource store loads by `{ sessionId, projectId, cli, profile/run identity, kind }` and exposes typed items, loading, stale, unavailable, and safe error state.

- [ ] **Step 1: Write failing tests**

Cover structured rendering (no `JSON.stringify`), safe config allowlist, empty states, old-request completion after session switch, retained content during refresh, and secret-looking values never rendered.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/projectResourcesDrawer.test.ts tests/stores/projectResources.test.ts`

- [ ] **Step 3: Implement exact-session resource ownership**

Use run scope for active Native sessions when available; otherwise use exact configured profile/project scope. Legacy resources continue through existing read-only stores but map into the same DTOs.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/projectResourcesDrawer.test.ts tests/stores/projectResources.test.ts tests/native-cli/projectionScope.test.ts tests/productBoundary.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/workspace/ProjectResourcesDrawer.vue src/components/resources src/stores/projectResources.ts src/stores/nativeProjection.ts tests/components/projectResourcesDrawer.test.ts tests/stores/projectResources.test.ts
git commit -m "feat: render structured project resources"
```

### Task 16: Human-Readable Errors, Confirmations, and Action Feedback

**Files:**
- Create: `src/components/dialogs/SessionConfirmDialog.vue`
- Create: `src/components/dialogs/ProjectConfirmDialog.vue`
- Modify: `src/stores/unifiedSessions.ts`
- Modify: `src/stores/projectsState.ts`
- Modify: `src/utils/userError.ts`
- Modify: `src/App.vue`
- Test: `tests/components/interactionFeedback.test.ts`
- Test: `tests/stores/staleActionFeedback.test.ts`

**Interfaces:**
- Dialog requests are typed discriminated unions; arbitrary native error strings are never stored or rendered.
- `REVISION_CONFLICT` reloads latest state and asks the user to retry; no side-effecting mutation is automatically replayed.

- [ ] **Step 1: Write failing tests**

Cover close-running, stop-and-archive, project removal, launch-config deletion, restart-unknown, revision conflict, one-CLI failure, and stale success/error completion after selection change.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/interactionFeedback.test.ts tests/stores/staleActionFeedback.test.ts`

- [ ] **Step 3: Implement typed confirmation and feedback flows**

Use inline notice for local retryable errors, banner for partial CLI availability, toast for completed low-risk actions, and full-page error only when the whole workspace cannot load.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/interactionFeedback.test.ts tests/stores/staleActionFeedback.test.ts tests/utils/nativeErrorCode.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/dialogs src/stores/unifiedSessions.ts src/stores/projectsState.ts src/utils/userError.ts src/App.vue tests/components/interactionFeedback.test.ts tests/stores/staleActionFeedback.test.ts
git commit -m "feat: standardize safe interaction feedback"
```

### Task 17: Settings Shell, General, and Appearance Sections

**Files:**
- Modify: `src/components/settings/SettingsView.vue`
- Create: `src/components/settings/sections/GeneralSection.vue`
- Modify: `src/components/settings/sections/AppearanceSection.vue`
- Modify: `src/stores/sidebar.ts`
- Modify: `src/stores/app.ts`
- Test: `tests/components/settingsShell.test.ts`

**Interfaces:**
- Settings sections are exactly `general | appearance | terminal | launch-configurations | shortcuts | update | about`.
- Simple settings save immediately and roll back on persistence failure.

- [ ] **Step 1: Write failing tests**

Assert seven sections, no fake unsupported tray option, independent GUI/terminal fields, 150% layout contract, immediate save rollback, and sidebar width 240–360px with 288px default.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/settingsShell.test.ts`

- [ ] **Step 3: Implement settings shell and sections**

Merge the currently relevant startup behavior into General. Do not delete old stored keys; provide compatibility mappings.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/settingsShell.test.ts tests/stores/app.test.ts tests/i18n/translations.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/settings/SettingsView.vue src/components/settings/sections/GeneralSection.vue src/components/settings/sections/AppearanceSection.vue src/stores/sidebar.ts src/stores/app.ts tests/components/settingsShell.test.ts
git commit -m "feat: unify general and appearance settings"
```

### Task 18: Terminal Settings and Theme Continuity

**Files:**
- Create: `src/components/settings/sections/TerminalSection.vue`
- Create: `src/components/settings/TerminalThemePreview.vue`
- Modify: `src/config/terminalThemes.ts`
- Modify: `src/stores/app.ts`
- Modify: `src/components/XTermTerminal.vue`
- Modify: `src/components/NativeCliTerminal.vue`
- Test: `tests/components/terminalSettings.test.ts`
- Test: `tests/config/terminalThemes.test.ts`
- Test: `tests/components/terminalThemeContinuity.test.ts`

**Interfaces:**
- Terminal preferences include theme, font family, font size, line height, cursor style, cursor blink, and renderer preference.
- Both terminal implementations consume the same computed preferences object.

- [ ] **Step 1: Write failing tests**

Cover four GUI×terminal theme combinations, old theme ID migration, colors-only update without resize/recreate, font update with one visible fit, hidden terminal deferred fit, and WebGL fallback color equality.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/terminalSettings.test.ts tests/config/terminalThemes.test.ts tests/components/terminalThemeContinuity.test.ts`

- [ ] **Step 3: Implement terminal settings and preview**

Preview is not connected to a real PTY. Theme/color updates modify xterm options only; font metrics schedule fit according to visibility.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/terminalSettings.test.ts tests/config/terminalThemes.test.ts tests/components/terminalThemeContinuity.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/settings/sections/TerminalSection.vue src/components/settings/TerminalThemePreview.vue src/config/terminalThemes.ts src/stores/app.ts src/components/XTermTerminal.vue src/components/NativeCliTerminal.vue tests/components/terminalSettings.test.ts tests/config/terminalThemes.test.ts tests/components/terminalThemeContinuity.test.ts
git commit -m "feat: preserve and improve terminal theme settings"
```

### Task 19: Launch Configuration Settings

**Files:**
- Create: `src/components/settings/sections/LaunchConfigurationsSection.vue`
- Create: `src/components/settings/LaunchConfigurationEditor.vue`
- Modify: `src/stores/cliProfiles.ts`
- Modify: `src/types/profile.ts`
- Test: `tests/components/launchConfigurations.test.ts`
- Test: `tests/stores/cliProfiles.test.ts`

**Interfaces:**
- UI uses “启动配置”; rows group by Claude Code/Codex CLI, expose one Edit quick action, and secondary menu actions.
- Editor maps to existing `CliProfile` patches with expected revision and no auto-retry.

- [ ] **Step 1: Write failing tests**

Cover grouping, default marker, copy/rename/delete, revision conflict reload, sensitive env display as set/unset/inherit, per-line argv, and running sessions unaffected by deleting a configuration.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/launchConfigurations.test.ts tests/stores/cliProfiles.test.ts`

- [ ] **Step 3: Implement settings and patch flows**

Complex edits use explicit Save/Cancel. A deleted default selects another existing configuration or creates the safe default; no running process is stopped.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/launchConfigurations.test.ts tests/stores/cliProfiles.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/settings/sections/LaunchConfigurationsSection.vue src/components/settings/LaunchConfigurationEditor.vue src/stores/cliProfiles.ts src/types/profile.ts tests/components/launchConfigurations.test.ts tests/stores/cliProfiles.test.ts
git commit -m "feat: add human-readable launch configuration settings"
```

### Task 20: Shortcuts, Update, and About Consistency

**Files:**
- Modify: `src/components/settings/sections/ShortcutsSection.vue`
- Modify: `src/components/settings/sections/UpdateSection.vue`
- Modify: `src/components/settings/sections/AboutSection.vue`
- Modify: `src/composables/useAppShortcuts.ts`
- Test: `tests/components/remainingSettings.test.ts`
- Test: `tests/composables/appShortcuts.test.ts`

**Interfaces:**
- Shortcut capture rejects conflicts unless user explicitly replaces.
- Update view distinguishes stable/candidate/test-only and never promotes test package artifacts.
- Diagnostic copy returns a redacted structured summary.

- [ ] **Step 1: Write failing tests**

Cover Ctrl/Cmd+N/W/P/, and F2, conflict prompt, per-item/all reset, test-package exclusion, safe diagnostic fields, and no secrets/prompts/response bodies.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/remainingSettings.test.ts tests/composables/appShortcuts.test.ts`

- [ ] **Step 3: Implement aligned sections and shortcuts**

Do not add unsupported system integrations. Update installation confirmation includes count of running sessions.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/remainingSettings.test.ts tests/composables/appShortcuts.test.ts tests/stores/update.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add src/components/settings/sections/ShortcutsSection.vue src/components/settings/sections/UpdateSection.vue src/components/settings/sections/AboutSection.vue src/composables/useAppShortcuts.ts tests/components/remainingSettings.test.ts tests/composables/appShortcuts.test.ts
git commit -m "feat: align shortcuts update and about interactions"
```

### Task 21: Remove Duplicate Product Surfaces and Lock Boundaries

**Files:**
- Modify: `src/App.vue`
- Delete: `src/components/NativeCliWorkbench.vue`
- Delete or repurpose after migration: `src/components/WelcomeView.vue`, `src/components/ProjectSelectView.vue`
- Modify: `src/stores/nativeWorkbench.ts` (reduce to internal bootstrap helper or delete if unused)
- Modify: `tests/productBoundary.test.ts`
- Modify: `docs/components.md`, `docs/native-cli-v3.md`, `README.md`, `README_CN.md`

**Interfaces:**
- Product has no `currentView === 'native'`, Native top-level button, Native internal tab strip, or raw JSON resource surface.

- [ ] **Step 1: Add failing boundary assertions**

Assert only the unified shell is mounted, `NativeCliWorkbench.vue` is absent, no user-facing “Native CLI workspace”, no top-level Skills/Agents/MCP/Plugins nav, and Native terminal files contain no legacy PTY APIs.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/productBoundary.test.ts`

- [ ] **Step 3: Remove duplicate surfaces after all callers migrate**

Preserve Native runtime/store/API modules used by adapters. Welcome/project empty states move into Workspace/Projects views.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/productBoundary.test.ts && npm run typecheck && npm run build`

- [ ] **Step 5: Commit**

```bash
git add -A src/App.vue src/components/NativeCliWorkbench.vue src/components/WelcomeView.vue src/components/ProjectSelectView.vue src/stores/nativeWorkbench.ts tests/productBoundary.test.ts docs README.md README_CN.md
git commit -m "refactor: retire duplicate native workspace surfaces"
```

### Task 22: Responsive, Accessibility, Localization, and Layout Contracts

**Files:**
- Create: `tests/components/responsiveLayout.test.ts`
- Create: `tests/components/accessibilityContracts.test.ts`
- Modify: `tests/i18n/translations.test.ts`
- Modify: `src/styles/global.css`
- Modify: affected components from Tasks 6–21.

**Interfaces:**
- Defines static/dynamic layout contracts for all supported window/scale/language combinations.

- [ ] **Step 1: Write failing contract tests**

Cover 1024/1280/1366/1440/1920 widths, long title/project/path, 150% scale variables, Chinese/English keys, focus order, icon tooltips, reduced motion, menu/dialog keyboard control, and no visible status text.

- [ ] **Step 2: Verify failure**

Run: `npm test -- tests/components/responsiveLayout.test.ts tests/components/accessibilityContracts.test.ts tests/i18n/translations.test.ts`

- [ ] **Step 3: Fix component contracts and tokens**

Do not solve overflow by hiding status, app, time, or menu columns. Use overlay drawer/collapsible session panel at pinned breakpoints.

- [ ] **Step 4: Verify**

Run: `npm test -- tests/components/responsiveLayout.test.ts tests/components/accessibilityContracts.test.ts tests/i18n/translations.test.ts tests/designTokens.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```bash
git add tests/components/responsiveLayout.test.ts tests/components/accessibilityContracts.test.ts tests/i18n/translations.test.ts src/styles/global.css src/components src/i18n
git commit -m "test: lock responsive and accessible UX contracts"
```

### Task 23: Visual Fixture and Screenshot Regression Harness

**Files:**
- Create: `src/visual/VisualFixtureApp.vue`
- Create: `src/visual/fixtures.ts`
- Create: `tests/visual/unified-workspace.spec.ts`
- Create: `playwright.config.ts`
- Modify: `vite.config.ts`
- Modify: `package.json`, `package-lock.json`
- Add development-only dependency: `@playwright/test`.

**Interfaces:**
- Fixture renders deterministic UI without real PTYs/Tauri mutations.
- Screenshot matrix covers compact/standard density, light/dark GUI, English/Chinese, long content, menu/dialog/drawer states, and representative terminal themes.

- [ ] **Step 1: Add failing visual test list**

Required named snapshots:

```text
workspace-empty-1024-zh
workspace-mixed-1366-zh
workspace-hover-action-1366-en
workspace-resources-overlay-1024
projects-150-percent
new-session-dialog
archived-sessions
settings-terminal-light-gui-dark-terminal
settings-launch-configurations
confirm-stop-and-archive
```

- [ ] **Step 2: Install and verify initial mismatch/missing snapshots**

Run: `npm install --save-dev @playwright/test && npx playwright install chromium && npx playwright test tests/visual/unified-workspace.spec.ts`

Expected: FAIL because baselines do not exist.

- [ ] **Step 3: Implement deterministic fixture route and capture approved baselines**

Fixture must use fake session/resource data and must not invoke Tauri or log user payloads.

- [ ] **Step 4: Verify**

Run: `npx playwright test tests/visual/unified-workspace.spec.ts`

Expected: PASS with no diff.

- [ ] **Step 5: Commit**

```bash
git add src/visual tests/visual playwright.config.ts vite.config.ts package.json package-lock.json
git commit -m "test: add unified workspace visual regression"
```

### Task 24: Migration, Stress, and Adversarial Coverage

**Files:**
- Create: `tests/stores/unifiedMigration.test.ts`
- Create: `tests/stores/unifiedStress.test.ts`
- Create: `tests/native-cli/unifiedUxAdversarial.test.ts`
- Modify: existing stores/helpers only for failures found by these tests.

**Interfaces:**
- No new product API; this task validates the completed system.

- [ ] **Step 1: Add migration and stress tests**

Cover old Claude-only data, old Native workspace, both together, invalid terminal theme fallback, 50 projects, 100 sessions/project, 30 open terminal descriptors, rapid state changes, 200-character titles, long Windows paths, and catalog records with malformed optional entries.

- [ ] **Step 2: Add adversarial lifecycle tests**

Cover stale create/recover/stop/resource completion, revision conflict without replay, duplicate native session across source roots, wrong runtime action routing, attempted secret rendering, and unavailable-one-CLI isolation.

- [ ] **Step 3: Run and observe failures**

Run:

```bash
npm test -- tests/stores/unifiedMigration.test.ts tests/stores/unifiedStress.test.ts tests/native-cli/unifiedUxAdversarial.test.ts
```

- [ ] **Step 4: Apply minimal fixes, then rerun targeted and core Native suites**

Run:

```bash
npm test -- tests/stores/unifiedMigration.test.ts tests/stores/unifiedStress.test.ts tests/native-cli/unifiedUxAdversarial.test.ts tests/native-cli tests/stores/session.test.ts tests/stores/nativeTabs.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tests/stores/unifiedMigration.test.ts tests/stores/unifiedStress.test.ts tests/native-cli/unifiedUxAdversarial.test.ts src
git commit -m "test: adversarially verify unified workspace migration"
```

### Task 25: Documentation, Final Freeze, Unified Verification, and Test Package

**Files:**
- Modify: `DESIGN.md`
- Modify: `PRODUCT.md`
- Modify: `docs/components.md`
- Modify: `docs/data-persistence.md`
- Modify: `docs/terminal-integration.md`
- Modify: `docs/roadmap.md`
- Create: `docs/superpowers/execution/U01-U10.md` or one focused record per completed milestone.
- Modify: `.github/workflows/conpty-integration.yml` only if the test-package UI verification list must be extended; do not add a publish path.

**Interfaces:**
- Final evidence binds the source head and PR merge commit. D20 remains `BLOCKED_EXTERNAL_TARGET` unless real authorized evidence exists.

- [ ] **Step 1: Update documentation and execution ledger**

Document the unified shell, adapters, catalog persistence, terminal-theme boundary, interaction vocabulary, visual matrix, and retired Native surface.

- [ ] **Step 2: Run full local verification before opening a PR**

```bash
npm ci
npm run typecheck
npm test
npm run build
npx playwright test tests/visual/unified-workspace.spec.ts
cd src-tauri
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

Expected: all PASS; working tree unchanged after generated-file checks.

- [ ] **Step 3: Perform final self-review and adversarial review**

Review against every spec section and the Review Focus list. Record all findings and fixes before opening the PR.

- [ ] **Step 4: Freeze the branch and open one draft PR**

Do not push further cosmetic commits after CI begins. The PR body lists the frozen source SHA, migration rules, visual evidence, dependency changes, and D20 boundary.

- [ ] **Step 5: Trigger only final workflows**

Required:

1. ordinary CI;
2. Windows test-package/ConPTY installer workflow;
3. visual workflow only if not included in ordinary CI.

Do not trigger D12/D13/D14/D17 or release workflows unless changed paths or a real failure require them.

- [ ] **Step 6: Validate Windows package manually and automatically**

Verify install/reinstall/relocation, old user data, mixed Legacy/Native sessions, Claude/Codex quick create and restore, session operations, GUI/terminal theme combinations, 125%/150% scaling, and no updater publication.

- [ ] **Step 7: Commit documentation before freeze**

```bash
git add DESIGN.md PRODUCT.md docs .github/workflows/conpty-integration.yml
git commit -m "docs: finalize unified workspace UX evidence"
```

## Plan Self-Review Results

- **Spec coverage:** Every design section maps to Tasks 1–25. Runtime safety, data compatibility, app shell, session operations, resources, settings, terminal themes, responsive layout, visual verification, CI restraint, and test packaging are represented.
- **Type consistency:** `UnifiedSession`, adapter contracts, catalog keys, shell sections, and resource ownership have one defining task and are consumed by later tasks with the same names.
- **Scope:** This remains one sequential plan because the session model, shell, terminal host, resource context, and settings all depend on the same unified identity. Splitting into independent plans would create competing navigation and persistence authorities.
- **Review focus:** Mixed identity, stale completion, long-content layout, partial CLI availability, and terminal continuity each have named owning tests.
- **Proportion:** The plan specifies interfaces, test names/assertions, commands, and task boundaries without transcribing implementation bodies.
