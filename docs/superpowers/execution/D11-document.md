# D11 document admission — execution ledger

Plan: approved 2026-09-22 native CLI v3, W2/D11. Baseline: 5e1dc5fb299c0005f51c5c839286aa71facc82a6, PR #19. D11 remains IN_PROGRESS.

## Rulings and scope

- Complete the document admission unit and a compiled native Tauri adapter before opening start/query endpoints. Existing runtime, startup, paste, window creation, versions, lockfiles and native user configuration are unchanged in this increment.
- C02 requires backend caller provenance; C02/D15 assigns renderer loss and explicit reauthorization to lifecycle integration. A native binding permits only its first completed document. Navigation/reload/destruction revoke it permanently. A late Finished event or rerunning a static bootstrap must never restore authority. This conservative loss of automatic reload recovery is intentional, not a transparent TUI recovery claim.
- Window/Webview labels and numeric resource IDs are not sufficient provenance. Bind to an Arc witness in the actual Webview ResourceTable, plus a backend-generated document proof and exact backend-selected local URL. Caller identity/epoch are minted by RunRegistry, never accepted from JSON. Registry revalidates the admitted identity before launch effects; admission is not a reusable capability.
- Authenticate before typed JSON decoding. Accept raw Tauri IPC bytes and apply independent start/query byte limits. This bounds application deserialization, not copies/allocation already performed by WebView/Tauri transport. It is separate from request-fingerprint and paste budgets.
- No external code-review agent or local Rust toolchain is available. Rust tests and native adapter compilation use the existing Windows CI. Author self-review must not be represented as independent review or real WebView acceptance.

## Interfaces and tests

Document binding -> existing RunRegistry/LaunchCoordinator; no replacement registry. Native adapter -> existing pinned Tauri 2.10.3 WebviewWindowBuilder, ResourceTable, Request and InvokeBody. Bootstrap -> raw UTF-8 body plus private proof header; no retry/fallback.

Initial RED commit introduces 15 Rust behavioral tests, 5 JS bootstrap tests and unimplemented admission/bootstrap scaffolds. Observe complete CI failures before implementation. Cover first load, native identity, labels/URL/proof, second attach, reload/navigation/stale completion, owner forgery, raw decode budgets, retained query, cancellation before spawn, resource retention after revoke and stale destruction versus replacement. Tests use production coordinator and actual Tauri ResourceTable/InvokeBody, not a real browser engine.

Live endpoint registration, safe output Channel ownership/rollback, actual document event ordering across WebView2/WebKit, explicit reauthorization and application lifecycle/startup migration remain unverified and are not certified by these tests.
