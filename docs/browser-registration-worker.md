# Isolated BrowserWasm registration owner

This slice connects the existing main-thread registration producer to the original native registration owner through genuine mutual-RPK TLS. It is an independent entry point, not an enrolled-profile shortcut. Existing application/grant/data workers and browser enrollment storage are unchanged.

## Ownership

- `createBrowserRegistrationBridge` accepts independently approved existing nonextractable TLS/grant/relay key handles, an exact HTTPS origin, original setup and total deadlines, and an original abort signal. It does not generate keys, obtain approval, establish identity, or persist a candidate.
- Its transferred ports and shared cancellation/attachment fence allow one dedicated Worker. Ordered request 1 and request 2 map to the original producer's create and confirm methods. Requests are at most 4096 bytes and responses at most 16384 bytes. Repeated, concurrent or out-of-order callbacks revoke the transaction.
- `startBrowserRegistrationWorker` uses a separate WASM registration owner. The Rust owner uses the existing CDR1 codec and distinct `cc-desk-enroll/1` ALPN. `RegistrationPending` cannot become AppBind, a grant, or terminal data.
- TLS setup remains capped at 10 seconds. The total registration deadline is fixed before TLS and capped at 120 seconds. Completion of setup does not renew the registration clock or revoke the complete transaction. The Worker samples both monotonic and wall clocks and fails closed after clock discontinuity or a suspension gap.
- The historical decision promise is separate from exact ACK/TLS-close completion. A delivered installation result is not a bearer credential or browser-install approval. Missing ACK does not authorize reinstalling anything.

## Retirement

Producer confirm closes the producer's own admission, not the complete transaction. Main-thread closure waits for the original producer operations and original TLS signing promise to settle. Worker closure waits for the original WASM task to drop, original IO operations to settle, and the transport's actual `waitClosed` to complete. A final two-way retirement handshake joins those owners; neither side can report successful closure while the other is still retaining work.

TLS `shutdown` flushes close-notify without closing the underlying transport's read side. This allows the exact acknowledgement sequence to observe the peer's TLS EOF. Dropping the original Rust task closes the transport.

WASM traps and failed retirement contracts report uncertainty rather than a successful closed state. The caller still owns final destruction/isolation of a failed Worker. There is no retry, automatic Worker replacement, deadline refresh, or replay.

## Isolated verification (2026-11-14)

Fifteen actual Edge cases exercise the dedicated WASM owner and original native registration gate: Yes, deny, native document invalidation, confirmation after TLS setup expiry, cancellation/freeze/total expiry while the original producer promise ignores abort, held transport retirement, held original TLS signing promise, wrong independent Host pin, compile/instantiate cancellation, and poll/event/cancel traps. The positive path checks actual mutual-RPK TLS evidence, the native run-local receipt, exact ACK and TLS close, plus joint retirement. Native registration channel tests remain 14/14 passing; native and WASM security strict Clippy and TypeScript checks pass.

WebAuthn returns synthetic platform objects. Native approval completes the original cfg(test) state-machine callback. TEST_ONLY WebSocket/MessagePort IO carries actual WASM-generated TLS records to the loopback native socket. This is not a native TLS client standing in for the browser. The Host stays Locked, with zero Host network attempts and zero private Host supplier opens.

## Explicit limits

The test authenticator responses, public scalar keys, original native GUI completion callback, and loopback transport are TEST_ONLY. This does not qualify an actual device authenticator, genuine native GUI approval, durable browser/native trust, production pins, deployment, or account identity. The caller must obtain the required specific authorization before using real credentials or initiating real registration. Relay keepalive is separately owned and must preserve this same total deadline and cancellation.
