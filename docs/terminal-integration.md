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
It admits typed shell requests once by sequence and leaves create/resume, project,
resource and consequential unconfirmed requests pending for their owning tasks.
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

Native terminal/workbench surfaces are interpolation-only:

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
