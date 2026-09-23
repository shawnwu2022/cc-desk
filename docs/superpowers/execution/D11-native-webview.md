# D11 real native document boundary

Plan: approved native CLI v3 W2/D11. Baseline: 7152ab3eb3e53c71d2a231ac994b47f0bf3782fc, PR #19. D11 remains IN_PROGRESS.

## Rulings and task brief

Ruling: execute the existing document adapter inside a disposable real Wry/WebView2 app before enabling production IPC — compiled ResourceTable/VM tests do not establish document-event or raw-header behavior — a missing runtime or callback is a failing native boundary test, not an implicit pass.

Ruling: the existing Windows cargo CI invokes a supervised child test process twice, for reload and destruction. Each process uses a separate test identifier and temporary WebView data directories; the production startup, plugins, workspace store, CLI accounts and processes are never loaded. No permission, dependency, version, lockfile or real application configuration changes.

Ruling: use event callbacks to start the probe, and condition-based bounded observation for revocation. The 10 ms monitor interval is only a deadline/polling mechanism, not an input flush, event-order workaround, or assumption that a sleep makes readiness correct. The parent kills/reaps its owned worker on a 90 s timeout. No browser callbacks or authority revocation are manually simulated.

Ruling: the initial native acceptance run need not manufacture a defect in already implemented code. The report verifier does require observed RED/GREEN: it initially accepts everything, while missing-case, wrong-identity, duplicate/wrong-result and sticky-failure tests require rejection. Both native execution and this independent completeness check must succeed before reporting native acceptance.

Pre-flight: native build_main -> real Webview resource table -> start_native/query_native -> existing registry; public application launch endpoints remain separate. Launch parsing exercises UTF-8/empty/spaced argv but deliberately does not spawn an agent. Test-only commands are registered only on the disposable app under cfg(test, windows).

## Scope and expected observations

Real initialization script, raw UTF-8 launch decoding, retained-status not-found response, missing/wrong/combined proof, forged owner, JSON-body rejection, inclusive 1024-byte status boundary, 1025-byte overflow, and an actual second WebView with the deliberately copied valid proof. The rejected peer must leave main authorized. Reload must revoke through real callbacks and reject another real IPC call. Native destruction must be observed and revoke the registry while the binding is still retained.

Evidence is assembled by Rust from actual native adapter results, not a JS success report. It contains only test-case result codes, main page-event kinds, test mode, target and the installed WebView2 version obtained through tauri::webview_version. It must contain every required case once, correct outcomes and no sticky failure. Proof, request, argv, paths and headers are not included in evidence.

## Execution

Initial test-only commit: report rejection tests are expected RED. Live test outcome is NOT_RUN until CI actually executes it. No local Rust toolchain is available; the attempted container git clone failed DNS resolution, so isolated Git tree objects on the existing authorized feature branch and Windows CI are used. Project file retrieval returned no indexed matches twice; already-mounted v3 plan/backlog were read directly. No user working tree was reset, cleaned or stashed.

This is narrow Windows runtime B-layer coverage, not complete production lifecycle, real CLI C-layer or installed package D-layer acceptance. Output route/Channel ownership and rollback, registered production launch/control commands, explicit reauthorization, startup migration, Unix/WebKit and final product acceptance remain open. Do not mark D11 complete based on this probe.
