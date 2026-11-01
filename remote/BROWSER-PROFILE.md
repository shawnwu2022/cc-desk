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

The wasm target now selects ring's official JS RNG feature and excludes native
Tokio net/multithread features. `cargo check` currently reaches ring's C build and
fails because this environment has no clang executable. No compiler or global
binding tool was installed. Even after compilation, a controlled worker bridge
is required for synchronous rustls `Signer::sign` and asynchronous SubtleCrypto;
it must keep UI/event-loop execution unblocked, preserve non-exportable keys and
fail closed without required isolation capabilities. Browser WebSocket IO,
runtime/time driving, original TLS/AppBind/grant/peer-install ownership and the
authenticated operation adapter remain unimplemented. Successful Rust native
fixtures or a wasm compile must not be presented as a functioning browser owner.

Reference: [WebAuthn Level 3 assertion verification](https://www.w3.org/TR/webauthn-3/#sctn-verifying-assertion).
