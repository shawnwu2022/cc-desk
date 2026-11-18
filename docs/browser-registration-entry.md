# Controlled browser registration entry

`/browser-registration.html` is separate from `/browser-remote.html`. Loading it
does not read a provider, generate keys, open a socket or call WebAuthn. The
explicit Prepare button reads the one-use source; its separate trusted identity
and full-review actions prepare local approval. Only the later Start button
consumes that approval and creates the registration entry. Missing, rejected or
invalid sources leave the page Locked; there is no retry on that document.
The separate [product profile reader](browser-product-provider.md) consumes only
a previously approved installed enrollment.

## Independent source contract

`controlledBrowserRegistrationSource().prepare(signal)` reads the same-origin
`/browser-registration.js` once per module/document, including failure and cancel.
Prepare has one absolute five-minute lifetime and resolves without returning
private inputs. Unapproved ephemeral keys may survive temporary visibility loss
within that lifetime, but approval requires a visible document. Approved inputs
are immediately captured and frozen, then expire within 60 seconds or permanently
on visibility loss. Cancellation, pagehide/freeze and document changes invalidate
the attempt. Pending original key generation/export is retained until settled.
The same source object's separate register(lifetime) consumes the approved
snapshot once; register without successful preparation rejects. No retry or
automatic key regeneration occurs on this document.
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

The [controlled source supply adapter](browser-registration-source.md) now offers
strict public registration configuration parsing, independent provider comparison,
original-handle preservation and an explicit standalone source build. The product
build also ships a local-operator preparation source: trusted explicit key
generation, public identity matching and a separate immutable full review.
The product source requires the exact fixed product database and origin without
rewriting them. Public JSON alone never establishes approval. Local operator
choice does not authenticate an account or prove Host identity; independent Host
pin checking and the original native/device/storage approvals remain necessary.

The entry API is `createBrowserRegistrationEntry(inputs, lifetime)` with
`lifetime = {signal, setupDeadline, registrationDeadline}`. Deadlines are absolute
performance epoch values, established at Start after preparation (setup <=
10 seconds, total <= 120 seconds). Preparation does not extend either original
deadline. Entry runtime loading and cryptographic preflight stay within setup. It returns `outcome`, `completed`, `evidence`, `install()`, `close()`
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
restart trust is introduced. The production connection provider is built as `/browser-profile.js`;
the separate `/browser-registration.js` supplies explicit local-operator preparation,
not automatically approved configuration or account/Host authentication.

## Evidence limits

`registration-entry.spec.ts` uses real Edge WebCrypto/IDB with synthetic Worker
notifications and socket events to test composition fences. It is not full TLS,
Relay/native approval or real-authenticator evidence. Artifact loader tests are
Node boundary tests. `registration-entry-worker.spec.ts` exercises the actual
dedicated Worker startup failure/replay/retirement paths. The production-page
smoke reads the built HTML and its real CSP and verifies no source read on load,
no authenticator call, and cancellation back to Locked. The production preparation
tests exercise the actual emitted source, trusted review and original entry's
three-key proof, then stop at a TEST_ONLY socket constructor. This is not a full
new-product registration, real pairing or native persistence acceptance.

`RegistrationEntry_ActualRelayNative_*` in `registration-relay.spec.ts` separately
uses the controlled source, production entry, production Worker, actual WASM,
Relay and native oracle. The success case installs and reloads without key import;
the wrong independently selected Host pin fails before WebAuthn and cannot install.
The outer fixture WSS forwarder still validates the fixture CA in Node, and both
authenticator objects and native approval callbacks remain TEST_ONLY. These cases
must be rerun for the final native executable; intermediate artifact results are
not final-source approval or evidence of real-device enrollment.
All generated profiles, logs and artifacts belong under project `.audit`.
