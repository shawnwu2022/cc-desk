# Browser product profile provider

## Fixed product startup

The normal production build emits `/browser-profile.js` with exactly one export:
`readApprovedBrowserInputs(signal)`. Importing it does not open IndexedDB, read a
record, present a dialog, create/import a key, use WebAuthn, sign or connect. Only
the existing explicit Connect action reads it. The registration entry remains Locked without its independent registration source.
Product startup remains Locked without an installed enrollment. This module does
not implement an enrollment ceremony or invent a default registration provider.

`browserProductStartup.ts` exports `BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME`,
whose value is `CC_DESK_BROWSER_ENROLLMENT_V1`. Trusted registration plan authors
must explicitly select that database in their independently approved plan. The
origin, database name and expected revoked revision are part of the original
native persistence approval. Never rewrite them after approval or silently move
records from another database. Existing generic installers retain their explicitly
approved targets. URL query/fragment values, public candidates and receipts cannot
select the product database or authorize an installation.

## One original installed record

`createInstalledBrowserRunProfileReader(store, present)` opens the original
`openExisting` session once, directly reads its approved provider once, and uses
`createInstalledBrowserEnrollmentProfileReader` for the existing strict config,
handle and assembly validation. It does not fabricate a candidate by comparing a
record with itself. Authority derives from the original independently authorized
installer and controlled same-origin code/storage boundary, not syntax validation.
The original complete configuration, three stored non-exportable keys, revision,
owner marker and lease remain those of that same session.

Only after validation does this explicitly named startup path select the existing
in-memory `scopePolicy: 'authenticated-host-run'` grant mode. Historical scope is
not edited in storage. All other pins, epochs, principal, role, credential,
relay route, artifact URLs, callbacks and lease are preserved. The existing
candidate exact reader and candidate run-handoff reader retain their comparison
semantics. No new key, TLS trust root or persistent scope is created.

Success, absence and failure are cached; there is no status-then-open, second read,
automatic retry or replacement owner. Each cached return checks the current read
signal, original read signal and original enrollment lease. Initialization and
cached-read failures close and await the original session when one was acquired.
An original store-open failure also joins the original store completion before
rejecting, even when no session was returned. Uncertain retirement still rejects
and retains its original lock; it is not treated as successful cleanup.

## Connection retirement

A persisted session's original lease additionally exposes a close-only `retire()`.
It revokes only that page-local activation, waits for retained original work, then
waits for the original store owner marker cleanup and exclusive Web Lock release.
It does not revoke/delete the persistent enrollment. Old two-field leases remain
valid; the validator accepts only exact two- or three-field data-property shapes.
The identical lease flows through the provider, approved inputs and source.

The source caches its closing promise before synchronous abort callbacks. Its
lease-retained work settles from actual transport/Worker completion only. The
outer close also joins store retirement and the original physical socket even
when Relay construction fails, avoiding a cycle in which the lease waits
on its own store cleanup. Disconnect, cancellation, expiry and admission failure
therefore do not need a fixture or UI to call `session.close()`. Unknown transport
or owner cleanup keeps the original fail-closed semantics. Forget is still the
separate existing explicit persistent revocation operation.

The controlled module source also retires an already-read enrollment when late
cancellation, deadline or source construction failure prevents handoff. Cancellation
is not a successful retirement receipt, and no timeout steals ownership.

## Trust and verification limits

Origin-isolated browser storage and trusted same-origin code are the architecture.
This does not defend against malicious same-origin scripts, privileged browser/OS
access, disk rollback, copied profiles, backups or eviction. Storage loss and
uncertain durable ownership stay Locked. Real key use, credential creation or
configuration, origin/RP selection and persistence still require their applicable
explicit authorization. This implementation performs none of those real actions.

`tests/remote-browser/browser-product-startup.spec.ts` uses actual Edge WebCrypto,
IndexedDB and Web Locks, with published TEST_ONLY scalars in a separate fixture.
It imports the actual built product module and verifies its sole export and zero
import effects, fixed database, missing/revoked/malformed/uncertain-owner refusal,
original handle/lease reuse, unchanged stored config, no startup import/signing,
competition, cancellation, late callbacks and original retirement. The source
retirement case uses a delayed synthetic socket to prove that its owner marker
cannot clear before original physical close. It is not real Relay/WASM evidence.

Build first with `node node_modules/vite/bin/vite.js build`; run the focused suite
with `node node_modules/@playwright/test/cli.js test --config
playwright.remote-browser.config.ts browser-product-startup.spec.ts`. Set
TEMP/TMP/TMPDIR and `CC_DESK_TEST_ONLY_BROWSER_RESULTS` inside this checkout's
`.audit`. The independent full product-entry acceptance supplies actual original
Relay/Host/WASM artifacts; native GUI consent, real authenticators and deployment
remain separate gates; see [real-device acceptance](browser-real-device-acceptance.md).
No test-only installer or private fixture scalar is an
entry in the production module build.
