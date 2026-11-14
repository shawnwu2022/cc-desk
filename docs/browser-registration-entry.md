# Controlled browser registration entry

`/browser-registration.html` is separate from `/browser-remote.html`. Loading it
does not read a provider, open a socket or call WebAuthn. Only its explicit Start
registration button calls the one-use registration source. Missing, rejected or
invalid sources leave the page Locked; there is no retry on that document.
The separate [product profile reader](browser-product-provider.md) consumes only
a previously approved installed enrollment.

## Independent source contract

`controlledBrowserRegistrationSource().register(lifetime)` reads the same-origin
`/browser-registration.js` once per module/document, including failure and cancel.
That controlled module must export exactly:

`readApprovedBrowserRegistrationInputs(signal): Promise<ApprovedBrowserRegistrationInputs | undefined>`

The result is `{keys: {tlsKey, grantKey, relayKey}, approvedPlan, artifacts: {wasm,
glue}}`. All three keys are original distinct non-exportable ECDSA P-256 signing
handles. The complete `BrowserRegistrationInstallationPlan` is independently
approved before TLS, including the exact Host SPKI, relay, user association,
database/revision, scope and later terminal artifacts. `artifacts` here names the
registration WASM and standalone glue, not the later terminal runtime.
No credential ID is invented in the source. No native candidate, Ready event,
receipt or returned outcome can supply this independent approval.

The entry API is `createBrowserRegistrationEntry(inputs, lifetime)` with
`lifetime = {signal, setupDeadline, registrationDeadline}`. Deadlines are absolute
performance epoch values, fixed before loading (setup <= 10 seconds, total <=
120 seconds). It returns `outcome`, `completed`, `evidence`, `install()`, `close()`
and `waitClosed()`. Only `install()` attempts persistence, after the unchanged
installer observes the original outcome, exact ACK/TLS completion and aggregate
retirement. Callers must close unused entries. Handles cannot start a second
entry on the same module instance even after failure.

The installer first validates the plan and key policy. The entry then independently
proves all three private/public key matches using fresh, purpose-separated
challenges before opening any socket. It preserves the source Host pin for the
first TLS handshake. Wall/performance clock discontinuity, page replacement,
visibility loss, freeze, cancellation and expiry seal the original transaction.

## Artifact and retirement boundaries

Source/glue are fetched once with omitted credentials, no redirects, no cache,
same-origin HTTPS and a 1 MiB streamed cap. WASM has an 8 MiB streamed cap.
Captured JavaScript bytes are imported through short-lived Blob URLs; the new
registration page explicitly permits these module imports in its CSP. Providers
and glue must be standalone modules or use approved absolute imports. The ordinary
terminal page's CSP is not relaxed. Rejection by a stricter host CSP stays Locked.

The dedicated Worker runs the original `startBrowserRegistrationWorker`; it does
not implement another registration protocol or terminal path. Repeated start
messages abort the original Worker, never launch another. Failed artifact loading
still joins the original bridge/transport retirement. The page joins original
bootstrap/preflight work, bridge, socket (including relay-constructor failure),
relay, transport and Worker retirement before
terminating the Worker. `terminate()` is cleanup, not a substitute for retirement.
Late original promises are retained; uncertainty cannot authorize IDB installation.

Reload uses `createBrowserEnrollmentStore().openExisting` and the existing profile
reader. No second private-key import, automatic terminal connection or native
restart trust is introduced. The production connection provider is now built as `/browser-profile.js`; no
default independent `/browser-registration.js` source or automatically approved
configuration is shipped.

## Evidence limits

`registration-entry.spec.ts` uses real Edge WebCrypto/IDB with synthetic Worker
notifications and socket events to test composition fences. It is not full TLS,
Relay/native approval or real-authenticator evidence. Artifact loader tests are
Node boundary tests. `registration-entry-worker.spec.ts` exercises the actual
dedicated Worker startup failure/replay/retirement paths. The production-page
smoke reads the built HTML and its real CSP and verifies both pages stay Locked
without a provider or authenticator call.

`RegistrationEntry_ActualRelayNative_*` in `registration-relay.spec.ts` separately
uses the controlled source, production entry, production Worker, actual WASM,
Relay and native oracle. The success case installs and reloads without key import;
the wrong independently selected Host pin fails before WebAuthn and cannot install.
The outer fixture WSS forwarder still validates the fixture CA in Node, and both
authenticator objects and native approval callbacks remain TEST_ONLY. These cases
must be rerun for the final native executable; intermediate artifact results are
not final-source approval or evidence of real-device enrollment.
All generated profiles, logs and artifacts belong under project `.audit`.
