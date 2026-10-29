# portable-pty qualification fork

Upstream: `portable-pty` 0.8.1, MIT (retained `LICENSE.md`), registry archive
`https://crates.io/api/v1/crates/portable-pty/0.8.1/download`.
The cached archive SHA-256 is
`806ee80c2a03dbe1a9fb9534f8d19e4c0546b790cde8fd1fea9d6390644cb0be`,
also recorded in the existing desktop Cargo.lock. Its published `.cargo_vcs_info.json`
records commit `4afedd626dadd15d9c2929bab0e2063b54f61393`, path `pty`.

`portable-pty-upstream.json` records all 15 original file hashes, all patched file
hashes, and the patch hash. `../patches/portable-pty-owned-job.patch` replays the
four-file change against that exact registry archive. Original `spawn_command`
is unchanged. The new Windows-only `SlavePty::spawn_command_in_job` reuses the
upstream argument, environment, cwd and pseudoconsole attribute helpers. It owns
the returned primary process/thread handles before assignment or resume, without
PID/thread lookup. The new path alone fixes the empty Unicode environment block
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
