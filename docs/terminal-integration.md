# Terminal integration

CC Desk contains a legacy Claude terminal path and the Native CLI v3 path for Claude Code and Codex CLI.

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
