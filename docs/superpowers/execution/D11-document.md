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

Initial test commit introduces 15 Rust behavioral tests, 5 JS bootstrap tests and unimplemented admission/bootstrap scaffolds. Cover first load, native identity, labels/URL/proof, second attach, reload/navigation/stale completion, owner forgery, raw decode budgets, retained query, cancellation before spawn, resource retention after revoke and stale destruction versus replacement. Tests use production coordinator and actual Tauri ResourceTable/InvokeBody, not a real browser engine.

## Observed RED

- 2364e9a, CI #174 (35825972962): frontend 641 passed / 4 expected bootstrap failures; the foreign-document negative test already passed the empty scaffold. Typecheck passed. Rust did not reach tests: E0277 in new test assertions required Debug on the intentionally non-Debug CallerIdentity; formatting also failed. This is not behavioral Rust RED evidence.
- 014f544c corrects only assertion mechanics and formatting. Equality remains checked without printing identities; generic rejection checks still fail on unexpected success. Production CallerIdentity was not changed.
- CI #175 (35826581482), Windows job 107069534552: Rust compiled, 482 existing library cases passed, all 15 new document cases failed for missing behavior, 15 existing ignored. Clippy passed; test-file layout still required rustfmt adjustments. Complete failure and formatter output were read before implementation. No old test failed. Formatting corrections below change no assertions.

## Implementation boundary

- Document state is mutex-protected. Only an initial navigation and initial Started/Finished sequence can establish Ready. A second navigation, reload, out-of-order initial finish or unexpected URL revokes permanently. Duplicate Finished for the current ready document is harmless; it cannot resurrect Revoked.
- Only one native table can attach. Admission compares the stored Arc witness by pointer identity, not just its type or numeric ID; a same-ID replacement fails. Proof must be a single 32-byte header. Diagnostics do not expose proof, URL or request contents.
- Start accepts at most 8 MiB raw bytes, status query at most 1024. JSON bodies are rejected rather than serialized again. Status query schema rejects extras and reads only the original retained request. No new request ID, route installation or process spawn occurs in this layer.
- Native adapter installs per-builder navigation/page-load handlers and a per-window destruction listener before Ready, preserves WindowConfig via from_config, and obtains context from the injected Webview. Resource-table locks are released before typed decoding and registry access. Window creation must be serialized by the application lifecycle; build_main is not a concurrency coordinator or a registered endpoint.
- Failed builds revoke the exact minted epoch. Failed post-build attachment attempts to destroy only the newly returned native window; it does not search by label or destroy some replacement. Resource cleanup errors do not reopen authority. Running process ownership is untouched.
- The bootstrap exposes only a non-replaceable frozen invoke bridge, serializes raw UTF-8, rejects serialization errors without dispatch, rejects missing transport, and never retries or falls back to an unauthenticated call. The token is hidden from ordinary object enumeration, not a defense against compromised same-origin JavaScript or DevTools. Native provenance and revocation remain backend responsibilities.

The standalone Node VM bootstrap assertions were run locally after observing JS RED and passed, including raw Unicode bytes, foreign/subframe exclusion, non-replacement, serialization failure, no retry and absent transport. This is not local Vitest or real WebView evidence. Final integrated CI results must be recorded against the exact tested head in PR #19.

## Native sources checked

Pinned Tauri tag tauri-v2.10.3: crates/tauri/src/webview/webview_window.rs (builder, page load, navigation), crates/tauri/src/resources/mod.rs (ResourceTable/add_arc/get/replace), crates/tauri/src/ipc/mod.rs (Request headers/body and InvokeBody). The existing lockfile is unchanged. Webview equality based on label was rejected as an instance-authentication mechanism.

## Remaining acceptance

Live endpoint registration, safe output Channel ownership/rollback, actual document event ordering across WebView2/WebKit, host transport/header behavior, explicit reauthorization and application lifecycle/startup migration remain unverified and are not certified by these tests. The new adapter is compiled but not called from lib.rs. No Codex launch button, new IPC command or active terminal path is enabled. Do not claim D11 complete or native authentication accepted before that integration and live testing.
