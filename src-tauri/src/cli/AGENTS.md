# Native CLI backend module

This directory implements the approved native CLI workspace plan. The older root AGENTS.md describes the existing Claude-only runtime; do not interpret that legacy description as permission to inject Claude settings into Codex.

- Keep resolved environment, argv and LaunchSnapshot backend-only. No Serialize on snapshots; Debug must remain redacted. Safe errors contain fixed codes/field identifiers, never original parser errors or user values.
- Reuse snapshot::configured_program/configured_runner in preflight and launch preparation. A selected program becoming unavailable is an error, never permission to choose another PATH candidate.
- cli_get_availability receives one strict request object: profileId and expectedRevision. The revision is the selected profile revision, not workspace CAS revision. Caller identity comes from the actual WebviewWindow.
- Availability is a point-in-time filesystem/environment preflight, not an executable-format check, CLI version/login probe, launch guarantee or compatibility certification. HostStatus reports empty PTY allocation independently. No CLI is spawned by preflight.
- Independent profiles never open legacy config.json. Preserve user-inherited variables; isolate only Desk-added overlays. Do not call legacy global checks/PATH refresh from new code.
- The public frontend wrapper is exported from src/api/cli.ts and implemented in cliAvailability.ts, matching the existing modular profile API.
- Keep discovery backend-only until authoritative exclusion roots and explicit candidate selection are integrated. Returned paths are candidates, not automatic trust.
- D09 invocation borrows the immutable snapshot and rejects any mismatched LaunchRequest. It constructs argv only; it does not re-read files, recheck PATH, mutate environment or spawn a process.
- Invocation.environment() is the COMPLETE child environment, not an overlay. D10 must clear inheritance before applying it, including when it is empty; otherwise deleted variables would reappear.
- Structured actions prepend their native action arguments before explicit profile/extra arrays. Do not parse or deduplicate caller flags; raw controls the entire argv ordering. Structured locator validation is not a blacklist on raw CLI syntax.
- Nonempty inherited defaultCustomArgs is shell text and requires explicit migration before the new structured route is used. Do not split it on spaces, execute it implicitly or silently discard it. Raw, explicit arrays and unset are distinct alternatives.
- The requested session locator is not verified identity. Initial effective cwd, native config root and session ID remain unknown. Do not parse extra flags or transcripts to invent them.
- D10/D11 own platform launch and actual run lifetime authorization. The current legacy PTY/startup route is not migrated by D09; retain its regression behavior until the replacement is verified. D13 owns observer argument plumbing.
- Tests go in src-tauri/src/tests; preserve behavioral RED/GREEN evidence and distinguish real OS probes from real Claude/Codex or WebView acceptance. See docs/superpowers/execution/D08-availability.md and D09.md.
