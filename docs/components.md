# Component architecture

## Application shell

`src/stores/shell.ts` owns the only primary section (`workspace | projects | settings`),
logical viewport mode, session-column visibility/width and context-drawer state.
`AppShell.vue` owns the titlebar and four global columns: 44px primary navigation,
288px sessions (240–360px), a shrinkable `minmax(0, 1fr)` main region and optional
344px context (300–420px). Content views never recreate these columns.

- `PrimaryNav.vue` exposes only Workspace, Projects and Settings, using shared
  accessible controls and localized names. Skills/Agents/MCP/Plugins/Instructions
  belong to project/session context, not global navigation.
- `WorkspaceView.vue` stays mounted across section changes and exposes one
  `terminal` slot / `data-workspace-terminal-host`, filled by `UnifiedTerminalHost`. The project/session tree remains the only tab system.
- `WorkspaceHeader.vue` offers session-column and context toggles, project/session
  titles that ellipsize independently, and typed new-session/add-project requests.
- `SidebarPanel.vue` is a content-only unified `SessionsPanel` wrapper. It forwards
  Task 9 typed actions and confirmations without translating them to old Legacy
  launch events. Global column widths belong only to `AppShell`.
  Shell `active` ownership reaches SessionsPanel, ProjectNode, SessionList and
  SessionItem. On navigation away from Workspace or session-column collapse,
  panel-owned archived dialogs and teleported project/session menus close and
  release focus. Persistent search/expansion/inline editing and workspace hosts
  remain mounted; standalone list callers default their surface activity to true.
- `TitleBar.vue` takes the unified context title, preserves Windows minimize /
  maximize / close and macOS traffic-light space, and has no Native product toggle.
- Projects currently has a content-only landing; Task 14 owns full management.
  `SettingsView.vue` mounts within the main column, with its contextual settings
  subsection navigation, and stays mounted after first activation. Task 17 owns
  the settings-shell migration; Task 18 owns terminal preferences.

At widths below 1180 logical CSS pixels, context uses shared modal `AppDrawer`
instead of reducing the main column. Below 900, the session column starts collapsed;
its compact choice is independent of the desktop choice. Resize reads `innerWidth`,
not the physical display scale. The native window's default and minimum are
1024×640, and global containers use min-width zero with no horizontal overflow.

Normal application initialization loads GUI/application preferences independently
of CLI availability. GUI theme updates do not change terminal-theme preference or
session selection. OS Settings / Shortcuts menu events route to the single settings
section; directory and restart events become typed presentation requests only.
No Claude-only environment gate, automatic startup decision, implicit PTY launch
or old Native product page is mounted in the normal path. `useUnifiedWorkspaceRuntime`
loads Legacy projects/history, Native profiles/registered projects/history and shared
project metadata independently. A failing source leaves other sources and open
terminals usable; saved launch configurations do not certify real CLI availability. A CLI's unavailable state
is an inline per-CLI notice, so navigation and other sessions remain accessible.

`WorkspaceRequest` is a discriminated presentation-only union. `App.vue` publishes
`workspace-request` and stores the latest ephemeral intent in shell `pendingRequest`
with a monotonic `requestSequence`; an integrating owner may clear only its current
sequence. It is not a persistent queue or automatic replay mechanism. The runtime claims each
sequence once and clears only its matching completed request. It explicitly
admits/dispatches requests through runtime-owned adapters; paths in these
requests do not authorize Native filesystem access. The Legacy adapter reads `getCatalogHistoryFor`, an unfiltered cached history
projection. Ordinary `getHistoryFor` keeps its existing archived/claimed filtering,
while the unified catalog retains archived rows so the archive drawer can restore them.
Live close/archive requests remain pending for the confirmation layer (Task 16).
Admission reads current runtime state, rather than trusting an older catalog row.
Queued operations capture adapter ownership before awaiting; an ended close/archive
must still be ended at execution. Confirmations never belong to the shell. New-session dialogs
belong to Task 12, resume to Task 13 and contextual resources to Task 15.

The old App is isolated as `LegacyCompatibilityApp.vue`, reachable only with BOTH
Vite DEV and `VITE_CC_DESK_COMPATIBILITY=1`. Setting the flag in production cannot
activate it. This temporary development route, `IconBar` adapter and old typed
caller compatibility are removed in Task 21. Native bridge/terminal safeguards
remain unchanged; this shell checkpoint is not real CLI or platform certification.

## Unified terminal runtime

`UnifiedTerminalHost` consumes the selected catalog ID and open runtime descriptors.
It owns one embedded `TerminalView` / `XTermTerminal` aggregator for every Legacy tab,
and one stable-key `NativeCliTerminal` per open Native tab. History records never
mount terminals. Switching sessions, primary sections or GUI themes changes only
visibility. Embedded Legacy content has no IconBar, project tree, header or implicit
startup/menu listeners.

`UnifiedTerminalHostPort` exposes explicit Legacy start/stop/restart/rename and
Native stop/recover-by-attempt hooks. The children expose focus and visible fit;
hidden instances mark `needsFit` and retain their output. Native background status
reads and output/ACK continue; Legacy ended scrollback lasts until close/restart.
Global terminal theme and font-size changes update the existing terminal options.
Legacy spawn first awaits core output and exit subscriptions and rechecks the captured
tab/PTY generation. Optional drag/drop registration is independent of that gate.

Native creation resolves the existing profile revision and registered project.
Resumed history passes `sourceSessionKey` and its complete cached context to the
runtime before creating the tab. Restart retains that context and revalidates it
after stop. It never creates a profile, registers a frontend path, or changes argv.

Runtime request handling supports open-session activation, explicit stop/cancel,
exact status recovery, explicit restart/retry, open-tab rename, ended close/archive,
archive restore, copy session ID and open project directory. Creation/resume dialogs,
project management, resources, diagnostics and required confirmations remain explicit
pending requests for Tasks 12–16. Unsupported operations are never translated to
Legacy commands. Refresh reloads sources; it does not retry a failed lifecycle request.

The optional adapter `captureOwnership` hook freezes exact Native request/run/generation
or Legacy tab/local PTY generation at facade admission, before an action waits in a
queue. Lifecycle completions recheck ownership before touching a newer context.

## Native CLI workbench

### NativeCliWorkbench.vue

Retained only by the DEV compatibility route; the normal shell never mounts its
independent tab strip. Compatibility responsibilities:

- Claude/Codex switch;
- independent profile selection/bootstrap;
- registered project selection;
- create New / resume-picker / resume-ID / raw-argv tabs;
- recover, stop and explicit restart controls;
- authenticated read-only resource projection.

The workbench must not invoke legacy Claude PTY APIs.

### NativeCliTerminal.vue

Responsibilities:

- xterm lifecycle for one native tab;
- launch through `createNativeLaunchEntry`;
- bind the exact run through `createDeskNativeTerminalBinding`;
- ordered native input;
- output parse/ACK;
- authenticated resize/stop;
- safe diagnostic projection.

No arbitrary HTML rendering or payload logging is allowed.

## Native stores

### nativeWorkbench.ts

Coordinates:

- `cliProfiles`;
- `cliWorkspace`;
- `nativeTabs`;
- selected CLI/project;
- safe workbench error projection.

### nativeTabs.ts

Stores the stable tab identity:

```text
tabId
cli
projectId/projectPath
profileId/profileRevision
requestId
runId
generation
action
status/errorCode/launchRevision
```

Only exact request/run/generation launch status may be adopted.

### cliProfiles.ts / cliWorkspace.ts

Own frontend snapshots of backend workspace/profile data and revisions.

They do not turn frontend paths or IDs into filesystem authority.

## Native APIs

- `src/api/cli.ts` — profile/workspace/native command facade.
- `src/api/nativeProjection.ts` — scoped resource projection.
- native functions in `src/api/tauri.ts` — authenticated document-bridge calls for runtime operations.

Bare `invoke(...)` fallback is forbidden in the native authenticated section.

## Native terminal helpers

`src/terminal/` contains the host protocol pieces:

- input intent queue;
- input policy;
- host protocol/provenance routing;
- launch entry;
- terminal binding;
- output transport frontend state;
- run lifecycle helpers.

These helpers preserve source/ordering identity rather than inferring behavior from byte content.

## Legacy workspace

The following retain the Legacy transport:

- `TerminalView.vue` (embedded content-only mode in the unified host);
- `XTermTerminal.vue` (one aggregator, explicit visibility, exact PTY lifecycle);
- `LegacyCompatibilityApp.vue` behind the explicit DEV-only flag;
- legacy Claude settings and hook-driven UI.

Legacy sidebar Skills/Agents/MCP/Plugins are projections. The removed Provider management UI and mutating resource toggles must not return.

## Settings

`SettingsView.vue` covers CC Desk-owned appearance/startup/shortcut/update/about settings inside the unified shell. `SettingsOverlay.vue` is retained only for the development compatibility app.

They do not provide Provider/API-key management.

## Testing boundaries

`tests/productBoundary.test.ts` protects high-level architecture:

- deleted Provider management stays deleted;
- CLI installer/overwrite APIs stay deleted;
- native workbench does not use legacy PTY APIs;
- authenticated IPC has no bare invoke fallback;
- native DOM/log surfaces stay inert/redacted;
- native resource panels stay projection-only;
- Native CLI remains a first-class entry;
- release docs match the enforced candidate-only workflow.

## Unified session icon primitives

`src/components/sessions/SessionStatusIcon.vue` accepts `state: SessionVisualState`
and renders a 16px icon, without an inline status-label node. The bundled SVG
shape identifiers are `gap-ring`, `active-play`, `reply-dot`, `question-diamond`,
`stop-square`, and `alert-triangle`. Only allowlisted static project SVG imports
are rendered; caller data never becomes SVG/HTML markup. The accessible name and
shared tooltip use the same English/Chinese locale key. Its single actual trigger
is keyboard-focusable and has the shared 2px ink-blue focus ring.

Starting rotates slowly; confirming breathes weakly with a .75 minimum opacity
to preserve ≥3:1 shape contrast; needs-user gives one brief cue on each entry
into that state. Stable state/localization updates do not
recreate the shape or replay that cue. Running, ended, and failed are static.
The later reduced-motion rule matches the animation selectors' specificity and
disables every animation.

`src/components/sessions/CliAppIcon.vue` accepts `cli: 'claude' | 'codex'`. Its
16px self-owned neutral SVG image uses low-saturation neutral ink and never
derives color from session state. Its GUI dark-theme-only brightness(1.4)
treatment preserves ≥3:1 contrast on primary/secondary/tertiary/hover row
surfaces and their selected overlays; light GUI keeps the original ink. The
image is decorative inside a single labelled keyboard-focusable `AppTooltip` trigger. Tooltip names are
`Claude Code` and `Codex CLI`. Only a current image loading error enables the
visible `CC` or `CX` fallback; changing CLI retries its image and rejects stale
errors from detached image nodes. The tooltip trigger remains stable across
fallback changes, preserving focus.

Both asset directories include MIT ownership notices. These are CC Desk-created
recognition marks, with no assumed right to official Anthropic/OpenAI artwork.
The Task 7 component gate is `tests/components/sessionIcons.test.ts` plus
`npm run typecheck`; actual Windows scaling and visual accessibility remain
separate final gates. Existing session rows are migrated by subsequent tasks.


## Unified project/session tree

`SessionsPanel.vue` consumes `UnifiedProjectGroup[]` and archived `UnifiedSession[]`
(or defaults to the unified catalog store). `ProjectNode.vue` directly nests mixed
Claude Code and Codex CLI rows through the strictly unified `SessionList.vue` and
`SessionItem.vue`; there is no legacy visual tree or tabs/history compatibility
adapter in those components. The global skip-permissions/custom-args footer is
removed; launch settings belong to the upcoming new-session/settings surfaces.

The 40px project row reserves arrow, flexible single-line name, attention marker,
new-session and overflow columns. Full project path is available in the shared
keyboard/pointer tooltip. A collapsed project's unified needs-user count keeps an
attention marker visible. Its only high-frequency inline action is new-session;
pin/unpin, rename, archive view, directory open and project removal share one
`AppMenu` for overflow and context entry points.

Project actions carry `ProjectActionRequest { action, projectKey, projectPath }`.
New sessions use `new-session-request` with the same project identity, deliberately
separate from the older container's legacy new-session event. Session activation,
primary/menu actions and rename requests carry catalog IDs. A running archive is
intercepted as `confirmation-request` with
`{ kind: 'stop-and-archive', sessionId, projectKey, projectPath }`. It never invokes
stop/archive itself. Unknown/starting sessions cannot request archive. Runtime
adapter setup/dispatch now belongs to `useUnifiedWorkspaceRuntime`; full confirmation
UI remains Task 16; none
of the tree components imports legacy PTY commands or performs lifecycle writes.

Explicit expansion is stored by project key. Search matches project display name,
original basename, path or session title, expands temporarily and disables toggles.
Clearing search restores the explicit state. Nested control keys are guarded and
consumed menu/editor Escape events do not dismiss the panel.

`ArchivedSessionsDrawer.vue` uses shared `AppDrawer` and the same session row/list.
It filters retained archived records by an optional normalized project identity
and sends `restore-request` for list-only restoration. Clicking a row does not
implicitly restore or launch it. Ordinary groups continue to exclude archived
records; archive-only project shells retain the per-project archive menu, and the
panel-level archive entry remains available even with no matching search results.
There is no native-history permanent-delete affordance in this drawer.

The drawer passes `menuTeleport=false` through the list/row to
`SessionOverflowMenu.vue`. The fixed-position shared menu then stays inside the
modal's DOM/focus boundary; normal tree menus still teleport to body. Shared focus
trapping, menu keyboard navigation, Escape and focus return remain authoritative.

Targeted gate: `npm test -- tests/components/projectSessionTree.test.ts
tests/sidebarKeyboardHandlers.test.ts && npm run typecheck`. Row, unified-store,
i18n and shared primitive regressions are affected narrow checks. CSS-rule/jsdom
checks do not certify Windows font layout, 1024×640 or 100%/125%/150% scaling.
