# D10 size validation and Bash diagnostic checkpoint

Observed CI #161 (`35806269023`), source `68687687e6f2cd85f2ab6782061c0a1ba9efbb0d`, merge `23c159519cd08bc95f08604b29b5524c689116c6`. Full Rust job `107007713909` read: compilation and Clippy passed; 434 passed / 5 failed / 15 ignored. Four size-limit tests failed because the resolver accepted oversized input; the positive Native large-environment case passed. This is the RED evidence for the new size checks.

The fifth failure is the still-unresolved Bash roundtrip, with the same apostrophe-merging receiver result. The first hex-word correction is NOT accepted as fixed. Add a test-only boolean marker before each existing Bash shell/shim variant to locate the remaining transition; neither variant nor any argv assertion is removed or weakened. No production diagnostic logs or user values are added.

Size implementation counts the pinned Windows serializer's UTF-16 quoting overhead and final terminator. Cmd has a stricter command and environment-entry bound; Native is not subject to the cmd environment limit. Rejected inputs return fixed ARG_NOT_REPRESENTABLE or ENV_NOT_REPRESENTABLE codes without truncation or fallback. These checks run after constructing the encoded wrapper command, not just on raw argv.

Status: continued implementation and targeted diagnostics; no GREEN or completed D10 claim. Corrected the single test-layout issue reported by CI #161 separately from behavior.
