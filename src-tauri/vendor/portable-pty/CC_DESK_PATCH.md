# CC Desk local patch

Based on the exact crates.io portable-pty 0.8.1 package selected by Cargo.lock.
Original license and .cargo_vcs_info.json are retained. No other crate is patched.

Only src/win/conpty.rs differs: the taken input writer owns the existing
descriptor and implements a real FlushFileBuffers drain on its input pipe.
Unix and output readers remain upstream. No raw handles are exposed to the app.
The same owned writer is used in production and native regression tests.

FlushFileBuffers may block until the console host consumes pending bytes,
just as a synchronous pipe write can block. The application holds only the
affected PTY's writer lock; its global registry and kill path remain independent.
This does not acknowledge receipt by Claude's editor. See docs/paste-framing.md.

Remove this local patch when upstream exposes equivalent pipe drainage or
the supported console hosts reliably preserve bracketed paste input.
