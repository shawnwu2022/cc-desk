# Browser enrollment authority lifecycle

This is a page-local ownership mechanism for independently approved material. It is **not a real enrollment ceremony**, operator authentication, credential registration, or a durable trust store. The normal product entry and the example profile stay Locked without an approved supplier. Public candidates remain untrusted and cannot create an approval.

## Owner and supplier boundaries

`createBrowserEnrollmentAuthority()` returns a frozen owner with `provider`, `approve(load)`, `revoke()`, and `waitClosed()`. Only controlled same-origin owner code receives the approval and revocation methods. Pass only `provider` to `createApprovedBrowserProfileReader(candidateBytes, provider)`.

- `approve(load)` is one-shot. The loader receives only the original owner `AbortSignal`, never candidate bytes, a scope lookup, or a key selector. It must independently load an already-approved public configuration and its correctly bound original TLS/grant handles, relay signer, and presenter. It does not generate keys or perform a trial signature.
- The authority validates and copies the public record and freezes the original handle references. Approval of the record does not prove that a human authorized it or that public key material corresponds to private handles. The controlled provider remains responsible for those facts.
- A read before approval synchronously seals the authority as unavailable. Approval failures, cancellation of the original pending read, and revocation cannot be repaired by re-reading, swapping the candidate, replacing a signal, or approving again. A new explicit owner flow and fresh authority are required.
- The existing pairing helper still performs a full comparison with the independent approved record before producing inputs. It never treats public JSON as an approved record.
- `revoke()` first closes admission and removes the record, then aborts the original owner signal. Cached pairing inputs and cached connection sources reject later use. Revocation does not undo already accepted Host effects.

The authority itself has no persistence, import of serialized approval flags, URL/local-storage enrollment or automatic recovery. The [product provider](browser-product-provider.md) now imports the existing store/authority for lazy startup from an independently approved installed record. It does not supply an initial registration approval. The controlled generic operator module and candidate example remain supported.

## Cross-bundle lifecycle contract

`BrowserProfileProvider` and `ApprovedBrowserInputs` support an optional trusted `enrollment` object with exactly two own data fields, or three when the original store owner supplies the optional close-only `retire` callback:

```ts
Readonly<{
  signal: AbortSignal;
  retain: (originalRetirement: Promise<void>) => void;
  retire?: () => Promise<void>;
}>
```

`retire()` closes only the current activation and joins original store owner cleanup and lock release, without deleting the persistent enrollment. The source retains transport completion separately so store retirement cannot wait on itself.

This object is a lifecycle contract, **not evidence of authorization**. It requires an actual native signal and function and is never serialized into the public pairing schema. The checker retains the same signal/function identities, taking a frozen snapshot if necessary. Structural validation deliberately supports separately bundled operator and product modules instead of depending on a module-local WeakMap brand.

The owner keeps each original load/read/connection retirement. A first already-cancelled read reserves its complete retirement before observing cancellation. The browser source reserves its complete retirement before opening a WebSocket, signing, starting brokers, or creating a Worker; it then listens for revocation and rechecks the signal. Revocation during registration therefore cannot cross that boundary.

Closing a source waits for the original application port, TLS and grant brokers, relay, transport, and Worker retirement. `waitClosed()` stays pending until revocation and every registered operation actually settles. A loader or presenter that ignores abort keeps it pending. A rejected original retirement yields the fixed error `browser enrollment retirement uncertain`; a timer or a caller's cancellation is not proof that an original operation retired. Expected rejection of a cancelled read/publication does not by itself make completed retirement uncertain.

Legacy providers without this optional contract remain supported. They do not acquire owner revocation semantics automatically. Controlled code must preserve the lifecycle through the helper and source. Code with independent access to key references, a stripped lease, another page, or another tab is outside this page-local mechanism. A non-exportable CryptoKey already handed to code cannot be cryptographically retracted by this API.

## Isolated verification

`tests/remote-browser/browserEnrollment.test.ts` covers default Locked state, independent snapshots and original handles, cache rejection after revoke, original pending ownership, uncertain retirement, one-shot failure, malformed lifecycle values, mismatched candidates, pre-aborted direct reads, same-stack read-before-approval, cancellation before loading, invalid direct signals, and later caller cancellation.

`tests/remote-browser/browser-entry.spec.ts` bundles the real authority/helper into the TEST_ONLY operator module separately from the built product page. It exercises the original Relay, Rust Host, WASM, and production browser source, including:

1. A matching independently approved record produces one memory-writer effect; scope/pin mismatches remain locked before signing or connecting.
2. Revoking an active connection retires its original chain and cached refresh cannot reconnect or write.
3. A pending synthetic presenter that ignores abort keeps `waitClosed()` pending until its original promise resolves. No WebAuthn assertion or Host installation follows revocation.
4. Revocation during source retirement registration yields zero signatures, WebSockets, Workers, and artifact loads.

Tests use fixed public TEST_ONLY private scalars imported as non-exportable in-memory handles, synthetic WebAuthn, a memory writer, Playwright route handling, and a Node TLS bridge. They do not establish real WebPKI/DNS deployment, real enrollment, real device presence, real CLI/PTY operation, or native GUI approval. See [pairing supplier contract](browser-pairing-profile.md) and [original Relay fixture commands](../remote/BROWSER-PROFILE.md#supplier-through-the-complete-browser-entry-and-original-relay).

## Remaining production admission work

A real deployment still needs an explicitly authorized operator approval flow, independent durable trust storage and identity-to-handle binding, real credential enrollment and its recovery/revocation policy, and the native GUI approval bridge. This slice implements none of those external actions. It provides a bounded, revocable in-memory owner that such a controlled flow can use without changing the supplier, Host grants, or Relay protocol.

The subsequent [origin-local store adapter](browser-enrollment-store.md) adds explicit persistence of independently approved existing handles and fail-closed revision/retirement records. It preserves this page-local authority and does not fill the initial authentication, real credential registration, hardware trust or recovery gates described above.
