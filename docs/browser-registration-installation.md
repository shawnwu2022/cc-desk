# Registration to browser persistence

## Selected design

This preview uses explicit local-owner device pairing, not account authentication.
Existing independently approved TLS pins and original browser TLS/grant/relay handles
remain prerequisites. Neither a public pairing candidate nor registration success
establishes that initial authority. Production defaults remain unavailable/Locked.

1. Read new credential metadata directly from the original live, typed native
   registration evidence. Do not parse the display JSON to recover authority or
   assume a credential ID before the synthetic/real ceremony returns it.
2. Keep the current run-local Yes and historical receipt unchanged. A separate
   native approval explicitly names browser persistence, exact origin/database,
   expected revoked revision, all three browser keys, credential, and complete
   configuration. It does not authorize a Host connection or native restart trust.
3. Deliver that separate decision only on the original authenticated registration
   channel. The browser's controlled owner compares all independently approved
   non-credential terms and retains the original handles. It must observe the
   original decision, ACK/TLS completion and actual joint retirement before a
   single existing IndexedDB install transaction. No retry or new deadline.
4. A browser commit is a separate transaction from the native run-local install.
   Lost delivery/ACK or a failed browser commit does not roll back native history
   or permit reinstall. Reload uses the existing stored reader; revoke uses its
   existing exact revision and retirement protocol. A new native run still needs
   independently admitted configuration and a fresh Host session approval.

## Implementation and verification plan

- Add live typed evidence access with cancellation/deadline regression tests.
- Add separate native persistence-purpose frozen approval and bounded channel DTO.
- Add controlled browser install owner over the unchanged store and default reader.
- Test default Locked, exact binding, missing completion, held retirement,
  cancellation/deadline, revision conflicts, store reload and revoke with synthetic
  credentials. Combine the actual Worker/WASM/Relay fixture afterward; do not call
  isolated mocked-session tests full-chain evidence.

No real authenticator credential, key configuration, installation or deployment is
performed by these tests. Actual origin/RP, account association, existing key use,
browser persistence and later signing need explicit authorization. Native durable
identity integrity/recovery and real native GUI/platform qualification remain
separate gates, not implied by this browser-only persistent record.

## Implemented owner boundaries

`UnapprovedBrowserRegistration::verified_binding()` returns borrowed public data
after checking the original cancellation/deadline. It has no deserializer and is
not a permit. Native approval retains and rechecks the original evidence, rather
than treating the view or display JSON as authority.

`NativeBrowserInstallationApproval` is separate from `NativeRegistrationApproval`.
Its original dialog names browser persistence, exact database/revoked revision,
user association, three keys and complete configuration. Consuming that single
permit installs only the current run's Host candidate, still Locked, and returns
`BrowserInstallationApproved` on the original channel. It is not proof that the
browser subsequently committed. Serialized copies cannot be imported as approval.

The controlled main-thread `createBrowserRegistrationInstallation` captures the
original Worker session, original handles, independent approved plan and original
signal/deadline. The plan includes every configuration field except the new
credential ID/SPKI and observed backup eligibility; its backup policy remains
fixed. Native verified evidence supplies only those new credential facts. An
absent plan, old run-local receipt, changed known field, failed ACK/TLS completion
or uncertain retirement cannot install.

After all checks the installer proves each original private handle matches its
independently selected public key and calls the unchanged store once. It awaits
the actual IDB transaction. If a real commit is observed after cancellation,
returned metadata preserves that historical fact, not fresh permission to connect.
An aborted transaction fails. No outcome relaxes revision, revocation or stale
owner-marker rules. The normal reader and default unavailable provider remain
unchanged. Same-origin owner code and its independent source remain trusted; this
does not defend against malicious same-origin code or manufacture pin/account trust.

## Verification boundary

The Edge/IDB owner tests use synthetic session promises and are not authenticated
channel evidence. The separate actual Relay matrix uses the original Edge producer,
dedicated Worker, newly compiled Rust WASM, real Relay and native test oracle. Its
native Yes callback and credentials remain TEST_ONLY. The outer WSS forwarder
verifies the published fixture CA in Node, not an actual Edge certificate prompt.

The independent plan is emitted before the browser creates its new synthetic
credential ID. Successful installation closes the page, reopens the store through
production APIs without private-key import, checks all three non-exportable
handles, revokes and verifies a later page is Locked. This proves registration to
browser persistence, not a complete terminal session: the fixture closes its native
run and does not resume it across restarts.

The 2026-11-14 isolated verification passed all 16 actual Relay scenarios (the
original nine plus install, deny, document replacement, expired approval, held
retirement, independent-plan mismatch and old run-local receipt). Separate real
Edge/IDB composition coverage passed 56 new owner tests plus 11 existing store
tests. Security libtest passed 113 tests with one pre-existing ignored browser
oracle; native approval/candidate/scope regressions passed 18. Native and WASM
strict Clippy, full project typecheck and edition-specific rustfmt passed.
The compile-fail doctest confirms public JSON cannot construct the typed view.

Use `tests/remote-browser/registration-relay.spec.ts` for authenticated composition
and `tests/remote-browser/registration-installation.spec.ts` for the standalone
owner boundaries. Follow the project-local fixture/build environment in
[browser registration Relay](browser-registration-relay.md), rebuilding the
registration WASM after protocol changes. All temp directories, profiles, logs
and artifacts must stay under the project. A missing fixture environment is an
unrun/failed preparation, not passing functional evidence.
