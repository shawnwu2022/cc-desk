# Terminal integration

CC Desk contains a legacy Claude terminal path and the Native CLI v3 path for Claude Code and Codex CLI.

## Unified host

The normal App mounts `UnifiedTerminalHost` in its only Workspace terminal slot.
The host keeps every open Native terminal and one Legacy aggregator mounted across
selection, navigation, and GUI-theme changes. Only visible terminals fit/focus;
background output and Native protocol replies/ACK remain live. Hidden Legacy
keyboard, paste, IME, copy, drag/drop and compatibility-command input are rejected.
The existing pinned xterm user-input provenance signal preserves parser replies
without classifying payload bytes. xterm `disableStdin` is never used for hidden
terminals because it suppresses parser replies before onData. Real pinned-xterm
parser/component tests cover DSR response generation with initial and changed
visibility. Ended Legacy terminals retain scrollback. First Legacy spawn waits
for output/exit listener readiness, independently of optional drag/drop, and then
rechecks the original tab owner.

`useUnifiedWorkspaceRuntime` supplies the real adapter ports and read-only bootstrap.
It admits typed shell requests once by sequence. Current creation/resume dialogs, project/resource stores and typed confirmations own preparation and authorization before existing adapter dispatch.
Native stop/recover receives the request/run/generation captured by the caller.
Restart and close revalidate ownership after awaits; unknown status reads remain
unknown and never authorize replay. Queued facade actions capture ownership before
waiting. A local Legacy PTY generation prevents a late start/stop/restart/archive
from modifying a replacement instance. These are host/unit-tested boundaries,
not real CLI or platform certification.

## Native CLI v3 — forward path

```text
NativeCliTerminal (xterm)
        ↕
authenticated document bridge
        ↕
launch_service / terminal_input / terminal_transport
        ↕
owned PTY + run supervisor
        ↕
Claude Code | Codex CLI
```

The Native CLI frontend must not fall back to legacy `ptySpawn`, `ptyInput` or `ptyKill`.

## Launch identity

Every native tab freezes:

- CLI kind;
- registered project;
- profile ID and exact profile revision;
- request ID;
- run ID;
- generation;
- launch action.

Supported actions:

- new;
- native resume picker;
- known native session ID;
- exact raw argv.

Raw argv is represented as JSON `string[]` and preserved as argument boundaries. It is not parsed with whitespace splitting.

An uncertain launch is recovered by querying the original request. CC Desk never treats uncertainty as permission to spawn a second process.

## Input transport

Native input is ordered before async work begins.

The queue reserves monotone intent sequence numbers so keyboard, IME, paste and protocol traffic cannot overtake each other merely because one path is asynchronous.

Large input uses:

```text
begin -> chunk* -> commit
```

Properties:

- bounded action/run staging budgets;
- one exclusive writer region for a committed frame;
- host-written and partial-or-unknown receipts are distinct;
- partial/unknown freezes later user input;
- no automatic replay after partial/unknown;
- protocol bytes share the PTY writer lock;
- ambiguous xterm `onData` bytes are not classified by DSR/DA content heuristics.

Clipboard arbitration preserves bracketed-paste framing and requires explicit handling for unsafe ambiguous multiline/escape payloads. Image-only handling uses a proven native CLI/system capability rather than synthesizing text.

## Output transport

Output is emitted as bounded binary frames with exact offsets.

The frontend acknowledges only parsed contiguous frame boundaries.

The backend validates:

- caller ownership;
- run ID;
- generation;
- stream epoch;
- monotone ACK;
- ACK not beyond sent data;
- ACK on a known frame boundary.

Backpressure includes per-run high/low watermarks and a global payload budget with FIFO waiter progress.

Route revocation releases reserved credit and wakes blocked peers. One degraded stream cannot release another run's credit or poison its transport.

## Resize / stop / exit

Native resize and stop go through authenticated native commands.

Natural exits are adopted by the exact run/generation. Restart is explicit and increments generation with new request/run IDs.

Observer or UI diagnostic degradation is not process failure.

## Terminal protocol replies

Terminal-generated replies must have source provenance. Byte patterns in ambiguous user `onData` are not guessed to be protocol traffic.

Where xterm/public APIs cannot prove provenance, that capability stays blocked rather than using content heuristics.

## DOM and error boundary

Native terminal and structured resource surfaces are interpolation-only:

- no `v-html`;
- no `innerHTML`;
- no payload `console.*` / `logMessage` path.

Public native failures are mapped to fixed safe codes. Raw serde errors, native paths, environment values and arbitrary exception messages do not become UI diagnostics.

## Legacy Claude terminal

The existing `XTermTerminal.vue -> pty.rs -> Claude Code` path remains for compatibility and legacy project/session workflows.

It continues to use the older Claude-specific environment/check/hook integration. Do not extend this path for new Codex/native features.

## Windows ConPTY

Windows packages a verified private ConPTY runtime and fails closed when that runtime is missing/corrupt rather than silently falling back to the known-bad path.

D21 includes installed-runtime evidence for the tested Windows Server 2022 target. That result is target-specific.

## Evidence

Host/unit/OS CI validates transport and lifecycle mechanics.

Real installed Claude Code / Codex CLI behavior belongs to D20 Layer C and remains BLOCKED until an authorized target environment runs the certification matrix.

## Task 12: preparation before Native admission

The unified catalog creates a synthetic starting row synchronously before invoking
its configured creation preparer. The normal runtime preparer selects/freezes the
launch configuration and ensures registration using `workspace.ensureRegistered`.
Only an explicit creation can call this helper or prepare the `desk-safe-claude` /
`desk-safe-codex` configurations through existing profile patch/CAS APIs. Safe
configurations use a native launcher, empty default argv/env, disabled observer, and
standard Claude permission checks; Codex retains its own permission semantics.
Read-only bootstrap never registers a project, creates a configuration, or starts a
CLI. The existing availability endpoint is a read-only filesystem/config preflight.

Preparation failures retain an addressable failed row with a fixed safe error code.
Retry is explicit, and unknown mutation outcomes reload state without replaying the
write. Local preparation/session controls dispatch even while unrelated bootstrap is
pending. Cancel/close invalidates the preparation owner; its late completion cannot
admit a Native tab. Close cannot discard an admission already in flight. After
admission the real Native tab owns launch/recovery/stop; no new runtime protocol or
Legacy fallback is introduced. A Native tab with `stopped` plus no launch receipt is
projected as starting until the existing terminal starts it; its unstarted admission
can be explicitly cancelled with an exact-owned close.

Creation/retry freezes profile revision as well as the existing request/run/generation.
Only a matching `running` receipt with launch revision records project+CLI success
preference through canonical `projectsState.setLaunchPreference`, preserving the
other CLI field at serialized write execution. Failed preference persistence reloads
metadata under canonical writer queue ownership without repeating the write, cannot
fail or restart the running process, and is caught separately from launch handling.
A recovery read failure marks the snapshot unverified; a queued update must obtain
a new authoritative read or fail before mutation, preserving other CLI fields. Tab creation, unknown outcome, failure, preflight availability, stale
receipts, and replaced attempts cannot record success. Older preparation completion
cannot steal unified selection or clear a newer shell request. Explicitly selecting
the same placeholder updates its selection-intent owner; admission transfers that
latest selection to the real row, but cannot supersede a newer selection elsewhere
or a newer in-flight activation. Selection intent is separate from the existing
lifecycle-invalidating epoch, so closing an unrelated ended row cannot revoke a
still-selected placeholder’s transfer. Close/archive stale-action checks are unchanged. The Task11 hidden
parser/protocol path, user-input provenance gates, Native authenticated bridge,
Legacy core-listener readiness, terminal binding, and no-replay behavior are unchanged.

## Task 13: source-bound restore admission

Historical restore bypasses the new-session preparer. The Native adapter freezes
CLI/profile ID/profile revision/project ID/path/sessionKey/native Session ID, performs
a fresh read from that exact authenticated source, rechecks for an existing owned
attempt after the read, then asks the existing runtime to admit the tab. The runtime
checks the frozen profile revision and registered project again. A changed or missing
origin fails closed with configuration guidance; no default substitution, registry
mutation, profile mutation or Legacy PTY fallback occurs.

Same-origin concurrent restores share one in-flight admission. Existing Native tabs,
including starting and unknown attempts, are activated without allocating new
request/run/generation values. Different CLI/source/profile/revision/registration
identities do not collapse merely because a raw Session ID is the same. Legacy
restores similarly coalesce and recheck project-scoped active claims after reading.
The caller's cancellation guard prevents a delayed history check from admitting a
tab after dialog dismissal/navigation. Unified selection epochs still prevent older
admission from stealing a newer selection.

Direct-ID and native-picker restore use the already supported launch actions, an
explicit configuration revision and explicit registered project identity. Their
saved CLI settings remain authoritative; picker interaction and final Session ID
availability are handled by the CLI. Host tests do not certify actual CLI restore
behavior. Task11 parser/provenance/readiness, exact attempt ownership, authenticated
transport and input/output no-replay paths are unchanged.

Review repair: an exhausted offset page chain is not a stable absence proof. Only a
single complete authenticated response with the original sourceRootKey/sessionKey
can authorize missing-record cleanup; changing root identity or multi-page negative
results fail closed. Positive source-bound records are still usable. Coalescing uses
independent per-caller cancellation ownership: a later explicit confirmation may
own one admission, and canceled callers cannot publish its result as their own.

## Task 16: consequential action ownership

The UI never interprets confirmation as evidence that an unknown Native launch is
safe to repeat. For restart-unknown, the production runtime first calls the existing
`recoverNative(tabId, exactAttempt)`, rechecks full ownership, refuses a still-unknown
state, stops a known live attempt through `stopNative`, and requires a definite ended
state with a launch receipt before the catalog's normal restart route. The existing
Native terminal already verifies the stop receipt; no transport or retry protocol
was introduced.

Confirmed close/archive freezes adapter ownership when the dialog opens and again
when the queued action is admitted. Guards cover Native request/run/generation and
profile/project/source/action identity. Legacy close awaits the existing exact-PTY
stop before removing the tab; archive and async rename recheck the captured PTY and
generation after awaits. Cancellation or replacement can leave an already-stopped
original session open, but cannot close/archive a replacement or start a late run.

Feedback ownership is separate from process ownership. A completed authorized action
may still update its canonical source, but its stale success/error cannot be published
for a newer user selection, shell request, navigation surface or runtime attempt.
Canonical archive/restore/UI-record mutation recovery remains in `projectsState`'s
single writer queue. Profile deletion similarly serializes read-only reconciliation
within its own existing profile CAS queue. No successful reload automatically repeats
a side effect, and no new Native code falls back to Legacy PTY commands.

Writer admission is the final synchronous boundary before IPC, after any canonical
queue wait and initial metadata load. Both archive adapters pass an optional
`beforeMutation(): void` guard through `projectsState.archiveSession`; the Legacy
session store forwards it unchanged. The guard throws before mutation error handling,
so a canceled or replaced owner causes no archive write, readback, or writer error.
Issued writes retain existing read-only reconciliation and are never replayed.
Project removal similarly guards `workspace.remove` inside its CAS queue, then guards
queued `unpinProject` against cancellation, open sessions, and any replacement
registration. Prior completed steps may remain (stopped session, hidden/unregistered
project); cancellation never invents compensation or deletes project/history files.


## Saved configuration deletion and retained runs (Task 19)

Deleting a saved configuration is a profile-repository CAS mutation and has no process
side effect (`cli/profile_service.rs` → `cli/storage.rs`, Delete arm). Run resource
scope in `cli/native_projection/service.rs` uses the retained launch snapshot and
checks the same Arc through exact run/generation access; it does not reload the saved
profile. Profile scopes still require that record and exact revision. This is source
inspection evidence, not a new Rust or real-CLI certification run.

The frontend admits deletion for an existing tab only when `nativeTabs` holds positive
receipt evidence bound to its full attempt/profile/project/source/action identity.
Local status strings or a non-null revision alone do not establish that evidence.
Starting attempts, unadmitted tabs, uncertain attempts without a prior trustworthy
receipt, and unified preparation before tab creation block deletion. Preparation with
no selected configuration conservatively blocks deletion within the same CLI until it
resolves. Queue admission rechecks the same profile and workspace CAS revisions.
Saved edits/deletion never change current tab identity or terminal mount; restart or
historical resume with a missing/changed original configuration fails closed with
configuration guidance. No substitute default is chosen for those historical actions.

The last-default ruling is deliberate: delete clears the stale global UI preference
and selects another existing same-CLI configuration if available. If none remains,
the UI explicitly describes the safe fallback; only a subsequent explicit New session
materializes it via the existing Task12 preparer. Deletion does not create a second
configuration or process, and project last-success metadata is untouched.


## Shared appearance without runtime replacement

Legacy Claude, Native Claude and Native Codex consume the same computed
`app.terminalPreferences` through `terminalAppearanceOptions` / `applyTerminalAppearance`.
Theme and cursor deltas update existing xterm options without resize, terminal
recreation, process launch, scrollback/selection mutation or input. Font metrics
request one coalesced visible fit; hidden terminals defer fitting until shown and
recheck lifetime/visibility at the frame boundary. A new renderer choice affects only
new terminals. WebGL load/context failure retains the terminal and latest colors.

The fixed Settings preview opens no xterm, bridge or PTY. GUI light/dark × terminal
light/dark independence, hidden parser replies, exact output ACK and selection/host
retention have frontend/real-pinned-xterm tests; actual CLI, GPU and OS glyph behavior
remain separate. Full preference persistence rules are in [terminal preferences](terminal-preferences.md).

## Rename, stress and final evidence limits

Unified history/live rename updates only `projectsState` display metadata. It neither
sends Legacy `/rename` nor edits CLI transcripts. Canonical queue admission rechecks
history identity and Native/Legacy attempts; late completions cannot name a replacement.
New/raw Native tabs without authenticated history association retain tab-local names;
no title/raw-ID/default inference associates later discovered history.

Task 24 host tests cover 50 projects × 100 mixed sessions, 30 open descriptors and 120
state/selection/layout cycles. They do not spawn 30 real CLIs or measure browser
throughput. The current Task 25 frontend build/typecheck/full-suite result belongs to
[U01–U10](superpowers/execution/U01-U10.md); earlier D21 installed ConPTY evidence is
historical and cannot certify this new UX package. Rust is NOT RUN locally, rendered
screenshots remain BLOCKED_VISUAL, and D20 remains BLOCKED_EXTERNAL_TARGET.

## Native activity projection after final review

Native launch receipts update `lastActivityAt` only when launch/state/error meaning changes; identical healthy polls neither refresh activity nor clear a transport diagnostic. Accepted output and successfully admitted user keyboard/explicit-text/paste operations report activity through the existing terminal binding. Parser replies and ACKs do not count as user activity. The terminal checks its exact request/run/generation and binding lifetime before publishing, while `nativeTabs.touch` coalesces reactive input/output timestamps to at most one publication per second per attempt. No delayed timer keeps idle rows recent.

Optional attention consumes the existing exact-run observer's projected state. Only an active ordered `waiting` projection means `needs-user`; unordered/raw events remain unknown. Repeated projected attention does not refresh timestamps, and unknown/failed/starting/stopped/replacement state clears previous attention. Subscriptions are passive and released with the run binding; they do not enable the backend observer or control a CLI. These are host/source contracts, not D20 real-CLI or throughput certification.

During a pending launch receipt, a starting tab retains one latest safe attention projection bound to its exact request/run/generation. It displays needs-user only after that same attempt is confirmed running. A newer unknown projection replaces waiting; unknown/failed/ended process state, restart, close and clear discard the pending projection. This bounded temporary state contains no raw observer event and cannot establish process state or input authority.
