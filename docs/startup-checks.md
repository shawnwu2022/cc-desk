# Startup and availability checks

CC Desk has one application startup path and two runtime adapters. Startup loads preferences and independently reads Legacy history, Native configurations/projects/history and project metadata. It never implicitly launches a CLI.

## Native CLI v3

Native CLI startup does **not** depend on the legacy Claude environment-check overlay.

Workspace, Projects and Settings remain reachable when either CLI is unavailable. Native availability and launch are evaluated through the selected launch configuration and authenticated launch path; failures are scoped to that CLI.

For a native launch, the backend resolves and freezes the exact launch snapshot for:

```text
cli + profile revision + registered project + requestId + runId + generation + action
```

If the CLI/program/profile/project is unavailable, the native path returns a bounded safe error. It does not invoke an installer, mutate CLI configuration, or fall back to the legacy Claude PTY path.

### No automatic CLI installation

CC Desk no longer ships an installer path that downloads, overwrites, switches versions of, or globally kills Claude Code / Codex CLI.

Users install and authenticate the CLI through the CLI/vendor-supported mechanism appropriate to their system.

The product-boundary test rejects reintroduction of the old installer APIs.

## Legacy compatibility

Existing Claude sessions and history remain available through the Legacy adapter
in the unified shell. The old Welcome/ProjectSelect decision and environment-check
overlay route are removed. Compatibility check APIs and stored paths remain for
the Legacy runtime; no global startup gate or automatic Legacy launch calls them.

## Failure semantics

Native failures are fail-closed and identity-preserving:

- a synchronous launch failure is reported for the reserved request/run;
- an indeterminate launch does not allocate a replacement process;
- recovery queries the original request;
- an explicit restart creates a new request/run generation;
- arbitrary native exception text is not reflected into the UI.

## Observer availability

Observer startup is not a process-start requirement.

For Claude native profiles where the observer is enabled, observer capability setup is optional metadata infrastructure. Its failure cannot kill, restart or automatically retry the CLI.

Codex/raw/shell launches never receive the Claude observer overlay.

## Testing

Use:

- frontend launch/unified-runtime tests;
- Rust launch/availability/supervisor tests;
- product-boundary tests;
- OS/runtime CI where applicable.

Real CLI behavior is a separate D20 Layer-C certification gate. Host availability tests do not certify an installed Claude Code or Codex CLI version.
