# Native terminal client bridge candidate

`NativeClient` consumes the original granted Client-role `RunningChannel`.
It derives the session/run/generation/Host context from that native authority;
JavaScript supplies neither an identity nor an authorization boolean. Its fence
must come from the native installed-peer result. It does not enroll or pair.

`exchange` validates existing control commands and returns only an exact typed
reply (request, operation, sequence, stamp and outcome as applicable). `snapshot`
returns a complete contiguous same-stamp/state grid under the live target and
binding, capped at 4 MiB. The existing UI reducer still validates semantic cell
roles and continuity. Snapshot return does not acknowledge it or grant input.
Both use one absolute 1..1000 ms deadline and one nonqueued operation per owner.
Errors or cancellation after admission close this exact data owner, without
replaying input. `close().await` waits for that owner's task; a dropped waiting
future still retains the owner until its containing resource is dropped.

The UI `nativeConnection` adapter maps the two native operations to the existing
`NativeConnection` interface, bounds and validates returned bytes, fences late
callbacks, and exposes `waitClosed` separately from synchronous close. This is
the portable bridge implementation. The application now registers snapshot,
exchange and close commands only with the default-off `remote-client-ipc`
feature. `documentClientPort` uses the original document bootstrap; native
admission checks the actual WebView and retained document proof before decoding
requests and again before disclosing bytes. Resource IDs locate an already
admitted original owner and never authorize a replacement document. Navigation,
revocation and destruction synchronously request closure of that exact channel;
the resource retains its retirement worker until it is joined. Window names, frontend fields and a
caller-supplied resource ID alone cannot supply that authority. No default
remote endpoint, native key provider or production connection factory is enabled
here. The native-only `install_native` entry has no application caller yet; there
is no JavaScript connect, grant or pairing command and no signed phone shell.

App exit seals native admission atomically, retains the original exit code and
waits for any original native creation reservation before joining its resource.
One tracked waiter may remain pending if a synchronous native factory does not
return; it cannot preempt that factory or report retirement early. Factory panic
preserves an unknown state and blocks successful shutdown or new admission. A
cancelled close command retains the worker's join handle. Completing
`NativeClient.close()` means the original data-owner task was awaited; its
current API folds an inner Tokio JoinError into Closed, so retirement does not
attest normal protocol exit or absence of an inner panic. Failure of the outer
retirement worker remains unknown.
The first pending exit intent survives competing exit events; only the original
code is admitted after retirement. Document loss requires app restart under the
current single-initialization document runtime. Revocation invalidates and starts
joining its original channel but does not offer navigation-based reconnection or
a new document-owned factory. Same-document network reconnection still requires
explicit close/join and a new native-installed resource.

Tests use public scalar1/RFC fixture keys, actual mutually pinned RPK TLS and
the opt-in AppBind grant-management exchange over bounded memory IO. The Host
side of bridge tests is a protocol fixture, not a PTY or CLI. They exercise
scope/source/fence, exact reply matching, snapshot completeness, cancellation
and concurrent-call refusal. They do not establish phone, WebView, native
presence, persistent key storage, real CLI or production acceptance. The
application IPC DTO, reservation and document-revocation tests are fixtures;
compiling registered commands does not establish actual WebView acceptance.

From the repository root:

```powershell
cargo +1.98.1 test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-client
cargo +1.98.1 clippy --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-client --all-targets -- -D warnings
node --experimental-strip-types --test remote/ui-terminal/src/*.test.ts
```

For the application IPC fixtures on the existing Windows toolchain (default
feature remains off):

```powershell
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_INCREMENTAL='0'
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --features remote-client-ipc --lib remote_client_ipc
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --features remote-client-ipc --lib revoke_hook_tests
```
