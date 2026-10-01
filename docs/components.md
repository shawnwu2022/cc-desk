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

## Task 12: quick and advanced session creation

`ProjectNode` owns the anchored `NewSessionMenu`. Its existing `new-session-request`
event now carries `NewSessionRequest` (`projectKey`, `projectPath`, optional `intent`:
`claude`, `codex`, `restore`, or `options`). The panel expands the requested project
for a quick creation. Sidebar forwarding preserves this intent, and normal App's
existing shell request channel handles it. A bare request from the workspace header
or welcome action opens the same quick chooser. Only More options opens the
advanced dialog in the normal flow. The plus menu keeps one action per CLI, Restore and More
options, uses shared menu keyboard/focus handling, and clamps to the viewport.

`NewSessionDialog` is mounted once in normal App, uses `AppDialog`, and closes when
the Workspace surface becomes inactive. Its fields are vertically grouped as Basic,
More options, and Developer options. Project is read-only; configuration options use
human names. The permission field describes Desk’s configured flag injection only, with an
explicit warning that saved argv and CLI settings determine effective permissions.
It does not infer effective permission mode or parse flags; no per-launch permission
override exists in the protocol. Existing settings are never
mutated by selection. Raw mode explains that the existing backend bypasses saved
default argv, permission flag injection, and observer injection.

`useNewSessionDraftStore` provides `open`, `openChooser`, `toInput`, `prepareInput`, `preferred`,
`recordSuccess`, `setDefault`, `refreshAvailability`, and `availabilityFor(project)`.
A `CreateUnifiedSessionInput` may include `launchConfigRevision` to freeze the chosen
configuration. Exact raw arrays are never shell-split. Each line is one argument,
including blank and trailing lines; a completely empty editor means `[]`. JSON mode
represents newline-containing arguments and `[""]`; switching these to an ambiguous
line representation is refused rather than losing data. Draft raw args are not
persisted. Canonical project/CLI last-success preferences are read from
`projectsState.launchPreferences` and persisted through its existing
`setLaunchPreference` action and projects.json single writer. The draft waits for
canonical metadata before automatic selection; a not-yet-loaded automatic draft
does not freeze a fallback configuration prematurely. The setter merges the other
CLI field inside the serialized mutation, after previous snapshots are adopted.
Only the separate global CLI default selection uses optional local UI storage with
an in-memory fallback; local project history is ignored. A failed metadata save
leaves the running session intact, reloads within canonical writer queue ownership
without replaying the write, and surfaces a safe notice. An unsuccessful reload
invalidates the snapshot; queued mutations must obtain a verified read or stop. Refreshing that notice never retries a process launch.

Missing configuration leaves a CLI's availability unknown and permits explicit safe
preparation. Existing `cliGetAvailability` filesystem/configuration preflight is
read-only; `available-unverified` does not certify a launch. Unavailability evidence
is scoped to a configuration and its revision. A fresh successful preflight can
clear an older executable failure; failed reads do not erase known failure evidence.

Restore choices emit `restore-session { project, cli?, mode }`, where mode is
`history`, `resume-picker`, or `resume-id`. Task13 now handles this with the common
restore dialog. An explicitly selected advanced configuration also carries its ID
and revision; automatic new-session configuration selection is not reused.

## Task 13: unified restore and history search

`ResumeSessionDialog` is mounted once in normal App, uses shared modal/input/select/
button/notice/loading/empty-state primitives, and closes with the owning Workspace
surface. Quick Restore, history-row activation/Resume, and all three advanced modes
reach this same dialog through the runtime and `unifiedSessions.resumeDialog`.
Selecting a history result requests confirmation; confirmation activates the exact
existing attempt or resumes its exact origin. Dialog dismissal/navigation invalidates
pending validation admission, and latest-search ownership rejects late success,
failure and completion from older filters.

History search defaults to the request's current project. Title and Session ID text,
CLI, current/all project scope, and 24-hour/7-day/30-day activity filters combine.
Unavailable sources produce a partial-history notice without hiding readable sources.
History remains cached for filtering; workspace Refresh explicitly rereads sources.
Native history consumes all supported pages through the authenticated projection
client, rejecting partial/error reads as absence evidence.

`unifiedSessions` exposes `openResumeDialog`, `closeResumeDialog`, `searchSessions`,
`resumeCatalogSession`, `launchResume`, and `removeMissingRecord`. The runtime supplies
the read-only `configureHistoryLoader` port. Adapter admission takes an optional
`canAdmit` guard for cancellation before side effects; no guard is sent to the backend.
`ResumeUnifiedSessionInput.nativeOrigin` carries the exact historical CLI, profile ID/
revision, registered project ID and path. These fields are not ordinary UI copy.
`CreateUnifiedSessionInput.registeredProjectId` freezes a direct restore's explicitly
selected project. Direct-ID/picker confirmations require an existing configuration
and registered project; they never call new-session preparation or create a default.

A verified missing row keeps a safe explanation and a two-step Remove record action.
Removal rereads the exact source, loads canonical app metadata, removes only exact
matching optional UI records via `projectsState`, and removes the catalog row. It
never calls legacy `deleteSessions`, removes real CLI history, or writes a tombstone.
Rediscovered history may appear again. Unknown/unavailable sources are not missing
records. If the source reappears before removal, the record is retained and refreshed.

Native history keys now include complete origin identity. Previously persisted
archive keys remain recognized; an explicit restore clears an old key only if it
maps uniquely. Previously colliding old keys preserve all archive metadata and show
safe ambiguity guidance. Resolving that ambiguity is not an automatic migration.

### Task 13 review repairs: absence evidence and cancellation

Native `NativeHistoryEntry.absenceEvidence` is published only for a single complete
ready response from the authenticated source. The adapter compares its CLI/root key
with the original source encoded in the exact saved sessionKey before declaring a
session missing, and repeats that check before removing app metadata. A physically
replaced source under unchanged configuration/project identity is source uncertainty,
not evidence that the original history disappeared. Multi-page offset enumeration
has no common stable-snapshot token in the existing backend contract; it remains
positive discovery for search/resume but cannot certify a negative result. Such
records cannot safely expose Remove record until authoritative absence evidence is
available. No speculative second-pass snapshot or backend protocol is introduced.

A coalesced restore now retains separate caller cancellation guards. At least one
current explicit confirmation may admit the single shared result. A canceled caller
still rejects and cannot publish selection, while a fresh confirmation after closing
and reopening the dialog can succeed without waiting for a second manual retry.

## Task 16: typed confirmations and owned feedback

Normal App mounts `SessionConfirmDialog`, shared `ProjectConfirmDialog` consumers,
and `AppToastHost`. Session confirmation requests discriminate `close-running`,
`stop-and-archive`, and `restart-unknown`; their public state contains the session
identity and display title, while executable ownership guards stay private to the
catalog. The same flow consumes the existing tree stop-and-archive request and
normal runtime Close/Archive/Restart commands. Opening a dialog has no process
side effect. Native unknown Restart is reachable through the existing restart
command path; the row's existing confirm-status primary action remains unchanged.

`unifiedSessions.beginSessionConfirmation`, `confirmSessionAction`, and
`closeSessionConfirmation` own admission. Native identity includes the exact
attempt plus CLI, profile/revision, registered project/path, sourceSessionKey and
launch action. Legacy identity includes the Tab object, PTY/generation and
project/session identity; a successful stop may clear only that same PTY. Dialog
cancellation/navigation may occur while an issued stop is completing, but no later
close/archive/restart step may execute for an invalidated owner. An already-issued
metadata write is not rolled back or replayed. Late outcomes do not close a new
dialog or publish errors/toasts onto a changed selection or attempt.

The project removal confirmation now renders through `ProjectConfirmDialog`, while
rename remains in `ProjectManagementDialogs`. Task14 visibility/removal admission
barriers remain intact. A known registration is frozen at dialog creation; if it
was not loaded, the first authoritative read binds it before any write. Replacement
registrations, open sessions, cancellation and changed selection fail before later
writes. Project/CLI history files are never deleted.

Configuration deletion provides the real downstream Task19 contract:
- `cliProfiles.requestDelete(id)` returns/publishes a typed frozen confirmation,
  or null plus a safe `deleteError` for missing configurations or unadmitted preparation
- `confirmDelete()` rechecks the configuration revision, workspace CAS revision,
  current request and unadmitted Native/unified preparation before the existing `cliPatchProfile`
  delete operation; `deleteBusy` and `isDeleting(id)` provide admission barriers
- `closeDeleteConfirmation()` invalidates queued work and feedback ownership
- ordinary `patch(..., { op: 'delete' })` rejects with `CONFIRMATION_REQUIRED`
- conflict/unknown acknowledgement performs a read-only reload while holding the
  writer queue; an updated revision requires a fresh request/confirmation, and no
  delete is replayed automatically

Normal App already binds the typed store request to the shared confirmation dialog
on the Settings surface. Task19 now provides the real grouped configuration list/editor and menu trigger.
Tests drive the real Settings menu through normal App and the existing API boundary.

Safe error extraction only accepts fixed allowlisted codes with own properties.
Inherited keys such as `constructor`, `__proto__`, and `toString` map to the generic
safe fallback. Profile/workspace/catalog error state does not retain raw exceptions.
Local failures render mapped inline notices and safe diagnostic codes in details;
explicit Retry is a new typed request that rechecks original ownership. Single-CLI
failure banners clear when newer successful evidence supersedes them. Whole-workspace
failure requires all relevant sources to fail and no usable cached/open context;
its error surface hides, but does not unmount, the terminal host. Existing read-only
resource notices remain owned by Task15 and are not promoted to global failures.


## Launch configuration settings (Task 19)

`LaunchConfigurationsSection` is the real Settings section. It groups saved Claude
and Codex configurations, shows the existing per-CLI default, and uses shared buttons,
icons and a context/overflow menu. Edit is the only row quick action. Copy, Rename,
Make default and Delete are secondary actions; Delete goes through the App-owned
`ProjectConfirmDialog` and `cliProfiles.requestDelete` / `confirmDelete`.

`LaunchConfigurationEditor` accepts a typed create/edit/copy/rename request. It freezes
the source and workspace revisions when opened. Save passes an immutable
`LaunchConfigurationSave` to the existing profile writer queue. Inactive navigation,
unmount and Cancel invalidate admission and feedback ownership. Already issued writes
may still update shared authoritative state; they cannot publish into another editor.
After conflict/uncertain write, the list is reloaded read-only and Save stays disabled;
the user closes/reopens after reviewing that state. No patch is automatically replayed.

The editor progressively reveals explicit program/launcher choices, permission and
observer overrides, and exact argv. Set/Unset/Inherit remain distinct. Existing
environment literals and host-reference names are not copied into editor fields or
rendered attributes; only the environment variable name and override mode are shown.
Rename omits all other fields. Ordinary edits omit env, preserving opaque stored
values; Duplicate copies the original saved configuration under a fresh ID and revision
zero, guarded by its source revision. No provider/credential management was introduced.
