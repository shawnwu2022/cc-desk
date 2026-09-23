# D10 implementation checkpoint

Base: RED scaffold 69dc80726541dc90e0aa6911d8ba079735d1ce02, PR #18.

## Observed RED

CI #159, run 35805102360, tested merge c9ba392768ba733e4f3beba3308e93df109e76f2. Rust job 107004046476 compiled successfully, then 421 passed / 13 failed / 15 ignored. Every new top-level D10 case failed at the intentional PLATFORM_LAUNCH_NOT_IMPLEMENTED boundary. The environment-removal parent also invoked its isolated worker, which failed at that same boundary. Strict Clippy passed. Rustfmt requested layout changes in the new test and stub files. Full Rust log was read before implementing behavior. Frontend job 107004046315 passed.

The six additional passing library cases relative to D09 are existing runtime tests included by the test bootstrap, not six additional launcher features.

## Implemented candidate

ProcessLaunchSpec owns selected paths, constructed OS argv, cwd and the full child environment; no CLI kind is stored. CommandBuilder's base environment is cleared before the complete map is installed. Missing program, runner, or cwd fails with a fixed error before normal command construction. PTY allocation and child spawn return fixed errors without raw launch values. The parent drops the slave endpoint immediately.

Bash passes values as positional argv to a fixed exec script. On Windows only the selected program/interpreter path is adapted to Bash path syntax; user arguments are not path-normalized. MSYS2 argument conversion is disabled in the wrapper. Noninteractive interpreter startup can still honor user BASH_ENV; this is not a sandbox.

PowerShell serializes program and argv as JSON transported in Base64 data, decoded by a fixed wrapper. No user string is interpolated as PowerShell syntax, including smart quotes. The wrapper requires Standard native argument passing (>=7.3); it does not add an execution-policy bypass. Old selected PowerShell exits 125 before starting the target. Native failures use fixed wrapper errors. Explicit selected scripts may contain their own behavior; they are not rewritten.

Cmd is Windows-only and uses CALL with a conservative literal subset. Expansion, shell operators, quotes and control characters are rejected rather than guessed. Automatic Native execution of Windows batch/PowerShell scripts is rejected; choosing an interpreter remains explicit.

Ruling: startup/PTY registration and legacy shell-text migration remain incomplete at this checkpoint. The old platform code is retained byte-for-byte. This commit does not claim D10 completion, live CLI compatibility, Unix acceptance, or installable release readiness. D11 owns run lifetime; no new IPC spawn is exposed here.

Status: implementation candidate submitted for CI; no GREEN claim. New test assertions are unchanged, only formatted.
