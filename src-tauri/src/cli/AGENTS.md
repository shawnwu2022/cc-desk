# Native CLI backend module

This directory implements the approved native CLI workspace plan. The older root AGENTS.md describes the existing Claude-only runtime; do not interpret that legacy description as permission to inject Claude settings into Codex.

- Keep resolved environment, argv and LaunchSnapshot backend-only. No Serialize on snapshots; Debug must remain redacted. Safe errors contain fixed codes/field identifiers, never original parser errors or user values.
- Reuse snapshot::configured_program/configured_runner in preflight and launch preparation. A selected program becoming unavailable is an error, never permission to choose another PATH candidate.
- cli_get_availability receives one strict request object: profileId and expectedRevision. The revision is the selected profile revision, not workspace CAS revision. Caller identity comes from the actual WebviewWindow.
- Availability is a point-in-time filesystem/environment preflight, not an executable-format check, CLI version/login probe, launch guarantee or compatibility certification. HostStatus reports empty PTY allocation independently. No CLI is spawned by preflight.
- Independent profiles never open legacy config.json. Preserve user-inherited variables; isolate only Desk-added overlays. Do not call legacy global checks/PATH refresh from new code.
- The public frontend wrapper is exported from src/api/cli.ts and implemented in cliAvailability.ts, matching the existing modular profile API.
- Keep discovery backend-only until authoritative exclusion roots and explicit candidate selection are integrated. Returned paths are candidates, not automatic trust.
- D09/D10/D11 own native argv construction, platform launch and actual run lifetime authorization. Do not wire the new launch route prematurely or change current PTY/paste behavior in D08.
- Tests go in src-tauri/src/tests; preserve behavioral RED/GREEN evidence and distinguish real OS probes from real Claude/Codex or WebView acceptance. See docs/superpowers/execution/D08-availability.md.
