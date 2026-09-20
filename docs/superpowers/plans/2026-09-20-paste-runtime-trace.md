# Paste runtime boundary probe

Goal: diagnose the actual CC Desk input path without changing paste bytes or adding async waits before input dispatch. This is a diagnostic build, not a field bug fix.

Scope approved in conversation: correlate normalized clipboard, frontend payload and Rust body; observe intervening PTY inputs without recording business text. Default builds must have tracing disabled.

- [x] Add failing tests for transaction association, interleaved input order, bounded lifetime/event counts, no retained clipboard text and unchanged write errors.
- [x] Implement synchronous metadata attachment in the production clipboard and IPC path. Send an optional normalized reference over the existing IPC only in diagnostic builds. Backend compares text in memory and logs only booleans, sizes and first-difference offsets. No clipboard text/hashes in logs.
- [x] Reuse original `commands::pty_input` unchanged through a narrow command wrapper. Do not rewrite markers, delay writes, retry, suppress errors or change shell selection.
- [ ] Compile/test both the default and diagnostic gate on Windows, build a standalone diagnostic executable, verify artifact bytes before handing over.

Explicit boundaries: this cannot determine the affected machine's first differing byte without one execution there. It is not a claim of a fixed ConPTY/CLI defect. Input receive order is not per-writer lock-acquisition order. Metadata is diagnostic, not an authorization mechanism. Submit hooks, external editor contents and keypress text are not collected by this instrumentation.

Local verification: 16 Node tests pass. Red controls: initial metadata stub 5 failures; original clipboard path 4 failures; original API wrapper 1 failure. Source copies were verified against their Git blob SHAs. Local environment has no Rust compiler or Windows runtime; Windows compilation/binary verification still required.
