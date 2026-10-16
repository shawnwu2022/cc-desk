# Native CLI v3 — authoritative architecture

> Status: code-side implementation through D27 is complete on the stacked native-CLI branch. D20 real Claude Code / Codex CLI Layer-C certification still requires an authorized target environment and must remain BLOCKED until that evidence exists.

## Product boundary

CC Desk is a desktop workspace for **Claude Code and Codex CLI**. The native path runs the real CLI process in an owned PTY and keeps the CLI responsible for its own interaction model, commands, permissions, authentication, MCP/runtime behavior, skills/agents/plugins, editors, and future CLI features.

CC Desk adds the host capabilities that a normal terminal does not provide well:

- one project/session tree with concurrent Claude/Codex sessions;
- stable project/profile/run identity;
- exact launch/recovery semantics;
- authenticated read-only native-resource projection;
- bounded ordered input and output transport;
- optional, isolated observer metadata;
- restart/recovery controls without replaying ambiguous writes.

CC Desk does **not** own Provider/API-key configuration. Provider/configuration switching belongs to the native CLI or external tools such as cc-switch. The native resource panels are projection-only.

## One product shell, two runtime adapters

`AppShell` exposes only Workspace, Projects and Settings. The project/session tree
mixes Claude Code and Codex CLI directly below projects and is the only tab system.
There is no independent Native page, title-bar switch or development-only Legacy
startup route. Welcome and project-empty states live inside the unified views.

`useUnifiedWorkspaceRuntime` performs read-only bootstrap and explicit action
dispatch through the two adapters:

1. **Legacy Claude adapter** preserves existing history and PTY ownership through
   one content-only `TerminalView` / `XTermTerminal` aggregate.
2. **Native CLI adapter** uses `nativeTabs`, `cliProfiles`, `cliWorkspace` and the
   authenticated Claude/Codex runtime introduced by D01–D27.

New sessions use the Native adapter. New dual-CLI work must not fall back to legacy
`ptySpawn`, `ptyInput`, `ptyKill`, legacy Claude roots or deleted mutating resource
APIs. A missing CLI leaves the other CLI and global navigation usable. Mounting or
switching views never implicitly launches a process.

The old `NativeCliWorkbench` component and its unused coordinating store are
removed. Their runtime/API dependencies remain because the unified adapters own
them. Settings use the user-facing name “Launch configurations” / “启动配置”;
profile IDs/revisions and Native/Legacy identity stay internal or diagnostic.

## Native launch identity

A native tab freezes:

```text
cli
+ projectId / selected project path
+ profileId / profileRevision
+ requestId
+ runId
+ generation
+ launch action
```

Supported launch actions are:

- new session;
- native resume picker;
- explicit native session ID;
- native unarchive (one-shot, see below);
- exact raw argv as JSON `string[]`.

Raw argv is not shell-split. An uncertain launch result keeps the original request/run identity and is recovered by querying that same request. It is never replaced by an automatic second spawn.

The unarchive action is a one-shot CLI command, not a session. It freezes the same profile/environment snapshot and builds argv through the same action mapping, but registers no run, opens no document, returns no tab and publishes no output: the CLI owns its own archive state and CC Desk only asks it to change that state (`codex unarchive <id>`). The child still runs in a PTY through the same admission gate, output is discarded, and only the exit status counts; the authenticated document is rechecked at the spawn commit point, and a backend owner observes shutdown and retains the exact PTY after a wait error or deadline. The caller receives one bounded safe failure while that owner retries termination and nonblocking reap with an interval; ownership and maintenance admission remain held until actual exit is proved. Unknown outcomes are not replayed. Restoring a Codex-archived session must go through this action because the CLI refuses to resume an archived session ("session <id> is archived. Run `codex unarchive <id>` …"); Claude has no such subcommand and rejects the action instead of silently degrading into a resume.

Fully converged launch receipts are evicted within the registry record budget. A reaped run's receipt stays pinned while its supervisor still owns output drain; releasing process handles does not make the receipt evictable. Final convergence marks the exact request/run receipt ready for eviction under the active-supervisor lock before removing that owner. Generation floors for the current authenticated document have a separate limit of twice that budget; reaching it rejects new tab identities with `REGISTRY_CAPACITY`, while known tabs can still advance their generations. Floors cannot be forgotten while that document remains authorized. Activating a new backend-authenticated document revokes the old epoch and clears only its floors, retaining all still-owned runs.

`cli_stop` carries the frozen `requestId`, `runId` and `generation`. Backend admission checks all three against one authenticated registry receipt before the supervisor revalidates the frozen owner and pinned state. A delayed stop from an evicted request cannot acquire authority over a newer request that reuses the same run key.

## Profiles and workspace storage

The native store is `cli-workspace.v1.json`.

Important rules:

- writes use an independent lock, revision/CAS validation, synchronized temporary-file replacement, and fail-closed parsing;
- `inherit`, `set(value)`, and `unset` are distinct states;
- explicit false/empty/unset values are not revived by later legacy writes;
- Codex never inherits legacy Claude secrets or settings;
- unknown schema-v1 extensions survive reads and writes;
- legacy compatibility files may still exist, but new native writes do not mutate them.

## Authenticated document and projection boundary

Native frontend calls are admitted through the authenticated document bridge. Frontend-selected paths or opaque IDs alone never authorize filesystem access.

Native resource reads:

- bind the selected CLI/profile revision and the registered projects in the observation; every project in the set is admitted individually, and the set is never a way around a single project's authorization;
- use one fresh workspace document snapshot per scope admission or authority check to validate the profile and all registered projects together, keeping workspace parsing independent of the project count;
- group history observations within the 64-project scope budget; split only a backend-reported project-admission failure so offline projects retain their own error and cannot suppress healthy projects' admitted histories;
- replace superseded nonempty profile project subsets within the same owner, configuration revision and held roots; root-wide, run, other-source and disjoint batch authorities remain separate. In-flight reads retain only their original grant, and future reads through a replaced reference fail closed;
- use backend-held scoped filesystem authority;
- return kind-specific projection DTOs only;
- do not return raw env values, headers, credential material, arbitrary argv, or unbounded native errors.

Codex rollout projections include `.jsonl.zst` alongside `.jsonl`, using bounded
streaming decompression through the same held source capability. Compression does
not change archive state or project/session identity. Input/output budgets and
partial-observation semantics are described in [data persistence](data-persistence.md).

The contextual drawer renders six structured categories: instructions, settings,
MCP, skills, agents and plugins. It prefers exact active-run authority; a missing or
invalid run never falls back to current defaults. Profile/project changes reject
stale completions. Legacy project-only observations, unavailable categories and
bounded partial pages are labeled explicitly. No raw resource JSON is rendered.
See [project resource contracts](project-resources.md).

GUI appearance and terminal preferences remain independent. Both adapters share
the same computed terminal options; color/cursor changes preserve terminal identity,
and metric changes fit visible terminals with deferred hidden fits. Renderer choice
applies to new terminals only. See [terminal preferences](terminal-preferences.md).

## Input path

Native input is an ordered per-run intent queue.

Key properties:

- monotone sequence is reserved before asynchronous clipboard work;
- keyboard, IME, paste, protocol replies, and native image-paste intent cannot silently reorder;
- large input uses authenticated staged `begin -> chunk* -> commit`;
- one PTY writer owns a complete frame at a time;
- partial/unknown host writes freeze later user input;
- partial/unknown writes are never replayed automatically;
- ambiguous `onData` bytes are never guessed to be terminal protocol replies.

## Output path

Native output uses a bounded stream with explicit offsets and acknowledgements.

Key properties:

- per-run high/low-water backpressure;
- global bounded payload budget;
- FIFO budget waiter progress;
- ACK must belong to the exact owner/run/generation/stream epoch;
- ACK must land on a parsed frame boundary;
- route revocation releases budget and wakes blocked peers;
- a degraded run does not poison another run.

## Supervisor lifecycle

The supervisor keeps the exact owned child and active run until a real PTY wait/reap succeeds. A wait error records an unknown/failed observation; it does not manufacture process exit or release process control. A later stop or shutdown starts at most one waiter for that same child. If an already accepted stop is followed by a wait or registry-transition error, the supervisor performs one delayed bounded retry; shutdown retries use the same monotonic minimum interval even when failure notifications arrive early. Repeated errors remain active and retryable instead of busy-looping or reporting a clean shutdown.

After real reap, both the registry exit transition and resource retirement must succeed before the supervisor removes the active entry or drops its retained resource. Either registry step can be retried against the same run without spawning a process or replaying input. Only that completed registry convergence permits the supervisor entry to retire with lifecycle `Exited`; output drain and exact ACK remain independent. The registry keeps the retired receipt pinned against capacity eviction until the exact active supervisor state completes its terminal drain and marks retirement complete. Retirement cannot complete while process adoption is still installing the reader and stream. At terminal completion the supervisor first detaches that exact stream without progress callbacks, so retained reader or ACK references cannot occupy the reused run key or its output credit, and only then releases the active state and receipt.

Caller stop carries `requestId` with the run identity. One registry lock admits that exact receipt, then the supervisor pins one active state and matches the receipt against its frozen request, owner, run and retained resource identity. A retired draining state remains bound to that pinned receipt, so delayed control from an old document or an evicted old request cannot be redirected to a newly adopted child that reuses the same run key.

## Observer boundary

Observer delivery is optional metadata, not process ownership.

- observer credentials/capabilities are scoped to the exact run/document;
- only allowlisted bounded metadata reaches the owning WebView;
- prompt, response, env, credential, and raw error bodies are excluded;
- observer failure cannot kill, restart, or implicitly retry a CLI;
- Codex/raw/shell launches never receive the Claude observer overlay.

## Security and diagnostics

Native UI surfaces are interpolation-only: no `v-html` / `innerHTML` execution path.

Public diagnostics use a fixed safe-code allowlist. Arbitrary exception text, serde errors, secret-looking codes, paths, env values, and user payloads are not reflected into terminal/resource views or notifications.

## Windows runtime

Windows uses the verified app-local ConPTY runtime. The installed-runtime certification for the tested Windows Server 2022 target verifies reinstall, relocation, missing-runtime and corrupt-runtime fail-closed behavior.

That evidence is target-specific; it is not a universal certification of every Windows build.

## Verification layers

Evidence is intentionally split:

- **Layer A** — pure/frontend/unit contracts.
- **Layer B** — host/runtime integration on CI targets.
- **Layer C** — real installed Claude Code / Codex CLI behavior using an authorized test account.

Layer A/B cannot be promoted to Layer C.

## Current code-side status

Completed and CI-verified:

- D01–D19 native foundation, authenticated scope, transport and terminal protocol work;
- D21 installed/runtime bridge certification on the tested Windows target;
- D22–D24 dual-CLI product integration and recovery workspace;
- D25 mixed-version rollback safety;
- D26 security/log/DOM/IPC boundaries;
- D27 low-resource fairness and fault stress.

D20's harness and target-machine command are implemented and tested, but real product certification remains **BLOCKED** until an authorized target environment supplies real Claude Code and Codex CLI evidence.

## Release boundary

The authorized release recovery path builds one signed candidate set and promotes it only from the exact current protected main commit after full matching main CI succeeds. The three platform artifacts must belong to the same release workflow run; all updater signatures are verified against the actual bytes and configured public key. Existing tags, releases and drafts block ordinary promotion. See [release-process.md](release-process.md).

A candidate build is not a published release. D20 real-CLI certification remains separate and BLOCKED unless its actual target evidence exists. Restoring publication does not certify D20 or enable automatic client installation.

## 0.18.1 consolidation boundaries

The shared frontend logging command now reduces untrusted string content to its UTF-8 byte count before every severity sink. This is a backend entry-point guarantee for both runtime frontends; it does not depend on every caller remembering to redact. Existing backend diagnostic channels retain their own allowlisted contracts.

Transport high-water hysteresis is latched when a frame reaches the high watermark, under the same accounting lock. An intervening ACK above the low watermark cannot admit another read. Exact ACK ownership, per-stream accounting, FIFO global budgets, revocation and cleanup retain their existing semantics. Production-source core regressions cover this ordering and 32 concurrent streams sharing a 16-byte budget; such host tests do not substitute for real CLI or native-platform acceptance.

Native input receipts are correlated against the frozen run, generation, sequence, mode epoch and expected byte count. The xterm 5.5.0 mode tracker observes nine public modes after parsed write batches; it detects changes across batches, not a mode round trip wholly inside one batch. Exhaustion and disposal permanently reject further epochs. A paused write produces a fixed localized diagnostic and never automatically replays pending input; inspect the session before deciding whether to restart. These changes do not add backend/schema fields or change queue admission and recovery semantics.
