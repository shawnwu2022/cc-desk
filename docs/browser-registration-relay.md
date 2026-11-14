# Actual Edge registration through the legacy Relay

This TEST_ONLY acceptance composes the existing main-thread registration producer, dedicated Edge Worker, real BrowserWasm mutual-RPK TLS, production browser Relay adapter, actual sibling Relay executable, native transport, and original native registration session/approval/install owner. It does not replace the browser TLS client with a native client.

## Fixed scope and ownership

- The Relay checkout is explicitly selected at `7026049026d0ee42bb6d5e8b626c741506184d1b`. Both endpoints select legacy mode. There is no rendezvous negotiation or automatic fallback.
- Playwright's TEST_ONLY outer WSS forwarder copies complete binary messages unchanged. Node validates the published fixture CA and `foobar.com` name with `rejectUnauthorized: true`; the test also requires TLS 1.3. It does not parse or sign admission/TLS records. This is not acceptance of Edge's certificate UI or a public endpoint.
- The browser executes the production Relay admission/signing adapter, existing registration bridge, dedicated Worker and real WASM TLS. Public scalar fixture keys and synthetic WebAuthn platform objects are confined to tests.
- Native uses `test_only_connect_admitted_loopback_with_mode`, `start_admitted_registration` and `registration_accept_until`, then the original `NativeRegistrationSession`. Approval completes only the original `cfg(test)` callback. There is no genuine GUI approval bypass in production.
- Browser bridge and Relay share the original abort signal and absolute setup/total deadlines. Native TLS/registration and transport receive the same original end, with an exact channel deadline equality assertion. Setup is at most 10 seconds and total lifetime remains capped at 120 seconds; the fixture uses browser 50 seconds and native 60 seconds, except explicit expiry cases. No phase refreshes these deadlines.
- The 35-second wait holds the original synthetic authenticator create promise before native approval. It does not extend the native approval cap, which remains 30 seconds.

## Nine-case matrix

1. Yes: verifies actual full mutual-RPK TLS evidence and enrollment ALPN, exact native run-local receipt, ACK/TLS completion, and all owners retired.
2. Deny: receives `Rejected`, acknowledges it, strictly completes TLS, installs nothing and retires.
3. Wait 35 seconds: retains the original authenticator promise beyond the 30-second IO budget while both directions exchange at least three empty Relay frames. Releasing that promise resumes the original registration and strict completion.
4. Wrong independent Host pin: TLS fails before either authenticator operation; there is no TLS evidence or installation.
5. Browser cancellation: the original abort signal fails the transaction; main/Worker retirement still waits for the original held producer promise.
6. Browser total expiry: Relay closes before 3.5 seconds for the original 2.5-second total; the held original producer is not falsely reported retired.
7. Native cancellation: native retires between 2.9 and 7 seconds with no installation or ACK. Browser Relay retires between 9 and 33 seconds, and no success or joint retirement is reported while the original producer remains held.
8. Native total expiry: the native channel's original end is exactly four seconds and retirement is between 3.9 and 7 seconds. The same browser-side retained-producer assertions apply.
9. Held transport retirement: a completed protocol exchange cannot make main/Worker report retired before the original transport retirement promise is released.

For the two native-ending cases, legacy Relay unregisters the peer rather than broadcasting an authenticated cancellation message. The next browser heartbeat fails on the offline destination. Relay socket retirement and registration completion are different events: WASM is still awaiting the original create promise. The test observes actual Relay retirement, verifies that main/Worker are still retained, then releases that same producer promise. It requires a completion error, zero `get` calls, no outcome/completed event and final joint retirement. It does not abort the browser or close its socket to manufacture that result.

Every case checks that the native Host remains Locked, Host network attempts and private Host supplier opens remain zero, and no persistent trust is created. Positive outcomes are historical run-local receipts, not durable credentials.

## Final-byte and fixture teardown ordering

The production registration-only transport flush receipt prevents the route from observing producer EOF before the original WSS writer flushes final TLS bytes. The deterministic RED/GREEN and complete transport suite are documented separately in [native registration Relay lifetime](native-registration-relay.md).

The fixture waits for the original native driver naturally. It keeps the actual Relay infrastructure alive until Edge proves protocol/transport/main/Worker retirement. Only then does the test send `TEST_ONLY_BROWSER_RETIRED` on the native oracle's stdin, remove the fixture policy and require successful Relay/native process exit. This avoids confusing early fixture teardown with missing final TLS bytes.

## Commands and evidence

Final verification on 2026-11-14: one complete final-source Edge run passed all nine cases in 1.7 minutes. Root native strict Clippy (`remote-host --lib --tests -D warnings`), complete project TypeScript checking and the native fixture formatting check all exited zero.

The final run is recorded separately in `.audit/registration-relay-chain-20261114/completion.json`; the older keepalive manifest is preserved. It contains source, native executable, WASM/glue and actual Relay hashes, source revisions, individual scenario artifacts, commands and explicit qualification limits.

All commands run from the selected cc-box worktree. `TEMP`, `TMP`, `TMPDIR`, Cargo target and Playwright result paths are set inside that worktree before execution. Native preparation/build:

```powershell
node scripts/prepare-conpty.mjs
cargo test --locked --manifest-path src-tauri/Cargo.toml --features remote-host --lib NativeRegistration_RelayOracle_903 --no-run
```

The generated executable is selected explicitly with `CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_ORACLE`. The run also requires exact `CC_DESK_TEST_ONLY_REGISTRATION_WASM`, `CC_DESK_TEST_ONLY_REGISTRATION_GLUE`, `CC_DESK_TEST_RELAY_ROOT`, `CC_DESK_TEST_RELAY_EXE` and project-local `CC_DESK_TEST_ONLY_BROWSER_RESULTS` paths. The WASM/glue are the previously verified isolated registration artifact, reused byte-for-byte, not rebuilt or substituted by this acceptance.

```powershell
node node_modules/@playwright/test/cli.js test --config playwright.remote-browser.config.ts tests/remote-browser/registration-relay.spec.ts --grep RegistrationRelay_ActualEdgeNative
npm run typecheck
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --features remote-host --lib --tests -- -D warnings
rustfmt --edition 2021 --check --config skip_children=true src-tauri/src/tests/native_cli_registration_relay.rs
```

Historical failures remain available. The pre-flush binary lost final TLS bytes in deny/wait35. The first post-flush complete matrix passed seven cases; its two native-ending cases incorrectly expected registration completion within 15 seconds while deliberately retaining the original authenticator promise. The final fixture separates native retirement, Relay retirement and producer settlement without changing production deadlines or protocol behavior. Native fixture comments were translated after the binary build, with no logic or line-count change; the manifest records both tested and final source hashes.

## Explicit limits

This is a synthetic registration-through-actual-Relay composition, not real-device WebAuthn, genuine native GUI approval, phone acceptance, persistent browser/native enrollment, production pins, account identity, public deployment or a push. The isolated 15-case Worker suite and separate Node rendezvous acceptance are independent evidence; they are not counted as cases in this nine-case legacy matrix. Default entry remains Locked.
