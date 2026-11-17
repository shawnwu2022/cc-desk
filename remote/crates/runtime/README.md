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

### Explicit native pipe custody prerequisite

The same default-off qualification feature now exposes a bounded backend recipe
and a native pipe owner for the next durable-input integration slice. It uses the
exact CreateProcessW primary-thread handle, assigns the suspended process to its
owned Job, requires ResumeThread=1 and restricts inheritance to three stdio handles.
It has no network admission, remote grants or desktop launch integration. Its
500ms qualification interval starts after CreateProcessW returns; it does not bound
that synchronous call. The broker design needed for a general launch deadline is
still OPEN and has not been implemented as part of this migration.

Eleven current-candidate cases cover explicit environment/argv/cwd, native IO, five
concurrent launches, assignment refusal before fixture entry, root stop/stream EOF,
unrelated inheritable-handle exclusion and harness error/budget boundaries. These
are fixed self-owned Windows10 fixtures, not PTY semantics or the three AI CLIs.
The existing portable-pty0.8.1 archive, source inventory and cumulative patch replay
are verified byte-for-byte; no new dependency version, global cache patch or desktop
behavior change is implied.

An integrated regression caught an intermittent empty-output canary failure. The
old fixture assumed a valid numeric child handle meant the parent object leaked;
a deterministic unrelated-child-event test reproduced that oracle defect. The
corrected fixture attempts to signal the number and checks the exact retained parent
event after child exit. Positive API sensitivity and an unrelated-event negative
control are checked separately. This proves only that the fixed child did not signal
that canary, not absence of every possible inherited handle. The original intermittent
failure's complete cause is not established by one later successful diagnostic run.

### Typed PTY custody prerequisite

The qualification-only NativeOwnedPtyRun reuses the same bounded backend recipe,
initializes the app-local pinned loader before allocating a PTY and transfers its
reader/writer at most once. Root stop uses the retained process handle and does not
take the transferred writer's lock. Four scoped cases pass, including a fixed cmd
PTY roundtrip and a Preparation classification when pinned resources are absent.
Source ordering places pinning before PTY allocation; the missing-resource case
does not supply an independent target-process creation witness.
The separately supervised TEST_ONLY worker is ignored by the parent test harness
and invoked explicitly under its private outer Job.

That cmd fixture contains direct DA/CPR startup responses. This is a fixture facility,
not an admitted Host response path. The new Host single-writer composition without
that bypass currently fails its independent effect observation; see ACCEPTANCE.md.
The wrapper neither proves complete OpenConsole/tree/old-IO retirement nor preempts
synchronous Win32 calls or master destruction. No desktop or remote endpoint is
enabled by this wrapper.

### Pressure-probe observation consistency

The TEST_ONLY lifecycle-pressure report consumes the same accepted read/written
sample that met both+64KiB resume thresholds. It no longer rereads a progress file
which the public fixture may be truncating/updating. The2s phase interval,two300ms
stall observations and owned cleanup conditions are unchanged. A real temporary-file
gap regression,late-sample refusal and checked-counter tests qualify this observer;
they do not prove a general hard deadline for blocking Windows IO. Fixed control
events and worker stderr are emitted before assertions so failures remain visible.
