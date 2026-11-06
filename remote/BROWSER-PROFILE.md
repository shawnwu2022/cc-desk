# Browser approval candidate (code only)

This change adds an explicitly enrolled, in-memory browser approval policy to the
existing TLS/RPK/AppBind grant exchange. It does not register credentials, choose
an origin/RP ID, deploy a website, or expose a browser `NativeClientPort`.
Missing enrollment remains unavailable. The application panel still requires its
original authenticated document bridge.

The controlled HTTPS origin is trusted code with permission to use browser keys.
SRI does not confer the native signed-bundle trust boundary. Before requesting
presence, that page must display the verified grant's principal, endpoint/context,
scope, role and expiry. UV proves verification at the authenticator; it does not
prove a trusted display of those grant semantics.

## Keys and enrollment

The browser holds separate non-exportable P-256 WebCrypto private keys for TLS
endpoint authentication and grant-issuer signing. `browserProfileBoundary` checks
the key handles, algorithm, usages and non-exportability; trusted enrollment must
also ensure distinct underlying key material. The authenticator holds a separate
WebAuthn credential. Host policy stores only the approved public keys, credential
ID, exact HTTPS origin, RP ID and explicit backup policy. No CNG export or software
key substitution is provided. Enrollment, revocation distribution, loss/recovery
and replacement require a separate explicit operation; this candidate performs
none. `GrantGate::revoke` immediately revokes its pending and live grants.

## Version 1 presence envelope

The existing bounded grant-management control record carries canonical JSON with
ordered fields `version`, `cose`, `credentialId`, `authenticatorData`,
`clientDataJson`, `signature`. Version is exactly 1; byte fields use canonical
unpadded base64url. The complete record remains at most 16 KiB; decoded COSE is
at most 8192 bytes, credential ID 1024, client data 2048, signature 80 and
authenticator data exactly 37. Unknown/duplicate envelope fields and noncanonical
serialization fail closed. Client data supports only `type`, `challenge`,
`origin`, optional `crossOrigin`, optional `topOrigin`; unknown fields and
duplicate known fields fail closed. This deliberately strict subset may reject
authenticator/browser extensions rather than infer their semantics.

The SHA-256 challenge starts with `CC-DESK-BROWSER-APPROVAL-v1\0`, then
big-endian u32 length-prefixed origin, RP ID, credential ID, credential SPKI,
two backup-policy bytes, management nonce, current TLS/AppBind binding ID, and
the existing full COSE Sig_structure. That structure includes issuer kid,
domain/AAD and every canonical grant field, including Host challenge, endpoint
context, principal, scope, role, grant ID and Host expiry.

The Host checks the original issuer COSE signature and WebAuthn ES256 assertion.
It requires exact credential/origin/RP ID hash, `webauthn.get`, expected challenge,
UP and UV, enrolled BE equality and explicit BS permission. Cross-origin/top-origin
assertions, AT/ED and reserved flags are refused. Signature counters, including
zero, do not replace the pending Host challenge. WebCrypto COSE signatures use
raw 64-byte P1363; WebAuthn assertion signatures use DER. TLS also needs DER and
must not reuse the COSE representation.

Policy selection belongs to the trusted Gate/signer owner, never wire detection.
An enrolled browser Gate cannot fall back to bare native COSE. One pending grant
and the existing per-challenge three-failure/30-second limit remain. A new
management attempt cannot clear a still-live pending challenge or its remaining
failure budget; it receives Busy. An expired or exhausted pending challenge may
be replaced under the existing Gate policy. This is not a cumulative per-device
or cross-challenge rate limiter. Accepted retries
must preserve the exact envelope bytes, nonce and binding and original Host
expiry. Another signature for the same grant is refused. A new nonce/binding
requires a new challenge and presence ceremony; browser client state therefore
does not reuse an old envelope on a newly established management exchange.

## Evidence and remaining work

Rust tests use public TEST_ONLY fixture signatures over in-memory
`tokio::io::DuplexStream` TLS/RPK/AppBind channels, then exchange an actual scoped
control record. They are not network, installed application or phone tests.
Node tests import a public scalar1 fixture into non-exportable WebCrypto handles;
the WebAuthn response is a synthetic API fixture, with no credential registration
or real `navigator.credentials` request. The production producer uses actual
SubtleCrypto and `navigator.credentials.get` APIs, but has no fabricated Ready
adapter and is not connected to the application panel.

The wasm target selects ring's official JS RNG feature and excludes native Tokio
net/multithread features. A workspace-local LLVM compiler now builds ring, and the
upstream `rustls-pki-types/web` feature supplies real browser time through
`web-time` on wasm32-unknown-unknown. The locked security-crate wasm check and
`cargo build --target wasm32-unknown-unknown --lib` pass. The build produces a
Rust `.rlib` archive with library code generation; it is not an executable `.wasm`
or browser bundle, running Tokio/browser clock or TLS session.

## Bounded TLS signing bridge

`ui-terminal/src/browserTlsSigner.ts` adds the limited synchronous callback needed
by a future rustls `Signer::sign` integration. `createBrowserTlsSignerBroker`
reuses `browserProfileBoundary` on the enrolled, top-level HTTPS Window and holds
only its non-exportable `tlsKey`. It does not sign with `grantKey`, export a key,
register a credential, request WebAuthn presence or implement another TLS stack.
The controlled page and its DedicatedWorker are trusted key-use code. The owner
must privately transfer `broker.worker.port` once, together with the descriptor's
shared buffer/metadata, to that Worker, then call
`createBrowserTlsWorkerSigner(descriptor)` there. Do not publish the port or pass
the profile/key into the Worker. No public window-message listener is installed.

Both sides require `crossOriginIsolated`, SharedArrayBuffer and Atomics. Only a
DedicatedWorkerGlobalScope may construct the synchronous signer; Window, service
or shared workers and missing capabilities fail closed. Isolation headers and a
controlled worker origin must be supplied by a future deployment. There is no
UI-thread wait or fallback. `sign(message)` either returns DER bytes or throws an
Error that the future rustls adapter must propagate as TLS failure; it establishes
no Ready, channel identity or grant. No Wasm ABI or Rust adapter is implemented.

The input is exactly 64 spaces, `TLS 1.3, client CertificateVerify`, one zero byte
and a 32-byte transcript hash. This matches the existing SHA256-only cipher-suite
profile. Server CertificateVerify, COSE and arbitrary messages are refused. Input
is copied before validation/use. Actual SubtleCrypto ECDSA/SHA256 returns raw
64-byte P1363; the broker checks scalar bounds and emits minimal positive ASN.1
DER integers (at most 72 bytes). This does not change the separate COSE raw and
WebAuthn DER representations.

Each bridge has a fresh private MessageChannel, fixed 92-byte SharedArrayBuffer,
random generation label and one-time Worker attachment. Its positive int32
sequence increases without wrapping or resetting; exhaustion closes the bridge.
The port, buffer and generation are scoped together and never reused. Only one
request/original SubtleCrypto Promise is retained, with no work queue or retry.
Requests have exactly four fields and one fixed-size message; responses use the
one fixed shared slot. Unexpected, duplicate, out-of-order, concurrent or malformed
requests close the entire bridge. These bounds describe the trusted producer and
broker; browser MessagePort itself cannot impose a transport queue quota against
compromised same-origin code holding the private capability.

A request gets one absolute monotonic deadline from
`performance.timeOrigin + performance.now()` before it is sent. The API requires
an explicit 1..10000ms budget derived from the original TLS setup's remaining
budget, without extending its original deadline. This bridge enforces its own
per-sign deadline; the future Rust adapter must still enforce the original setup
deadline across all handshake work. Both bridge sides compare the same per-sign
deadline; message progress never refreshes it. A queued request first observed after expiry starts zero crypto
operations. Timeout, AbortSignal cancellation, explicit close or protocol failure
sets a sticky shared close flag and wakes the Worker; no later sign or result reuse
is allowed. Close also stores a terminal value into the actual waited-on state
before notification, so a Worker entering native wait after the notification
observes a changed value instead of sleeping through close. Normal request,
completion and consumption transitions use compare/exchange and cannot overwrite
that terminal state. A result is checked again after copying before publication. The final
close/deadline sample is its completion linearization point; cancellation cannot
retract a signature already returned before that point.

SubtleCrypto operations already started cannot be canceled. The broker keeps
their original Promise and key reference until settlement, discards late results,
and only then resolves `waitClosed()` after removing handlers, clearing output and
releasing its key reference. A never-settling API Promise therefore leaves
`waitClosed()` pending. This proves retirement of this bridge's own operation and
references only: it does not prove CryptoKey destruction, Worker-thread exit or
whole browser/TLS-owner retirement. The caller must close the broker when its
Worker/owner is canceled or terminated; no browser thread join is claimed.

Node tests use actual `worker_threads`, MessagePort, SharedArrayBuffer/Atomics and
real SubtleCrypto with public TEST_ONLY scalar1 imported as non-exportable handles;
Node crypto independently verifies the returned DER. Browser platform globals are
explicit fixture shims, not evidence from an actual isolated browser. Held-Promise
fixtures exercise cancellation, timeout, concurrent refusal and late completion;
each fixture awaits termination of its own Node Worker. Existing browser approval
tests still use a synthetic WebAuthn response and never open a real prompt.
A deterministic scheduling fixture pauses immediately before the production
native Atomics.wait call, closes and retires the broker including its original
crypto Promise, then releases the gate. The unchanged V1 implementation returned
`timed-out`; the repaired implementation returns `not-equal` without waiting for
the original budget. The fixture still invokes the real native wait.

Browser WebSocket IO, wasm bindings, runtime/time driving, original
TLS/AppBind/grant/peer-install ownership and the authenticated operation adapter
remain unimplemented. Successful native fixtures, this Node bridge test or a wasm
compile must not be presented as a functioning browser owner, app panel or phone
end-to-end connection.

Reference: [WebAuthn Level 3 assertion verification](https://www.w3.org/TR/webauthn-3/#sctn-verifying-assertion).
