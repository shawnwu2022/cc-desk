# Component architecture

## Application shell

`src/App.vue` owns top-level view switching.

Main views:

- `WelcomeView.vue` — exposes Native CLI v3 as a first-class entry and keeps the legacy Claude compatibility entry.
- `ProjectSelectView.vue` — legacy Claude project/session management.
- `TerminalView.vue` — legacy Claude terminal workspace.
- `NativeCliWorkbench.vue` — forward-path Claude Code / Codex CLI workspace.
- `TitleBar.vue` — window controls plus a persistent Native CLI toggle.

Native and legacy terminal views stay mounted where required so active terminal state is not destroyed merely by switching UI views.

## Native CLI workbench

### NativeCliWorkbench.vue

Responsibilities:

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

The following remain compatibility components:

- `TerminalView.vue`;
- `XTermTerminal.vue`;
- the older `SidebarPanel.vue` composition, pending the unified shell integration;
- legacy Claude settings and hook-driven UI.

Legacy sidebar Skills/Agents/MCP/Plugins are projections. The removed Provider management UI and mutating resource toggles must not return.

## Settings

`SettingsOverlay.vue` / `SettingsView.vue` currently cover CC Desk-owned appearance/startup/shortcut/update/about settings.

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
adapter setup/dispatch and the full confirmation UI remain integration tasks; none
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
