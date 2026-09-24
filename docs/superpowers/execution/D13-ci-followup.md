# D13 complete-suite follow-up

## Full-suite precondition follow-up

CI #230 on `5ed67be2` did not pass: attempt 1 timed out in the unchanged D11
native WebView launch fixture; attempt 2 passed that exact fixture but failed
D11_Owned_WindowsTerminateFailureMustNotSucceed_07 (629 passed / 1 failed / 20
ignored in both attempts). All D13 cases and D12 native WebView checks passed in
both attempts. Formatting/strict Clippy passed; application loader was skipped.

The second failure exposed a fixture precondition: portable-pty 0.8.1 try_wait
reads GetExitCodeProcess without waiting for the process object to be signalled.
The Win32 ExitProcess documentation lists setting the exit code before signalling
the object; TerminateProcess documents waiting on the process handle to ensure
termination. The fixture now uses its already-owned process HANDLE and a bounded
WaitForSingleObject to require real termination first. Both the expected
PROCESS_TERMINATE_FAILED and retained original exit-code assertions are kept;
production termination/wait code, dependency versions and the full-suite command
are unchanged. The synchronization helper is test-only and does not expose an
application API or look up a PID. Fresh complete-head CI remains required.

References: Microsoft Learn TerminateProcess and ExitProcess API documentation.
The separate D11 WebView timeout remains an unexplained intermittent observation,
not a defect claimed fixed by this test-precondition correction.
