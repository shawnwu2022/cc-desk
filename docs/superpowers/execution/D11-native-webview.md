# D11 real native document boundary

Plan: approved native CLI v3 W2/D11. Baseline: 7152ab3eb3e53c71d2a231ac994b47f0bf3782fc, PR #19. D11 remains IN_PROGRESS.

## Rulings and task brief

Ruling: execute the existing document adapter inside a disposable real Wry/WebView2 app before enabling production IPC — compiled ResourceTable/VM tests do not establish document-event or raw-header behavior — a missing runtime or callback is a failing native boundary test, not an implicit pass.

Ruling: the existing Windows cargo CI invokes a supervised child test process twice, for reload and destruction. Each process uses a separate test identifier and temporary WebView data directories; the production startup, plugins, workspace store, CLI accounts and processes are never loaded. No permission, dependency, version, lockfile or real application configuration changes.

Ruling: use event callbacks to start the probe, and condition-based bounded observation for revocation. The 10 ms monitor interval is only a deadline/polling mechanism, not an input flush, event-order workaround, or assumption that a sleep makes readiness correct. The parent kills/reaps its owned worker on a 90 s timeout. No browser callbacks or authority revocation are manually simulated.

Ruling: the initial native acceptance run need not manufacture a defect in already implemented code. The report verifier does require observed RED/GREEN: it initially accepts everything, while missing-case, wrong-identity, duplicate/wrong-result and sticky-failure tests require rejection. Both native execution and this separate completeness check must succeed before reporting native acceptance.

Pre-flight: native build_main -> real Webview resource table -> start_native/query_native -> existing registry; public application launch endpoints remain separate. Launch parsing exercises UTF-8/empty/spaced argv but deliberately does not spawn an agent. Test-only commands are registered only on the disposable app under cfg(test, windows).

## Scope and expected observations

Real initialization script, raw UTF-8 launch decoding, retained-status not-found response, missing/wrong/combined proof, forged owner, JSON-body rejection, inclusive 1024-byte status boundary, 1025-byte overflow, and an actual second WebView with the deliberately copied valid proof. The rejected peer must leave main authorized. A reload attempt must revoke through real callbacks and reject another real IPC call. The current adapter blocks subsequent navigation; this does not assert a replacement document loaded or transparent reload recovery. Native destruction must be observed and revoke the registry while the binding is still retained.

Evidence is assembled by Rust from actual native adapter results, not a JS success report. It contains only test-case result codes, main page-event kinds, test mode, target and the installed WebView2 version obtained through tauri::webview_version. It must contain every required case once in order, correct outcomes and no sticky failure. Proof, request, argv, paths and headers are not included in evidence. Reports are checked in both worker and parent, irrespective of GUI process exit status.

## Observed failures and root-cause experiment

- fe83cb5d, CI #179 (35829747113), Windows job 107079357959: Rust compilation and Clippy passed, but the library test executable stopped before running any tests with 0xc0000139 / STATUS_ENTRYPOINT_NOT_FOUND. Five new test-file layout changes were also required. Frontend succeeded. This is not report-verifier behavioral RED or native acceptance.
- Diagnostic-only 20401447, CI #180 (35830403391), job 107081422640 reproduced that loader failure. The initial PE diagnostic itself failed on repeated kernel32 import descriptors; no DLL conclusion was claimed from that broken diagnostic. Clippy and frontend succeeded; formatting remained.
- Diagnostic-only 0808bcb8, CI #181 (35831069122), job 107083540844: merged repeated DLL descriptors and examined the same compiled executable. comctl32 resolved to the 5.82 assembly, missing TaskDialogIndirect. mt.exe confirmed the original test executable had no resource section/manifest. A temporary copy with the pinned Tauri default Common Controls v6 manifest could list all 522 tests successfully. The original executable's SHA-256 was checked unchanged. Original cargo test remained failed; diagnostic success did not mask it.
- The manifest-only copy ran all four report-verifier tests: 0 passed / 4 failed / 0 ignored, exit 101. Missing cases, invalid schema/identity, duplicate/false success and unsupported mode were rejected by assertions but accepted by the still-empty verifier. This is the observed behavioral RED, read before implementation.
- The same copy started real Wry/WebView2 version 131.0.2903.86 for both modes and observed initial Started/Finished. The first raw request was ADMITTED, but both reports had zero observations and sticky failure PROBE_MISMATCH:0:missing-case:ADMITTED. The parent falsely returned success because the verifier still accepted everything. These raw reports are NOT native acceptance; the report verifier is required even when Tauri run_return or a worker exit appears successful.

## Corrections grounded in those observations

- The pinned Tauri scripts/core.js defines __TAURI_INTERNALS__.invoke using a non-writable Object.defineProperty. The test fixture's assignment-based spy silently did nothing, so its initial test header never appeared. The corrected fixture never replaces/mocks native transport. Its first request still traverses the unmodified production bridge; the test-only handler returns the proof it just validated so later adversarial calls can reuse it. This deliberately disclosed test proof never enters diagnostics, a production endpoint, or the report.
- The verifier now checks schema, mode, target, engine/version, initial native page events, every exact ordered observation and the absence of a sticky failure. All four original negative/positive tests are retained. Process success alone is insufficient. Passing evidence is written directly to stdout so normal cargo capture does not hide it from CI logs.
- MSVC build.rs now embeds the same Common Controls v6 declaration through /MANIFEST:EMBED and /MANIFESTINPUT, following the pinned Tauri example. The default manifest is omitted from Tauri's resource archive to avoid duplicate embedding; its icon/version resources remain. /MANIFESTUAC:NO avoids synthesizing a new UAC declaration absent from the original default. Non-MSVC builds retain tauri_build::build. Existing ConPTY preparation/copy code is unchanged. This is a Windows build-linking correction, not a dependency upgrade or a claim that the production startup path changed.
- The temporary loader diagnostic script and its CI step are removed after establishing the cause. Full cargo tests, formatting and Clippy remain. An additional bounded smoke step builds the actual Windows application and invokes its existing --check-conpty path; it checks a temporary safe report without loading user configuration or starting a CLI. This guards the main executable affected by relocating manifest embedding, not just test binaries.

## Source references and verification limits

Pinned Tauri tag tauri-v2.10.3: examples/api/src-tauri/build.rs uses new_without_app_manifest plus general link arguments for tests; crates/tauri-build/src/windows-app-manifest.xml supplies the unchanged declaration; crates/tauri/scripts/core.js establishes read-only invoke. Cargo rustc-link-arg-tests does not cover library unit-test targets (rust-lang/cargo #10937). Microsoft /MANIFESTUAC documentation defines NO as omitting linker-generated UAC information.

No local Rust toolchain is available; the attempted container git clone failed DNS resolution, so isolated Git tree objects on the existing authorized feature branch and Windows CI are used. Project file retrieval returned no indexed matches twice; already-mounted v3 plan/backlog were read directly. No user working tree was reset, cleaned or stashed. The initial local VM fixture check incorrectly modeled invoke as writable and is not native evidence. Author self-review is not independent code review.

The corrected successor still requires a full CI run. Record final results against its exact head in PR metadata without an untested documentation successor. This is narrow Windows runtime B-layer coverage, not complete production lifecycle, real CLI C-layer or installed package D-layer acceptance. Output route/Channel ownership and rollback, registered production launch/control commands, explicit reauthorization, startup migration, Unix/WebKit and final product acceptance remain open. Do not mark D11 complete based on this probe.
