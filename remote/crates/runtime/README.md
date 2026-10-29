# Native runtime: E06 Windows ownership candidate

This crate is a small backend-only process/Job primitive, exercised on Windows10.
It has no network DTO, CLI selection, credential, grant or document authorization.
BackendLaunch owns a backend-created std::process::Command; neither it nor OwnedRun
implements Serialize or Debug. Its constructor does not confer permission. E08 must
build this from the existing admitted immutable ProcessLaunchSpec, with complete
env_clear/environment and explicit executable selection; that bridge is not present.
The existing desktop launch/runtime/ConPTY paths have not been changed.

OwnedJob::launch creates an unnamed, noninheritable kill-on-close Job without
breakaway flags, creates the child suspended, assigns that exact owned Child handle,
then resumes only its one discovered thread after verifying thread ownership. The
thread snapshot scan has a65536-entry cap and500ms soft elapsed guard started before
snapshot creation, with another check immediately before resume. Windows kernel
calls cannot be preempted by this guard; it is not a hard500ms Win32 deadline. The newly
created suspended child cannot have its PID reused while its handle is held. This
kernel primitive follows existing opencode_prepare::server_job, not its account,
HTTP preparation, admission or launch policy. E08 should share one reviewed primitive
with desktop ownership rather than leaving two parallel launch policy implementations.

Assignment/resume failure kills only the just-created owned child and closes its Job.
AssignmentFailedReaped is returned only when the never-resumed root exit is confirmed.
Every resume-stage failure returns OutcomeUnknown even after confirmed cleanup:
execution may already have happened. Enumeration ends successfully only on the
specific ERROR_NO_MORE_FILES result; other errors remain unknown. No retry or automatic
relaunch is performed. No API accepts arbitrary process IDs for adoption
or termination. Child pipes are owned by the run and can be taken explicitly.

stop closes the Job and requests owned-child termination, then polls try_wait against
an absolute monotonic deadline without holding or draining stdout. This is cooperative
OS cleanup, not a promise that Windows syscalls/driver cleanup are preemptible or
real time. Drop closes handles/requests termination but does not claim a successful
reap or block waiting on unread pipes. stop/try_wait return RootExitOnly with a
root_status ExitStatus: this confirms only the root process handle, not descendant
quiescence or completion of earlier Host IO. Closing the Job requests tree termination;
E08/E09 must not publish a new generation or reuse pipes solely from RootExitOnly.
No tree-quiescence barrier is implemented here. The descendant-handle test is an extra
observation, not a stronger API contract. Tasks do not survive Host crash under
kill-on-close semantics.

Actual first probes: assign failure never executes the marker fixture; closing the
Job signals the handle of its newly created descendant; an unread pipe has at least
4096 unread bytes then independent stop completes; this does not measure physical pipe
capacity or prove a truly full pipe/ConPTY stall. An unrelated dummy remains alive;20
cycles show88→88 process handles in this sample. This is not100-cycle stress, PTY,
Windows11, UI-detach/Host crash integration, real CLI or release certification.

Review regression adds a controlled cfg(test) assignment hook: assign this owned child,
resume it, wait for its marker side effect while alive, then let the unchanged normal
resume flow fail. The final test does not override thread selection or require a
second ResumeThread return0: on this machine the ordinary flow detects multiple owned
threads after execution. It asserts marker presence, OutcomeUnknown and a signaled
duplicate of this exact owned root handle after cleanup. Initial normal-flow RED14
returned the misleading ResumeFailedReaped; GREEN15 preserves uncertainty.

```powershell
cargo test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-native-runtime -- --nocapture --test-threads=1
cargo clippy --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-native-runtime --all-targets -- -D warnings
cargo fmt --manifest-path remote/crates/runtime/Cargo.toml -- --check
```

src/bin/owned-job-fixture.rs is a harmless process-only test fixture; it must not be
packaged as a production application/remote launch endpoint. Tests create their own
temporary markers and stop only owned children. Descendant PID metadata is used only
to acquire a query/synchronize handle for that exact fixture, never PID-based kill.
Non-Windows launch returns Unsupported; platform coverage remains a separate gate.
# Pinned ConPTY qualification (default off)

The `conpty-qualification` feature adds a `test=false`, TEST_ONLY finite worker
and an isolated vendored portable-pty suspended Job seam. It is not a public Host
launch API or an integration with desktop admission/TerminalCore. The worker
source-includes the mature desktop `conpty_runtime.rs` loader; this is a qualification
reuse detail, not a completed production shared-loader extraction. Loader failure
is fixed rejection; no system/inbox fallback is admitted by the worker.

From the isolated checkout root, first run `node scripts/prepare-conpty.mjs`.
Build the worker with `cargo build --manifest-path remote/Cargo.toml -p
cc-desk-native-runtime --features conpty-qualification --bin
conpty-qualification-worker --offline`. Copy only the three files listed by
`src-tauri/conpty/manifest.json` from the verified generated runtime to ignored
`remote/target/debug`, then run `node scripts/prepare-conpty.mjs --verify
remote/target/debug`. The loader independently rechecks those files when run.
Run the finite qualification tests with:

```powershell
cargo test --manifest-path remote/Cargo.toml -p cc-desk-native-runtime --features conpty-qualification --test conpty_qualification --offline -- --nocapture --test-threads=1
node remote/scripts/verify-portable-pty.mjs
```

New seam errors are fixed codes, never command/cwd/environment text:
`KnownNotStarted { Preparation | CreateRejected }` means no child was created;
`KnownNotStarted { AssignmentReaped }` requires failed assignment, no resume call,
successful termination of our suspended child, and a successful wait. Any resume
return other than exactly one, or unconfirmed root cleanup, is `OutcomeUnknown`;
there is no automatic retry. Cleanup waits are finite, but synchronous Win32 calls
and pseudoconsole destruction are not promised to be preemptible or bounded.

Actual Windows10 evidence covers normal input/output, the fixture's measured
100x30 console buffer after resize, exact retained-handle stop, failed assignment
with no execution marker, Job-close root exit, and observed output EOF/reader join.
The parent watchdog bounds worker lifetime and stdout/stderr capture together;
captures have fixed limits. It can terminate only its exact owned worker. Timeout
fails with cleanup unknown; it is not silently accepted. Startup CPR/DA1 responses
are finite fixture setup using the serialized writer, not an ongoing terminal
reply authority. Physical pipe capacity, resume-anomaly and Windows11/real CLI
qualification remain unmeasured. Subsequent finite lifecycle cases below add real
worker-crash and output-stall observations without those broader guarantees.

The CLI Job does not establish membership or a measured exit barrier for the
separate OpenConsole process created during `CreatePseudoConsole`. Root exit,
observed reader EOF, console-host exit, descendant quiescence and old-IO fencing
remain distinct obligations. Existing non-PTY `OwnedJob` API is unchanged and is
not upgraded by these results. The new path uses returned `hThread` directly;
it never calls the legacy non-PTY Toolhelp resume path or any global inventory.

Before production E08 integration, preserve typed creation uncertainty and the
backend StartTicket lifecycle: existing desktop `AdmittedChild::spawn_native`
assumes its original spawn error means no child created, which is incompatible
with blindly substituting a post-create/unknown seam. Native loader extraction,
signed distribution, broader crash/pressure qualification, and whole-tree/old-IO
barriers remain OPEN. No network, credentials, remote launch DTO or grants exist here.

## Finite lifecycle observations (Windows10)

`--test conpty_lifecycle` adds five TEST_ONLY cases: abrupt worker death observed
through an exact CLI-process witness; the same with a separately witnessed fixed
grandchild; actual stalled output with drain/recovery/stop; and an actual forced
observer timeout whose error remains Timeout after separate cleanup succeeds;
and a deliberately pending separately owned witness proving cleanup cannot reopen
its first absolute deadline when that witness exits late.

Private handle transfer uses `DuplicateHandle` from the supervisor into the worker
it actually created, then from exact retained fixture process handles back into
that known supervisor. Witnesses have synchronize/query rights only. The fixed
grandchild is created by the fixed fixture, without breakaway or inherited env;
its transferred query handle is checked against the worker's exact Job before
READY. No PID opening, process/thread inventory, or parent-retained Job handle
changes the last-Job-handle close semantics. Process-local DUP_HANDLE capabilities
are trusted-harness details, not a protocol, account, pairing, or production grant.

Output is at most64MiB of public, sequence-varying text in16KiB writes. After at
least64KiB is actually read, the drain pauses; a live heartbeat must advance while
successful-write progress stays unchanged for300ms. Resuming drain must advance
both output read and write progress by64KiB, then a second paused stall is measured
before exact-handle stop. Root exit and actual reader EOF/join are checked separately.
This establishes observed backpressure/recovery under these conditions, not physical
pipe capacity, arbitrary terminal workloads, capacity certification, or DoS immunity.

The supervisor has a20s case deadline plus a distinct shared2s cleanup observation
budget. It terminates only its exact owned worker first, then observes known
read-only fixture handles. Control fields/captures are bounded; worker input has
receive deadlines, and fixture synchronous input has5s/12s self-exit watchdogs.
Positive cleanup cases reject the watchdog exit90, rather than treating natural
fixture expiry as a successful stop. The forced120ms observation deadline remains
an error even when owned cleanup is subsequently observed; timeout never authorizes
a retry or a generation transition. Kernel calls/destructors are not preemptible
deadline guarantees. An unjoined control read on an error is contained by the
finite worker boundary, not described as a proven graceful IO barrier.
Starting cleanup seals the witness set. Pending cleanup can only retain or tighten
its first absolute deadline, including subsequent Drop; an expired budget still
requests exact-worker termination but cannot wait again or accept late success.
Cleanup already completed within its sealed budget remains idempotently completed.

Known root and fixed-grandchild exit do not prove every possible descendant,
OpenConsole membership/exit, or old application-IO fencing. Abrupt worker kill
does not join its internal reader. No negative control establishes Job as the
exclusive cause of observed exit; ConPTY teardown may also contribute. Production
Host/terminal attachment and new-generation admission remain unimplemented.

### Qualification build resources

On Windows x64, `conpty-qualification` now stages the prepared, pinned desktop
ConPTY bundle next to both profile binaries and dependency test binaries. Run
`node scripts/prepare-conpty.mjs` from the repository root before Cargo. The build
script reads the one desktop manifest, rejects duplicate/unexpected file names
and invalid sizes, and tracks each source for rebuilds. It does not download or
install files. The runtime loader still verifies all hashes, PE architecture and
exports before use; staging is not a replacement for those checks or an atomic
multi-file installation. Other platforms and the default feature do not stage it.

A clean remote target previously had no adjacent bundle: all five lifecycle tests
exited before their first witness. After this fix, the same five and the full
16-case feature suite pass, including two manifest-preflight cases. This remains
fixed-fixture Windows10 evidence, not real AI CLI or complete old-IO retirement.
