# Windows ConPTY integration

This file supplements the repository root's architecture/testing rules.

- Windows desktop startup now verifies and pins the bundled ConPTY before calling `cc_desk::run`. Never remove that guard or add silent system fallback to turn a test green.
- `conpty/manifest.json` pins the field-verified x64 DLL, OpenConsole host, upstream package and license. Keep the pair, hashes, architecture and license synchronized; do not commit downloaded binaries.
- `tauri.windows.conf.json` prepares resources for normal build/dev; `build.rs` places the same files beside Cargo binaries/tests. Windows direct cargo commands require `node scripts/prepare-conpty.mjs` first. Other platforms are unchanged.
- Production input bytes, bracketed-paste framing and writer remain unchanged. External-editor workarounds are not a fix.
- Run Node `conptyBundle.node.cjs`, binary `ConptyRuntime_` tests, strict lint/formatting, the NSIS installation probe and actual `UserPromptSubmit` comparisons. `--check-conpty` verifies loader/PTY lifecycle only, not CLI input content.
- `CC_PASTE_BUNDLED_RUNTIME=1` makes the real CLI harness use the production initializer. Keep historical strict failures separate from confirmed field content loss; do not relax equality, strip TABs, or hide failing samples.
- No automatic version bump, merge or publication. Full details and validation boundaries: `../docs/conpty-runtime.md`.
