# D11 bounded receipts and status-only recovery implementation

## Observed RED before this implementation

Source `36b9ee18ce19244127286d14b5a6161e279f4188`, CI #166 run `35812936492`, tested merge `75402a71ff5726c495a70a6fe9e4c21b10b2485d`:

- Full Rust job `107028233438` read: compilation and Clippy passed; 460 passed / 3 failed / 15 ignored. The three new failures were exactly missing routing bounds, request serialization budget and instance/revision receipt fields. Four strengthened existing behavior cases passed. Two test-only rustfmt differences were separate and are corrected without modifying assertions.
- Full frontend job `107028233686` read: typecheck passed; 628 previous tests passed / all 12 new launch-retry tests failed against the deliberate scaffold. Three unhandled scaffold rejections were also reported because two tests asserted before attaching rejection handlers. The tests now attach cleanup handlers to those promises while retaining their identity, result and call-count assertions; no assertion is removed. Policy tests/build were skipped after failure and are not counted as passed at this RED checkpoint.

## Implementation

- Routing IDs retained in records are bounded to 128 UTF-8 bytes and reject controls. The canonical launch request is serialized incrementally into a process-keyed fingerprint sink with an 8 MiB budget. Oversize is explicitly rejected before preparation; no prompt is truncated or second full serialized-prompt Vec retained. The original IPC decode budget is still a separate live integration concern.
- LaunchStatus includes the backend instance and canonical u64 revision. Phase changes increment revision; repeated exit and resource retirement do not. The private state graph is finite and cannot wrap its counter.
- Frontend attempts own and freeze the exact validated request, memoize a single start Promise and recover only by querying the original request ID. Transport failures become fixed unknown-outcome errors, never automatic retries or newly allocated IDs.
- Receipts are schema/identity/version checked before replacing state. Wrong instance, malformed fields, equal-version conflicts and impossible phase reversals fail without changing the previous state. Delayed lower revisions cannot overwrite newer exit states.

This is a candidate pending the next CI. Core registries, fake-resource race tests and a transport-injected frontend policy are not live WebView/document authorization or a complete D11 integration. No new Tauri spawn endpoint or UI route is enabled; D10 and prior production PTY behavior remain unchanged. Review is author review only.
