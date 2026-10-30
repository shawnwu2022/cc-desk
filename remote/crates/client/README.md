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
the portable bridge implementation, **not a registered Tauri command or a signed
phone shell**. Shell integration must retain the original authenticated document
resource, admit it before each call and disclosure, and close/join its owner on
navigation, revocation or destruction. Window names, frontend fields and a
caller-supplied resource ID alone cannot supply that authority. No default
remote endpoint, native key provider or production route is enabled here.

Tests use public scalar1/RFC fixture keys, actual mutually pinned RPK TLS and
the opt-in AppBind grant-management exchange over bounded memory IO. The Host
side of bridge tests is a protocol fixture, not a PTY or CLI. They exercise
scope/source/fence, exact reply matching, snapshot completeness, cancellation
and concurrent-call refusal. They do not establish phone, WebView, native
presence, persistent key storage, real CLI or production acceptance.

From the repository root:

```powershell
cargo +1.98.1 test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-client
cargo +1.98.1 clippy --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-client --all-targets -- -D warnings
node --experimental-strip-types --test remote/ui-terminal/src/*.test.ts
```
