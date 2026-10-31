# Staged owned-Job launch custody

This backend-only Windows seam is default off. It does not register a remote
provider, IPC command, identity factory, or ordinary desktop Job launch.
`NativeRuntime::stage_owned_job_start` accepts only backend-owned resources;
the intended pair is the pinned `native_pty_system()` pair and the Job is newly
created/owned by that backend. No serialized handle or PID is accepted.

The runtime registers one custody slot **before** entering CreateProcess. The
slot retains the original PTY pair, Job handle and maintenance admission ticket.
After process creation, the typed failure retains the original process and
primary-thread handles, assignment and resume-attempt facts. Assignment/reap
failure is not a never-created result. Neither a Copy classification nor an
ignored Result may release these resources. Duplicate/busy/closed starts are
rejected before entering process creation; no automatic replay is provided.

Only an exact original process-handle signal settles the child maintenance
ticket. An assigned Job additionally requires `ActiveProcesses == 0` while the
same original Job is still held. Root signal and Job-member zero are independent
facts. The original blocking application shutdown worker performs cleanup and
PTY destruction; `HostNativeShutdown` retains/observes the same slot even when
there are no published NativeRun pins. Busy/drop-in-progress remains Pending.
Query/termination error remains Unknown, preserving resources and blocking exit.
Explicit reconciliation may query originals again, but a termination request is
not blindly reissued. A resolved cleanup does not rewrite an uncertain launch
into a successful command result.

An unexpectedly abandoned unresolved slot quarantines that **one runtime's**
custody for process lifetime rather than closing its only handles. This last
resort is not retirement, recoverable coordination or a global resource budget.
Normal application exit must pass the original owner gate instead.

CreateProcess and ClosePseudoConsole are not covered by a preemptive deadline.
This staged slot has no output-reader/drain handoff. Before live use, it must
reuse the existing writer/reader/drain and original-I/O lifecycle; otherwise a
large-output process can leave ClosePseudoConsole waiting while exit remains
Pending. Job-member zero does not prove OpenConsole, broker/service, kernel I/O,
terminal parsing, real AI CLI compatibility, or phone end-to-end acceptance.

Qualification uses only newly created test executables and their fixed
descendant, without user CLI/accounts, global process lookup or breakaway.
Two ignored worker tests are entered only by the exact self-fixture. The native
unexpected-resume test increases the original suspended **test** thread's count
to two, then exercises the same production ResumeThread check: a real return of
two must produce an owned Unknown and cannot become success. It does not claim
an organically occurring OS permission/termination failure.

The application patches the same upstream portable-pty 0.8.1 to the repository
vendor source. The ordinary `spawn_command` body, legacy Copy qualification
entry and license remain; no CLI version gate or ordinary size restriction is
introduced. The Cargo lock removes only the registry source/checksum for that
path dependency. No application user-database migration is involved.
