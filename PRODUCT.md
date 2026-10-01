# Product

<!-- impeccable:product-schema 2 -->

## Platform

Tauri 2 desktop application using the system WebView.

Primary design target: Windows. macOS and Linux must remain functionally correct.

## Users

CC Desk is for developers who use **Claude Code and/or Codex CLI** heavily and need multiple projects or sessions visible at the same time.

The target user already understands terminal-native workflows. CC Desk should reduce coordination overhead without replacing the CLI interaction model.

## Product purpose

CC Desk is a native multi-project, multi-session host for real developer CLIs.

Success means a user can:

- keep several Claude/Codex sessions alive concurrently;
- switch project/session rows without losing terminal state;
- start, resume, recover, stop, and explicitly restart an owned native run;
- inspect useful native resources without giving the frontend arbitrary filesystem authority;
- retain the real CLI's commands, prompts, authentication, permissions, extensions, editor behavior, and future updates.

## Architecture direction

There is one application shell: Workspace, Projects and Settings. The project/session tree mixes Claude Code and Codex CLI sessions and is the only tab system. Resources are six structured read-only context views, not global navigation.

Two runtime adapters share that shell: Legacy Claude preserves existing sessions/history, while Native CLI v3 is the authenticated forward path for all new Claude/Codex sessions. The old product pages and compatibility startup routing are retired.

The native workspace owns host mechanics only:

- project/profile/run identity;
- process launch and lifecycle;
- ordered input;
- bounded output transport;
- authenticated read-only resource projection;
- optional observer metadata;
- safe diagnostics.

The CLI remains the capability owner.

## Product boundaries

### CC Desk owns

- desktop project/session organization;
- one project/session tree and restart/recovery controls;
- terminal rendering and host protocol;
- input/output transport integrity;
- application-local launch-configuration/workspace metadata;
- backend-held read capabilities for native projections;
- optional observer delivery that cannot own the process.

### CC Desk does not own

- Provider/API-key switching;
- native CLI configuration mutation;
- installation or replacement of Claude Code / Codex CLI binaries;
- an independent MCP runtime;
- reimplementation of slash commands, approval prompts, login flows, editors, or extension runtimes.

Provider/configuration switching belongs to the CLI or external tools such as cc-switch.

## Data boundary

Native CLI workspace state is stored separately from the legacy compatibility files in `cli-workspace.v1.json`.

Legacy values may be read only where explicitly allowed for Claude compatibility. Codex must never inherit legacy Claude secrets.

Native resource reads are scoped and projection-only. Raw headers, env values, credentials, arbitrary argv, secret-looking errors, and unbounded native payloads must not cross into the UI.

## Reliability principles

- ambiguous launch state is recovered through the original request, not a second spawn;
- ambiguous/partial input writes are never replayed;
- output ACK is exact-owner, exact-run, exact-generation, exact-stream and parsed-boundary only;
- route loss and observer failure must not restart a healthy CLI;
- explicit restart creates a new request/run generation;
- mixed-version rollback must not mutate or revive native state through legacy files.

## Evidence model

Verification is split into three layers:

- Layer A: unit/frontend/policy contracts;
- Layer B: host/runtime integration;
- Layer C: real installed Claude Code / Codex CLI execution.

Layer A/B evidence must never be relabeled as Layer C.

D20 implements the fail-closed Layer-C harness, but real certification remains BLOCKED until an authorized target environment runs the required real-CLI matrix.

## Release boundary

The repository currently builds signed candidates only. Publishing is disabled by policy and workflow structure.

A future publishing path requires an explicit promotion decision. It must not be inferred from a version bump or a green code-side CI run.

## Unified workspace behavior

The selected session owns resource context. New Claude/Codex sessions use Native admission; historical resume keeps its authenticated source and runtime. Startup only restores a UI destination, never a process. Selecting an already open source reuses its terminal, including unknown attempts. Explicit start/stop/restart/close/archive actions revalidate ownership after waits.

Projects can be added once and normalized path variants reuse registration. Hide/remove is blocked by any open or preparing terminal; removal unregisters where present and hides the ordinary row while retaining CLI files, history and Desk metadata. Legacy-only remove shares the reversible visibility behavior of Hide.

Resources expose Instructions, Settings, MCP, Skills, Agents and Plugins as safe typed observations. Native run scope is preferred; invalid run authority never falls back to current defaults. Legacy observations require proven exact-project authority, omit ambient global/user records and remain partial; unsupported instructions are unavailable. Empty, unavailable, partial and stale-same-owner states are distinct.

Settings has General, Appearance, Terminal, Launch configurations, Shortcuts, Update and About. GUI/terminal preferences are independent and share a serialized compatible settings writer. Launch editors preserve exact argv and expose environment names/modes without rendering existing values. Update installation remains disabled under the candidate-only policy; About diagnostics use an allowlist.

## Current acceptance status

The unified source and host/component contracts are implemented through Task 24. The local Task 25 evidence is recorded in [U01–U10](docs/superpowers/execution/U01-U10.md). The refactor is **not yet visually frozen or fully accepted**: Task 23 is `BLOCKED_VISUAL`; final Rust/CI, Windows package/manual/scaling, native macOS/Linux rendering, accessibility and D20 gates remain separate.

A future PR must bind the tested source head and PR merge commit to its CI, visual and installer artifacts. Candidate screenshot generation is only review input; every PNG needs inspection, any fixes need regression evidence, and a subsequent no-diff run is required. No local host test or pre-existing Windows package result establishes acceptance of this new UX head.

## Remaining direction

- Complete the rendered and authorized installed-platform matrices, including CJK/emoji and real CLI continuity
- Review migration and old user data on the exact final package
- Complete D20 with authorized CLI/account/target evidence
- Decide any later Legacy deprecation separately
- Design explicit signed-candidate promotion only under separate approval
