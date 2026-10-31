# Native CLI v3 — authoritative architecture

> Status: code-side implementation through D27 is complete on the stacked native-CLI branch. D20 real Claude Code / Codex CLI Layer-C certification still requires an authorized target environment and must remain BLOCKED until that evidence exists.

## Product boundary

CC Desk is a desktop workspace for **Claude Code, Codex CLI and opencode**. The native path runs the real CLI process in an owned PTY and keeps the CLI responsible for its own interaction model, commands, permissions, authentication, MCP/runtime behavior, skills/agents/plugins, editors, and future CLI features.

opencode's history is not per-project files but a selected SQLite store under the OpenCode data root, or the exact file named by `OPENCODE_DB`. Its projection runs through a dedicated read-only reader (`cli/native_projection/opencode_db.rs`), not the file-walking catalog path; attribution matches `project.worktree` (named projects) or `session.directory` (the drifting `global` bucket) by normalized spelling, and sessions with a non-null `parent_id` (subagents) are excluded. The CLI has no picker/rename/archive verbs. Picker/archive CLI actions remain explicitly unsupported; list rename uses the official scoped HTTP metadata API described below. Known inherited defect: `session.directory` is never updated when a project directory is renamed (upstream anomalyco/opencode#23248, verified), so sessions recorded under the old path stay invisible after a rename; Desk neither moves nor guesses. Resume (`-s <id>`) ignores the launch cwd and re-enters the session's recorded directory, so `effective_cwd` stays Unknown. Layer C (real logged-in opencode four-cell: create/resume/stop/restart-continue) remains BLOCKED like the other CLIs.

The SQLite reader rejects linked or non-regular selected database, `-wal`, and `-shm` entries. On Windows, where SQLite's VFS can follow a sidecar link, Desk opens the database and WAL through the retained root capability, copies them into a private temporary directory, and pins the private db/WAL/SHM files while SQLite reads only that copy. A shared source lock preserves the exclusive-writer `SOURCE_BUSY` boundary; a changed file fails the snapshot. The copy is capped at 1 GiB and may return `SOURCE_TOO_LARGE`. On Unix, SQLite's VFS uses `NOFOLLOW` for these files; before opening, Desk resolves only the already authorized root directory and rechecks its held identity so a symlink in an authorized ancestor (such as macOS `/var`) does not block normal reads. The database and both sidecars remain subject to the no-link checks, and root path replacement still fails closed. The `global` project ID is excluded from named-project worktree matching; its sessions are attributed only by their own directory.

For OpenCode history, a nonempty `OPENCODE_DB` selects that exact absolute file or a path relative to the OpenCode data root. `:memory:` has no file projection. Without an override, a profile scope probes the selected CLI using its official read-only `db path` command with the frozen profile environment, a bounded owned helper, and an admitted project cwd. A structured run probes the same exact frozen launch environment before creating its PTY and binds that selected database to run-source provenance, so later restart checks retain the original source. Only an `opencode.db` or `opencode-<channel>.db` file inside the already selected data root is accepted. The explicit `OPENCODE_DISABLE_CHANNEL_DB=1|true` override proves the default filename without a probe. Unknown versions, failed probes, unexpected output, missing first-run data roots, and unproven run scopes report `SCOPE_UNKNOWN` for optional history or restart; the normal CLI launch remains available. Source keys include the selected filename, so two databases in one directory cannot share a history scope.

For a structured new opencode tab, the authenticated backend first uses the selected frozen profile and its environment to start a short-lived, password-protected loopback `opencode serve`, calls the official `POST /session` once, checks the returned ID and directory, then reaps the helper tree. When preparation succeeds, the tab starts the real TUI through `-s <id>`; its history row can then supply the current title and survive restart. An authenticated OSC title change or run exit refreshes that exact admitted project's history. A lost create response is not replayed under the same request ID. Preparation rejects nonempty extra/default arguments because their effect on the TUI's project directory cannot be inferred during API preparation. That explicit unsupported outcome, or the fixed `OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE` code proving no creation request was sent, permits a native New launch with the original default/extra argv and unknown identity. Generic failures, unknown outcomes, validation and authority failures never permit this fallback. Health probes have a short read timeout; creation has a separate bounded timeout and never retries or follows redirects. Pending receipts are retained, active-document outcomes remain pinned, and only settled receipts from revoked documents can be reclaimed. Raw launches keep their explicit argv semantics and do not claim this identity.

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

Adopting a changed or deleted profile immediately revokes its obsolete history
contexts, including pending reads. The workspace reloads only changed profiles at
their new revisions and refreshes availability without restarting the application.
The successor read waits for existing background refresh batches and retired
scans to finish, preserving the two-reader budget; successive saves keep only
the still-current revision eligible for the queued read.
Unchanged profiles retain their history; admitted tabs and runs keep their frozen
identity. Saving alone never launches or replays a failed launch.

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

Legacy mouse `onBinary` events are human input. Their unchanged eight-bit bytes
use the authenticated user-intent/staging path and the current input-allowed gate;
they are never suppressed as native-generated parser replies or paste-wrapped.

With the default-off `remote-host` build, supervisor adoption initializes the native
canonical producer before the original output reader's first read. The same actor
keeps canonical state and short journal transactions; one owned physical-IO worker
retains the original PTY mutation region for each complete frame or resize. While
IO is pending, canonical output and factual queries can progress, but snapshots,
input eligibility, resize and control transitions cannot overtake that mutation.
Stop retains the original result and worker join; it does not manufacture retirement.

The authenticated protocol route can return `state: "native-owned"` with zero
confirmed bytes and the exact run/generation. This means the browser reply had no
physical effect. New clients retain the original parsed frame's `streamEpoch` and
`throughOffset` as `protocolSource`, including across an asynchronous input wait;
the backend rechecks that source against the original run stream. An oversized
valid local resize is applied in full, without raising the remote terminal bounds.
Pure canonical/projection loss also revokes the peer and enters irreversible
local-only mode. The original canonical output cut suppresses delayed browser
replies for the native prefix; later authenticated desktop replies use the same
physical worker. Missing or mismatched source information fails closed in that
mode. Local input may continue after a durable Written/Rejected original outcome;
unacknowledged receipts, sequence floors and fences remain intact. Actual partial
native IO, source discontinuity and journal failure retain their fail-closed rules.

Resize returns `state: "resized"` and `canonicalOutput` (`complete`, `local-only`
or `unavailable`); the local-only diagnostic does not disable ordinary local input.

Native remote admission retains the exact current RunAccess, original resource Arc
and supervisor authority; a shutdown pin alone grants no admission. Successful
handoff consumes this temporary admission and registers the original lifetime pin;
rejection returns admission and producer custody while retaining the same Host.
The durable remote resize actor can retain a physical resize on its sole IO worker
through cancellation/expiry. It compares the adapter-reported size before publishing;
ConPTY's cached size is not independent OS evidence. The actual provider connection
entry and wire lifecycle route remain Locked/unregistered until their native adapters
are connected and qualified.
These capture facts are not remote admission. Identity provisioning, the production
verified connection entry and complete Job/process-tree retirement remain separate
release gates; this bridge does not enroll credentials or enable remote access.

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

An active process snapshot can already report `Exited` while those registry steps
are still converging. That observation is not an atomic registry-retirement
receipt. Tests observe the exact registry resource independently, with a bounded
deadline, before requiring its release without renderer ACK; a process snapshot
alone cannot establish that condition.

Caller stop carries `requestId` with the run identity. One registry lock admits that exact receipt, then the supervisor pins one active state and matches the receipt against its frozen request, owner, run and retained resource identity. A retired draining state remains bound to that pinned receipt, so delayed control from an old document or an evicted old request cannot be redirected to a newly adopted child that reuses the same run key.

## Observer boundary

Observer delivery is optional metadata, not process ownership.

`cli_get_observer_status` uses the authenticated document and exact request/run/generation. Its safe DTO reports `off`, `connecting` or `unavailable` plus a fixed reason: disabled/unsupported, waiting for events, missing service, unavailable plugin, preparation failure or generic unavailable. It describes the frozen installation result; only accepted hook events establish active observation. Frontend subscription immediately publishes the current state. Query failure cannot fail a successful launch, and a delayed query cannot replace newer activity. New Claude configurations default to observation enabled; explicit off and the original profile of resumed history remain authoritative.

For a machine-specific investigation, collect only: CC Desk and CLI versions; runtime/CLI kind; frozen profile revision and observer override mode/boolean; launch/run/generation matching booleans; installation state and fixed reason; listener readiness; whether an event was accepted, its allowlisted type and age; current observation/activity enums; plugin content-ready, service-ready and required shell-tool-available booleans; effective `disableAllHooks` boolean if independently verified. Do not collect transcript bodies, full settings/environment, arguments, port/capability values or secrets. A normal run on another machine does not certify the affected machine.

- observer credentials/capabilities are scoped to the exact run/document;
- only allowlisted bounded metadata reaches the owning WebView;
- prompt, response, env, credential, and raw error bodies are excluded;
- observer failure cannot kill, restart, or implicitly retry a CLI;
- Codex/raw/shell launches never receive the Claude observer overlay.

Structured Codex new/resume launches have a separate identity-only channel using
official session hooks. The reservation winner adds per-launch `-c hooks.*` and
exact handler hashes in the synthetic session-flags trust layer, before user
profile/extra arguments. It never writes user configuration, overrides
`features.hooks`, replaces `notify`, or stops a shared daemon. Codex 0.163 chooses
its official embedded app-server when these flags cannot be forwarded to the
implicit shared daemon; explicit user remote/disabled-hook configuration remains
authoritative and may leave identity unavailable. Raw and unarchive actions do
not receive this channel. The four Codex-specific environment values are stripped
from inherited inputs and minted for the exact owned run, generation and document.
Before accepting an optional observer overlay, the host validates the complete
invocation against the selected runner's argument and environment limits. An
unrepresentable overlay is discarded together with its capability; the original
frozen invocation still undergoes every ordinary launch check. In particular,
Windows Cmd shims retain their strict CALL literal subset and size limits, so
Codex hook TOML cannot prevent an otherwise valid new or resumed session from
starting. The audited official npm Codex 0.163.0 Cmd wrapper has a separate
per-launch argv transport: the selected Cmd runner and wrapper still execute,
while Node's supported `NODE_OPTIONS --require` preloads a fixed CJS module before
the official npm entry point. That module prepends only the four backend-generated
hook configuration pairs to `process.argv`; the official wrapper then forwards
them directly to its selected native Codex binary. Hook TOML never enters CALL.
The module first restores the original absent/empty `NODE_OPTIONS` and removes
the two internal `CC_DESK_CODEX_ARGV`/`CC_DESK_CODEX_NODE_OPTIONS` values, so neither
the native CLI nor subsequent Node tools inherit the preload. Both internal names
are stripped from inherited/profile inputs, including Windows case aliases.

This transport requires matching audited wrapper/entry bytes and npm package
name, version and bin metadata. Selected and entry paths must have direct canonical
spellings: junction/short-name aliases are not adapted. The backend holds the
audited files and their parent directories against writing/deletion, rechecks
paths and actual file identities after opening, and retains a private fixed-code
preload with the owned snapshot until run retirement. It does not change global
configuration or persistent permissions. The existing locked `tempfile`
dependency also owns isolated capability-probe configuration roots.

The audited npm transport admits exactly 0.163.0 and 0.164.0, with the same
normalized shim and entry SHA256 checks for both versions. The installed 0.164.0
package's entry bytes match 0.163.0; an isolated, credential-free `hooks/list`
exchange confirmed all three enabled/trusted session-flags command handlers.
This restores the authenticated identity path after that package upgrade;
titles, timestamps and newly discovered history never establish identity.
The native executable probe below retains its separate exact-byte policy.

Nonempty user `NODE_OPTIONS` are deliberately left untouched: Node permission or
loader options can fail before CJS executes. Unknown/custom wrappers, different
versions, nonliteral paths, preparation failures and size limits retain the
original frozen launch without this transport. This degradation leaves identity
observation unavailable and never selects another executable or writes a Codex
configuration file. A degraded new tab lacks the hook-proven native ID, so its
first-title synchronization and history deduplication cannot operate. Explicit-ID
resume keeps its original frozen historical source without gaining observation.
Codex discovers disk configuration hooks separately in each configuration layer,
so the added session-flags handlers coexist with user-file handlers. A later user
`-c` override in the same session-flags layer retains its normal precedence.

Native Node interpreter launches do not receive Codex flags before their script
argument. Native identity enhancement is separately admitted using verified
executable bytes and an isolated `app-server` `hooks/list` exchange, never a model
turn or a user configuration write. Currently only the audited Windows x64
0.163.0 native SHA256 is admitted for that probe. Unknown/native replacement
bytes, other platforms, custom wrappers and unsupported metadata disable only
identity enhancement; the original frozen new/resume invocation is used once.
This conservative boundary avoids executing an unknown wrapper as a capability
probe, which could ignore metadata arguments or leave unowned descendants.
The probe inherits only launch-essential environment entries, uses temporary
home/config/cwd, caps stdout at 64 KiB/128 lines, polls the pipe without a reader
thread for at most four seconds and reaps its direct child. All three trusted,
enabled session-flags command handlers must be present. Unknown extra fields are
ignored; unknown required trust/schema values are unsupported. This is optional
launch preparation, not availability preflight. Extending audited probe targets
requires real process-lifecycle and hook evidence for the new bytes/platform;
unknown versions remain usable with reduced identity features. OSC status has
independent admission and remains available.

The bounded reporter forwards only a main-thread UUID and allowlisted
SessionStart/UserPromptSubmit/Stop type. It never publishes prompt/response bodies
or treats Stop as activity completion. Windows hook commands preserve raw UTF-8
stdin across the CLI-selected shell, including executable paths with spaces.
Only admitted startup/input metadata can establish an unknown identity; an old
thread's asynchronous prompt/stop cannot replace a newer startup identity.
Unknown, retired, replaced and closed attempts reject late events. The identity
channel does not enable Claude activity observation for Codex.

The Desk-owned Codex reporter emits fixed-width boot-monotonic replay IDs with a
random suffix. Backend and tab keep at most 1024 recent IDs plus an eviction
floor: new IDs continue beyond 1024; duplicates and IDs at/below the floor are
rejected. Late IDs inside the retained window remain acceptable. These IDs are
not a CLI turn sequence and do not establish event causality. Legacy opaque
Codex IDs retain the conservative lifetime cap, while Claude replay and occurrence
notice contracts remain unchanged. Run/generation/document replacement discards
the corresponding window and rejects the old capability independently.

Codex TUI activity has a separate passive source: its native OSC 7501 program
status (`working`, `blocked`, `idle`, `clear`). The 0.163.0 TUI emits it on any
TTY; Desk does not require iTerm2-only OSC 21337 or alter terminal environment,
launch arguments, hooks or configuration. The existing exact-run contiguous
output stream and xterm parser deliver only allowlisted status enums to that
attempt. Background terminals keep observing; parser handoff/clear, output
degradation, unknown/end/restart/disposal invalidate the hint. Missing and unknown
payloads remain unknown. `blocked` is generic needs-action, and `idle` never proves
completion. These untrusted terminal snapshots have no thread/turn ID and never
authorize input, processes, history identity, permission decisions, unread replies
or taskbar receipts. See [status semantics](session-status-semantics.md).

An exact observed Codex ID joins its opened owner to the same authorized history
record, retaining one row across Refresh. Startup/input/stop invalidate only the
affected Codex history context. Since Codex generates its first title in a
separate hidden thread with hooks disabled, input/stop also schedule bounded
read-only retries after 1, 2, 5 and 15 seconds; restart, close, context changes and
replacement cancel pending work. Titles and cwd/time similarity never establish
identity or authorize a rename.

The Windows Codex 0.163 smoke uses the installed real TUI, bundled ConPTY and
production reporter with temporary configuration/project data and a local synthetic
Responses endpoint. It covers concurrent new identities, delayed automatic title
readback, one survivor after another TUI closes, resume identity, and reporter
paths containing spaces/non-ASCII characters. A shared-home, shared-project variant
checks ten production history refreshes with exactly one record per native UUID.
Host projection regressions replay its captured safe IDs, titles and authenticated
event metadata to check Refresh deduplication and late-event retirement. These
checks do not certify an installed WebView2 build or the account-backed D20 Layer-C
matrix.

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
- **Layer C** — real installed CLI behavior using an authorized test account. D20 scopes the four-cell matrix to Claude Code / Codex CLI; opencode has no Layer-C certification yet.

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

## Frozen source after process retirement

Opened Claude/Codex `resume-id` tabs may explicitly rename their retained historical
source, using fresh profile history authority only after matching frozen run root,
profile revision, registered project, native ID and source key. This target remains
the resumed source even if the CLI switches threads. It is never a proof of the
current terminal thread. A new Claude or Codex tab with a positively observed native ID
may explicitly target that historical record after the same fresh-root/profile/
project/key checks. Raw/picker and unproven new identities cannot infer a target.
`cli_rename_session` still rejects Run scopes. Codex official app-server
`thread/name/set` and Claude public Agent SDK 0.3.301 `renameSession` must read back
successfully before Desk clears matching aliases; uncertain writes are not replayed.

History omissions block rename only when they can conceal the requested identity.
The per-read backend budget keeps a monotonic anonymous-omission flag and omitted
native IDs; neither is a public DTO. Claude can exclude a different identity only
for a regular canonical `projects/{project}/{UUID}.jsonl` file. Codex requires a
successfully parsed authoritative first `session_meta`, never a rollout filename
or display-cache guess. Unknown subtrees, noncanonical/unidentified entries and
failures on the requested ID still reject rename. Full-source duplicate checks,
file pinning, project/cwd/root checks and official readback remain mandatory.
The menu uses the single **Rename** action; resumed-source/history-record target
details remain in the editor tooltip.
The adapter exposes rename capability only for an exact, readable source with a
current profile revision and registered project. Menu, F2, double-click and store
admission share that capability; execution still reacquires and validates source
authority. Unbound new/raw/picker sessions and unreadable sources omit Rename.
Claude project identity follows the first transcript cwd; later cwd changes do
not change the project, while a different first cwd or any different session ID
still rejects the target. Plugin hook commands quote the complete script path so
Windows configuration directories containing spaces preserve observer delivery.

2026-10-19 WSK-MSI regression: a valid Claude target plus an unrelated canonical
empty transcript reproduced `SESSION_ORIGIN_AMBIGUOUS` before the fix. The fixed
test executes the pinned official SDK, verifies the original file and refreshes
the title without changing that empty transcript. Guards cover same-ID duplicates
across projects, unknown subtrees, noncanonical names, target tail-read failure,
anonymous Codex failures and equivalent UUID spellings. The UUID spelling case
also produced RED before normalization. The 20 native-rename and 172 projection
tests passed under the host token; three original private-directory fixtures fail
under the restricted sandbox, so sandbox failure is not presented as a product
diagnosis. Frontend related coverage passed 289 tests, error/localization coverage
11 tests, plus typecheck, build, rustfmt and strict all-target Clippy. Independent
read-only review found no remaining blocker. The user's actual error code and
machine remain unconfirmed; these fixtures do not certify live CLI/WebView2 or
the user's histories. Anonymous omissions still block rather than guess identity.

Rename operation receipts share a 4096-entry budget. Before each new admission,
completed receipts belonging to permanently revoked full caller identities are
reclaimed through the authenticated run registry. Active-document receipts,
including unknown outcomes, and all in-flight operations remain pinned; exact
retries preserve their stored result and conflicting requests are rejected. A full
budget still rejects admission with `SOURCE_BUSY`; document replacement can reclaim
its revoked predecessor's completed receipts without restarting the application.

Claude metadata runs in a private same-volume namespace containing only the pinned
transcript hardlink and bundled helper. It needs an existing Node >=22.13 from the
frozen profile PATH; the app neither bundles nor installs Node. Unsupported filesystem,
path or runtime requirements fail explicitly. The helper clears inherited credentials,
limits filesystem access and never calls a model query. Node permission is not a
general network sandbox. Private files are cleaned before the helper owner settles.
Actual idle TUI restart evidence does not certify concurrent busy-writer behavior.

Display-only history folding compares full CLI/root/project/path/native-ID/session-key
identity across profiles. It preserves distinct opened process owners and archive
groups. It does not merge source authority or use titles as identity. Successful rename
refreshes every matching physical-source projection and clears only still-matching
tab aliases; a Legacy alias additionally requires a fresh same-root proof.

Exact run receipts retain a backend-only source provenance separate from PTY resources: CLI, profile revision and already-selected configuration/project paths, without env, argv or process handles. It is published atomically with the existing launch completion and only authorizes read scopes for the same caller/document/run/generation. Retirement still invalidates all old process-control and snapshot handles. Revoke, document reload and mismatched generations reject retained source access. This lets explicit structured restarts verify their original physical root after exit; raw restarts keep their explicit rerun semantics.

## OpenCode HTTP title updates

List rename targets a freshly revalidated configured-profile history source, never a
Run scope or a guessed newest session. Opened resume/new records require their exact
frozen root, profile revision, project, native ID and source key; unbound/raw, archived,
failed-read and incomplete OpenCode sources hide Rename, F2 and double-click editing.
The backend rechecks the selected SQLite filename/channel and source-sidecar boundary,
then starts an owned private authenticated loopback server from the selected CLI and
frozen environment. Only official GET/PATCH/GET session metadata calls update the
title, with exact ID/directory and title readback. PATCH is issued once; uncertain
results never replay or fall back to direct SQLite writes or a Desk-only alias.
The existing title receipt and affected-source refresh path synchronizes list aliases.
Official OpenCode 1.19.0 was checked through isolated PATCH/list/server-restart and
fresh CLI list reads without model calls. This evidence does not certify a logged-in
interactive model turn or unrelated external/unregistered sessions.

## OpenCode optional activity observer

Structured OpenCode runs can load a private, per-owned-run plugin through the official
`OPENCODE_CONFIG_CONTENT.plugin` tuple mechanism. It preserves existing inline settings
and user plugins; it does not install a plugin globally, inject Claude flags, poll an
unrelated `serve` process or change native input. The selected executable/runner has a
bounded, isolated, owned `--version` probe. Only the examined **1.19.0** contract is
enabled; older, future, unrecognized or unavailable versions retain their original
basic CLI launch with an unsupported optional observer. Explicit observer off/unset,
raw launches and `OPENCODE_PURE=true/1` do not receive an enabled observation channel.
Invalid/oversized inline configuration or an unrepresentable optional overlay also
falls back to the original frozen launch once, without loosening admission checks.

The plugin uses the SDK passed by the actual CLI instance. It verifies main-session
metadata, excludes subagents and foreign IDs, and binds the requested resume ID or the
first verified main-session ID in the selected new-run directory. Initialization
returns hooks before querying the instance API, avoiding an initialization self-wait.
Official `dispose` clears initialization and prevents pending metadata callbacks from
publishing after instance disposal. The observer lease owns and removes the private
plugin directory, and exact caller/run/generation/capability still authorize delivery.
Only allowlisted session identity and status metadata enter the public DTO; prompts,
tool arguments, retry/error messages and credentials are excluded.

| Official event evidence | Desk activity | Completion claim |
| --- | --- | --- |
| `session.status` busy / retry | working | None |
| `session.status` idle / deprecated `session.idle` | idle | None |
| `permission.asked` with an outstanding request ID | waiting_permission | None |
| `question.asked` with an outstanding request ID | waiting_input | None |
| `permission.replied`, `question.replied/rejected` | Remaining requests, then last known status | None |
| `session.error` | error, retained across the following idle until busy/retry | None |
| Missing/future status | unknown | None |

The verified 1.19 cancellation path removes pending requests without a reply event and
publishes idle; idle retires this turn's request IDs. Error causes remain independent
because the official processor also emits error followed immediately by idle. Waiting
does not expire merely because the user takes over 90 seconds to respond. Other stale
OpenCode activity becomes unavailable/unknown after 90 seconds; silence never invents
idle, success or completion. Closed/restarted/invalidated run owners reject late events.

Only relevant event types enter the serial queue; token deltas are ignored. The queue
and pending sets are bounded at 128, opaque upstream replay and retired-request ledgers
at 4096. Exhaustion stops this optional channel with unknown status instead of evicting
old identities and accepting replay. Authenticated published events carry a monotone
per-run sequence; backend and frontend reject late/duplicate delivery beyond 1024 events.
All providers reuse `SessionAppIcon` and `SessionStatusIcon`; observed OpenCode idle gets
a static badge, unknown a neutral question, permission/input/error their shared shapes.
Raw and explicitly unsupported/off channels keep neutral running presentation.

Evidence: official source commit
[`733b2c6`](https://github.com/anomalyco/opencode/tree/733b2c627e5aec39cb33cc22373bdce33053ef2c),
particularly `plugin/index.ts`, SDK v1 `sdk.gen.ts`, `session/run-state.ts`,
`session/processor.ts`, `permission/index.ts` and `question/index.ts`. The explicit
`node tests/scripts/opencodeObserver.real.mjs <installed-opencode.exe>` fixture ran the
installed 1.19.0 CLI and production plugin in private HOME/XDG roots: real creation,
identity isolation, abort/idle, HTTP rename, server restart and persistent title passed
without model calls. Request/error/reordering/overflow/dispose cases are isolated
production-plugin tests; they are not authenticated model-turn evidence. Browser
light/dark status rendering is synthetic Chrome evidence, not native Windows WebView2.
