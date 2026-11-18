# Browser grant presentation

## Design and boundaries

The controlled same-origin operator module currently supplies a presenter callback, but repository end-to-end fixtures use a TEST_ONLY button. This slice adds a reusable production HTML dialog presenter for the existing Rust-verified canonical grant. It does not add enrollment, key creation, persistence, a new grant protocol, or a default provider. The native Host one-shot approval dialog remains independent and mandatory.

Use `createBrowserGrantPresenter(origin)` while constructing the controlled module's original handles. It captures the original secure, isolated, visible, top-level document and returns the existing `(verified, signal) => Promise<void>` callback. The callback is one-shot. Only a trusted explicit click on the displayed approval button resolves it; programmatic clicks, JSON flags, and dialog return values do not grant approval. Browser trusted event status is not a physical-presence guarantee against OS/browser automation. The original WebAuthn verification remains required after this dialog.

The dialog shows the requested Observer/Controller permission, exact session/run/generation and the entire immutable grant payload as inert text. It validates presentation shape and bounds, not cryptographic authority. Unknown fields, accessors, invalid scope/role, and changed presentation DOM fail closed. Directional control characters are escaped for review. Cancel is initially focused. Cancel/Escape, original signal cancellation, hidden/replaced document, page exit, or removal of the dialog reject and clean up the original UI/listeners. No timer extends or replaces the broker's original deadline.

Alternatives considered: continuing an injected arbitrary test button leaves no production UI; adding persistent enrollment at the same time would silently assume a trust root. A focused presenter is useful independently and can be tested through the existing actual Edge/Relay/Host/WASM chain without creating real credentials. A durable enrollment design still needs independently authenticated approval, identity-to-handle binding, version/revocation policy, and explicit credential authorization. Editable public JSON and Host `ApprovalRecorded` cannot supply those facts.

## Implementation plan

1. Add actual Edge presenter tests: immutable complete display, trusted click, programmatic-click rejection, cancel/Escape, abort/removal/visibility, concurrent/repeated use, malformed input, original document/origin, and narrow viewport. Observe the missing implementation failure first.
2. Implement `remote/ui-terminal/src/browserGrantPresenter.ts` and product dialog styles, without changing the broker protocol or default Locked entry.
3. Add a fixture mode that supplies the production presenter to the existing original enrollment authority and grant broker. Prove successful one-write, rejection with zero WebAuthn/signing beyond the established TLS channel, and owner revocation while the dialog is open.
4. Run Edge and affected package regressions, root typecheck/build, independent read-only review, then commit only this slice.

The full-chain test continues to use fixed public TEST_ONLY scalars, synthetic WebAuthn, a memory writer, Playwright routing and a Node TLS bridge. It is not actual enrollment, native installed-dialog, browser WebPKI, real CLI, or phone acceptance.

## Controlled-module integration

Create one presenter with the already-approved HTTPS origin and supply that exact callback as `handles.present` alongside the independent approved configuration, original non-exportable keys, and relay signer. The same-origin `/browser-profile.js` export remains only `readApprovedBrowserInputs`; adding the presenter does not grant permission to call the enrollment owner's `approve` or persist anything. The default example remains Locked.

The production dialog styles are included by `src/browser-main.ts` through `browser-remote.css`. Cancel is initially focused without scrolling the grant heading out of view. The actions stay visible while long details scroll, including at a 390px viewport. All descendants' text/attribute/structure mutations reject the pending prompt; exact original markup is rechecked on the trusted click. This guards accidental stale presentation, not malicious same-origin code or external CSSOM manipulation.

The broker, not this dialog, owns the original grant deadline. Full-chain cancellation, owner revocation and deadline expiry must leave WebAuthn at zero calls, with no additional signing after the established channel's original TLS work, no Host installation and no writer dispatch. The acceptance case still requires the original synthetic WebAuthn proof to pass Rust verification before one exact writer effect.

Because the operator module is bundled separately, root app typechecking alone is not sufficient. Run its strict check explicitly from the repository root:

```powershell
node node_modules/typescript/bin/tsc --noEmit --strict --noUnusedLocals --noUnusedParameters --target ES2022 --module NodeNext --moduleResolution NodeNext --allowImportingTsExtensions --lib ES2023,DOM remote/ui-terminal/src/browserGrantPresenter.ts remote/ui-terminal/src/browserEnrollment.ts
```

The standalone actual Edge suite is `tests/remote-browser/browser-grant-presenter.spec.ts`. The full-entry suite's `ProductionPresenter` cases require the same explicit original Relay/Host/WASM artifact environment as [Browser profile acceptance](../remote/BROWSER-PROFILE.md#supplier-through-the-complete-browser-entry-and-original-relay).
