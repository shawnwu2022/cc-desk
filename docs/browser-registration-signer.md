# Browser registration TLS signer

## Boundary

`createBrowserRegistrationTlsSignerBroker(tlsKey, origin, {setupDeadline, signal})`
adapts a caller's **independently authorized, existing TLS identity** before a
WebAuthn credential exists. It does not construct a `BrowserProfile`, invent a
credential ID, accept a public registration candidate as approved policy, or
establish a new trust root. The enrolled-profile API remains separate.

The caller must independently select and authorize the identity and exact
controlled HTTPS origin. Possession of a `CryptoKey` and passing these structural
checks do not prove that authorization, a human identity, a peer pin, or native
GUI approval. A same-origin caller is inside the existing code/key-use trust
boundary. This API is not an authorization sandbox against that caller.

Required admission checks:

- Existing private, nonextractable, sign-only ECDSA P-256 `CryptoKey`.
- Exact canonical HTTPS origin matching the secure top-level window.
- Cross-origin isolation, `SharedArrayBuffer`, `Atomics.wait`, and WebCrypto.
- The operation's original `AbortSignal`, not already aborted.
- The original finite absolute setup deadline in the
  `performance.timeOrigin + performance.now()` clock domain, with strictly
  positive remaining time of at most 10 seconds. The caller must not refresh
  the deadline or substitute a new signal during retries.

The only signing surface remains the existing dedicated-worker TLS 1.3 client
`CertificateVerify` message with a 32-byte transcript hash. The registration
entry adds no generic signing, TLS server signing, key generation/export,
WebAuthn calls, persistence, network transport, enrollment, grant, or supplier.
It cannot attest that the caller-selected TLS key is separate from a grant key;
that independent identity selection remains the owner's responsibility.

## Lifecycle

Both entries use the original broker, single-transfer worker port, generation
and sequence fences, bounded shared response, DER conversion, and close/retire
semantics. Only the port and shared bridge cross into the worker; the key stays
in the window. The registration deadline determines the remaining per-request
budget without extending the original absolute deadline.

The setup deadline bounds signing, not the lifetime of an established TLS
channel or the complete registration ceremony. Neither entry closes an idle
bridge merely because that setup deadline passes: the existing worker also
uses the shared CLOSED flag to revoke its entire data channel. A new signing
request after setup expiry fails closed without another WebCrypto operation.
Abort or expiry during an active signing request wakes the waiting worker and
prevents subsequent signing. The owning operation must abort/close the broker
when its work ends. A future registration worker/owner must separately enforce
the original overall registration deadline and signal; it must not mistake the
at-most-10-second TLS setup deadline for the whole ceremony's lifetime.
`waitClosed()`
does not claim cancellation of WebCrypto: it retains an in-flight original
Promise until settlement, discards late output, clears shared signature bytes,
and then releases the broker's key reference. This is reference retirement,
not proof of CryptoKey destruction or revocation of another caller's reference.

## Verification and limits

All fixtures use a published TEST_ONLY scalar-1 key imported nonextractably in
memory. They never create an authenticator credential or use a user's key.

- `browserTlsSigner.test.ts`: admission, exact origin, invalid keys/signals and
  deadlines, secure/top-level/isolation guards, idle abort, both APIs keeping an
  established bridge across setup expiry while rejecting another signature,
  plus the original enrolled signer regression cases.
- `browserRegistrationTlsSigner.integration.ts`: real Node worker, MessagePort,
  shared memory, WebCrypto signature and independent DER verification; rejects
  generic/server signing; abort/deadline during held original crypto; deadline
  preserved after a successful signature. Browser globals are fixtures here.
- `tests/remote-browser/browser-registration-signer.spec.ts`: actual isolated
  Edge window and dedicated worker, real WebCrypto and independently verified
  DER, generic/server rejection, cancellation/expiry while crypto is held, and
  non-isolated fail-closed admission. Test instrumentation forbids key generation
  and WebAuthn calls. No browser trust or security setting is weakened.

From the repository root, set `TEMP`, `TMP`, and `TMPDIR` to the existing
`.audit/runtime-temp` directory. Set `CC_DESK_TEST_ONLY_BROWSER_RESULTS` to an
isolated directory under `.audit` before running Edge. Commands:

```text
npm --prefix remote/ui-terminal test
node --experimental-strip-types --test remote/ui-terminal/src/browserRegistrationTlsSigner.integration.ts
node node_modules/typescript/bin/tsc --noEmit --strict --noUnusedLocals --noUnusedParameters --target ES2022 --module NodeNext --moduleResolution NodeNext --allowImportingTsExtensions --lib ES2023,DOM remote/ui-terminal/src/browserTlsSigner.ts remote/ui-terminal/src/browserRegistrationTlsSigner.integration.ts
node node_modules/@playwright/test/cli.js test --config playwright.remote-browser.config.ts tests/remote-browser/browser-registration-signer.spec.ts
```

This slice deliberately does not wire the registration entry into a production
worker/WASM/bootstrap transport. These tests are not evidence of a complete
registration TLS handshake, native GUI approval, end-to-end registration,
durable trust, OS authenticator operation, or deployment. The existing
[registration evidence](browser-registration-evidence.md) and native approval
requirements remain unchanged.
