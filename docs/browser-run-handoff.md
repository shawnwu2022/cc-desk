# Authenticated Browser run handoff

## Explicit opt-in, unchanged exact path

`createStoredBrowserRunProfileReader(candidateBytes, store, present)` is a separate
controlled-module reader for a remembered device connecting to a new native run.
It is not selected by a URL, public JSON field, discovery response or receipt.
A trusted `/browser-profile.js` deployment must explicitly choose this reader;
no production provider is installed, and the normal entry remains Locked without
one. Creating or reading the helper does not connect, present, sign or use WebAuthn.
The existing explicit Connect/Refresh action still opens the original source.

The helper first executes `createStoredBrowserProfileReader` unchanged. The
candidate must exactly match every field of the original stored configuration,
including its historical scope. It cannot select a different Host, issuer,
principal, role, epoch, credential, relay route or artifact. A candidate cannot
contain the new policy marker. Missing enrollment, a competing owner, an uncertain
retirement marker, a mismatch or revocation stays Locked.

Only after that original comparison does the new reader construct an in-memory
grant configuration. It removes the historical scope and sets
`scopePolicy: 'authenticated-host-run'`. It preserves the exact original
non-exportable TLS/grant handles, relay signer, presenter and enrollment lease.
There is no new key import, identity root or second session owner. Every cached
read still checks the original reader's signal and lease. Forget/abort retain the
original connection's complete retirement and Web Lock rules.

The existing `createApprovedBrowserProfileReader`, stored exact reader and Rust
`ClientGrantTrust` retain exact-scope semantics. A changed Host scope is still
rejected before presentation in that path. The previous replacement-only guidance
in the enrollment-store document continues to describe that exact reader; this
separate opt-in reader does not replace or mutate the stored enrollment.

## Mutually exclusive configuration

The Browser-to-WASM grant configuration has two explicit shapes:

- Exact: the original `scope` object, with no `scopePolicy` field.
- Resume: `scopePolicy: 'authenticated-host-run'`, with no `scope` field.

All other fields are identical. Neither marker, both fields, an unknown marker,
an inherited/accessor marker, or an explicitly present undefined/null scope cannot
select resume. TypeScript entry/broker boundaries and the Rust configuration
parser reject malformed modes; missing scope is never a fallback. Persisted v1
pairing configurations remain exact-only and reject the new marker.

## Authentication and fresh approval

The resume marker is a request to consider one freshly authenticated Host proposal,
not authorization of any scope. The companion Rust implementation uses a separate
Browser-only grant entry, never a wildcard in the existing exact path:

1. The original stored Host SPKI and Browser TLS handle establish mutual RPK TLS.
   Original AppBind checks the Host/Client endpoints, host/client key epochs and
   relay origin, then authenticates the current exporter-bound context.
2. On this same owned channel, a fresh request has a fresh nonce and no old grant
   reuse. The Host challenge must match the complete current context, nonce,
   channel binding, frozen principal and role; the original codec bounds and
   validates its scope before constructing `VerifiedCanonicalGrant`.
3. That exact original verified payload supplies the displayed session/run/
   generation, COSE TBS and WebAuthn challenge. The original presenter requires a
   fresh explicit approval; then the original grant signing and WebAuthn UP/UV
   verification run. Native Host approval remains separate and mandatory.
4. The original Accepted nonce/binding/grant ID/digest checks, peer installation
   and Data path still bind the resulting capability to this new scope. A receipt,
   displayed scope, policy string or previous successful connection cannot reopen
   it or replace the new approval.

Only host/client key epochs are preselected by the Browser configuration.
Activation, incarnation, policy epoch and issuer epoch come from the authenticated
Host offer and are bound into its full AppBind context; they are not claimed to be
independently preselected Browser values. No extra ALPN, scope endpoint, socket,
TLS trust root or ad-hoc signature protocol is introduced.

The newly observed/approved scope is never written to IndexedDB. The original
configuration bytes and revision stay unchanged; only the existing original-owner
marker follows its established lifecycle. Future native runs still require fresh
admission, Host approval, authenticated challenge, presentation and WebAuthn.
Cancellation, deadline, hidden/replaced document, late callbacks and Forget do not
turn an unfinished original operation into retirement or approval.

## Verification and remaining boundaries

`browser-run-handoff.spec.ts` exercises actual Edge WebCrypto, IndexedDB and Web
Locks with fixed published TEST_ONLY keys. It covers all three non-exportable
stored keys, exact original handle/lease reuse, no persistence of the policy or
new scope, missing enrollment, changed candidate bindings, strict explicit modes,
unchanged exact reading, cancellation and durable Forget/retirement.

`browser-run-handoff-relay.spec.ts` uses the built product page and original
Worker, actual WASM, original Relay and original memory Host. Its stored scope is
intentionally different from the current Host scope, and the latter never enters
the profile provider. It checks fresh production presentation/approval, synthetic
WebAuthn, current-scope descriptor and one exact data effect, plus unchanged stored
configuration and blocked reopen. Exact-old-scope and wrong pin/principal/epoch/
role cases must reject before grant signing/presentation. Cancel, expiry and
Forget must not produce a grant or write. Each case records the actual artifact
hashes; the old-WASM RED is not evidence for a rebuilt WASM.

Run the targeted browser suites with `playwright.remote-browser.config.ts`. The
Relay suite requires explicit `CC_DESK_TEST_ONLY_ORIGINAL_RELAY_ORACLE`,
`CC_DESK_TEST_ONLY_DATA_HOST`, `CC_DESK_TEST_ONLY_DATA_WASM` and
`CC_DESK_TEST_ONLY_DATA_GLUE` artifacts. Rebuild WASM after the companion Rust
change and build the current frontend before acceptance. Set TEMP/TMP/TMPDIR and
`CC_DESK_TEST_ONLY_BROWSER_RESULTS` under this checkout's `.audit`.

The fixture's outer WSS bridge validates a fixed test CA in Node; it does not prove
browser WebPKI or certificate UI. Its Host approval and authenticator remain
synthetic and its writer is in memory. These tests do not certify native restart,
DPAPI, real named keys, native GUI consent, a real authenticator, phone suspension,
CLI control or production deployment. Those gates remain independent.
