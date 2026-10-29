# portable-pty qualification fork

Upstream: `portable-pty` 0.8.1, MIT (retained `LICENSE.md`), registry archive
`https://crates.io/api/v1/crates/portable-pty/0.8.1/download`.
The cached archive SHA-256 is
`806ee80c2a03dbe1a9fb9534f8d19e4c0546b790cde8fd1fea9d6390644cb0be`,
also recorded in the existing desktop Cargo.lock. Its published `.cargo_vcs_info.json`
records commit `4afedd626dadd15d9c2929bab0e2063b54f61393`, path `pty`.

`portable-pty-upstream.json` records all 15 original file hashes, all patched file
hashes, and the patch hash. `../patches/portable-pty-owned-job.patch` replays the
eight-file change against that exact registry archive. Original `spawn_command`
is unchanged. The new Windows-only `SlavePty::spawn_command_in_job` reuses the
upstream argument, environment, cwd and pseudoconsole attribute helpers. It owns
the returned primary process/thread handles before assignment or resume, without
PID/thread lookup. The qualified new paths fix the empty Unicode environment block
to have two NUL terminators. The old desktop dependency and behavior are unchanged.

The exact-byte checks require preserved LF files and disable Git autocrlf during
patch replay; narrow `.gitattributes` rules preserve vendor/patch bytes on checkout.

```powershell
node remote/scripts/verify-portable-pty.mjs
node remote/scripts/verify-portable-pty.mjs --archive <cached-portable-pty-0.8.1.crate>
```

The first command checks the complete current inventory and patch hash. The second
also checks the registry archive checksum, original inventory, clean patch replay,
and resulting inventory. One-byte tampering was actually rejected, then restored.
These are reviewed content locks; a party able to replace the manifest and source
together is not defeated by a checksum. This is not a publisher signature, upstream
security audit, maintained-fork commitment, or release supply-chain attestation.

The dependency is presently consumed only by the runtime's explicit, default-off
`conpty-qualification` feature and finite TEST_ONLY worker. It has no authorization,
credential storage, network entry, TerminalCore attachment or dispatch integration.

## Separate D native pipe qualification

The Windows-only `spawn_pipe_in_job` path additionally reuses CommandBuilder's
existing quoting, environment and cwd helpers for an explicit backend recipe.
It owns the exact CreateProcessW process and primary-thread handles, assigns the
borrowed Job while suspended, and requires ResumeThread's previous count to be1.
No PID adoption, primary-thread enumeration, breakaway, token/ACL change or desktop
admission bridge is introduced. The old spawn_command and spawn_command_in_job
paths are unchanged by D; the updated patch also retains the earlier reviewed hook.

Only three inheritable child stdio handles occur in the explicit attribute list;
parent pipe ends are noninheritable. Anonymous CreatePipe handles remain opaque
synchronous File-based Read/Write wrappers: they are deliberately not converted
to Rust ChildStdin/ChildStdout/ChildStderr, whose Windows conversion requires
asynchronous handles. The new runtime wrapper is default-off under the existing
conpty-qualification feature, and does not adapt the private journal actor.

Qualification uses one500ms absolute interval beginning immediately after native
creation returns, with checks around assignment/resume and before publishing.
It does not preempt CreateProcessW, File I/O, debug output or other synchronous
kernel calls. Failed assignment can be KnownNotStarted only after exact owned-root
reap and within qualification; any resume-stage ambiguity remains OutcomeUnknown.
RootExitOnly does not certify descendants, complete old-I/O retirement or a grant.
Drop requests termination but cannot synchronously promise bounded cleanup.

Exact new tests, prior failures and remaining gates are recorded separately in
integration-evidence/native-pipe/author-report.md outside the repository. Tests
use fixed public TEST_ONLY self fixtures under a parent-owned outer Job before GO.
Neither successful D qualification nor this content lock repairs/qualifies the old
OwnedJob entry, actual three CLIs, Windows11, enrollment or production admission.
