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
- sessions/sidebar views;
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
