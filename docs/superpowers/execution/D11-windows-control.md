# D11 Windows termination result correction

Base `de71673de4dee79ff4558e4dba2f3a5a4b41e7c0`, existing PR #19. No new branch or old terminal route switch.

## Evidence before correction

CI #170 (`35818853237`, head `a3e210c3`, merge `840cbd8dcb140e66d10e1ef84e01bdc4e357639c`) compiled and ran the owned-resource implementation: 480 library tests passed, one failed, 15 ignored. All six route lease tests and five of six real-PTY tests passed, including 100 concurrent requests producing exactly one child report, raw stdin and trailing output. Root control returned an error promptly rather than timing out; the test kept its success assertion. Frontend 640 tests, typecheck/build, three Node policy tests, Rust formatting and Clippy passed.

The pinned portable-pty 0.8.1 source was checked, not the latest crate: [version declaration](https://github.com/wezterm/wezterm/blob/20240203-110809-5046fc22/pty/Cargo.toml) and [WinChild/WinChildKiller](https://github.com/wezterm/wezterm/blob/20240203-110809-5046fc22/pty/src/win/mod.rs). Its cloned Windows killer treats nonzero TerminateProcess as Err and zero as Ok. [Microsoft's native contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess) specifies the opposite; a call on an already terminated process fails even while its handle remains valid.

Added negative test D11_Owned_WindowsTerminateFailureMustNotSucceed_07 before correction. CI #171 (`35819388808`, merge `a64f829eacb84b25893b9e299db8b119fc85af20`, Rust job `107047789721`) was read in full: 480 passed / two failed / 15 ignored. The new test observed an incorrect Ok on the exited child, while the existing live-child test again observed incorrect failure. Formatting, Clippy and the frontend job passed. The test-only diff added 15 lines; no old assertion was removed or weakened.

## Correction and ownership ruling

Ruling: fix native BOOL interpretation inside the new owned layer, without upgrading portable-pty, patching dependencies, swallowing kill errors, or searching by PID. Windows calls TerminateProcess directly using the original handle retained by its private Child. The immutable opaque handle is stored in AtomicPtr for cross-thread access, never exposed to callers and never closed by that field. Every call borrows OwnedPty, retaining the owning Child; that field is never replaced/extracted. The pinned native WinChild retains its original process handle after wait, which uses a duplicate. Future child extraction or dependency changes must revisit this invariant.

No fallible duplicate-handle allocation is added after spawn. A non-native child that cannot supply a process handle reports PROCESS_CONTROL_UNAVAILABLE, stays owned, and does not fall back to a PID lookup. Native API failure returns the existing fixed PROCESS_TERMINATE_FAILED code without leaking OS payloads. Accepted termination still requires a separate wait/reap; no success is inferred for already-exited processes. Unix keeps its existing retained ChildKiller route.

The correction applies to OwnedPty only; the original application PTY route remains unchanged. Live document-lifetime authorization, start/query/control IPC, autonomous waiter/drain/stop ownership and old-path migration remain unfinished; no Codex user entry or whole-D11 completion is claimed.

Exact successor CI is pending. Final results and author review will be recorded against the tested head in PR metadata, not by appending an unverified documentation successor. No main write, merge, release, version/lockfile change, native user configuration mutation or independent-review claim.
