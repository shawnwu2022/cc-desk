# Windows bundled ConPTY

## Evidence and scope

The affected user verified the local DLL (`local_verified`) and a complete draft
from direct Ctrl+V, without external-editor backfill. The earlier trace compared
normalized clipboard text to Rust input exactly. This supports replacing the
system console backend for this compatibility issue, not a claim about a
specific upstream implementation defect or every Windows/CLI combination.

## Production integration

Windows x64 Tauri builds and development startup run `scripts/prepare-conpty.mjs`.
The official release package, each binary and its license are hash-pinned in
`src-tauri/conpty/manifest.json`. The build-time download is bounded, verifies the
archive, picks the paired x64 files, and verifies each staged file. Cached files
are reverified, not trusted by existence. Binaries are generated/ignored, not
committed. There is no runtime download or user-provided URL.

`tauri.windows.conf.json` maps the DLL, host and license next to the application
in Windows bundles. `build.rs` also stages them next to ordinary/debug/test
executables for `tauri dev` and `--no-bundle`. Plain Windows `cargo` users first
run `node scripts/prepare-conpty.mjs` from the repository root. Other platforms
retain their previous startup/build behavior. Windows ARM64/x86 are deliberately
unsupported until their own runtime artifacts and tests are supplied.

Before Tauri or any PTY starts, `main.rs` calls `conpty_runtime::initialize`.
The loader verifies the compile-time pinned sizes/SHA-256 and x64 PE architecture,
holds read-only handles denying mutation/deletion, loads an absolute path with
`LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32`, verifies
required exports and the basename lookup that portable-pty uses, and retains the
module for the process lifetime. It does not change process-wide DLL search
settings or search the project working directory/PATH for a runtime. Missing,
corrupt, mismatched or load-failed components stop startup with a visible error;
there is no silent fallback to the affected system backend.

A process that can replace the application executable/embedded manifest is outside
this integrity check's threat model. Reinstall/update must close the app first,
as the runtime files are intentionally held open while it runs.

## Automated verification

`node --test tests/scripts/conptyBundle.node.cjs` checks resource mapping and
binary verification failures. Windows `cargo test --locked --bin cc-desk
ConptyRuntime_` checks the actual SHA-256 API, file locks and malformed components.

`cc-desk.exe --check-conpty OUTPUT.json` is a noninteractive, pre-Tauri packaging
probe. It verifies the pinned runtime and exercises portable-pty creation/resize/
close, returning only backend/build/path metadata. It does not start Claude,
load user configuration, or transmit data. Failure exits nonzero, also producing
a JSON error when the output destination is writable.

The integration workflow installs a real NSIS package, repeats installation at
the same version, checks relocation/Unicode paths and a decoy working-directory
DLL, then verifies missing/corrupt components fail closed. Same-version reinstall
is not a cross-version updater test, and this probe is not a full WebView UI test.

Real CLI acceptance uses the same secure initializer when
`CC_PASTE_BUNDLED_RUNTIME=1`, retains all historical strict comparisons and adds
the 28,037-byte/230-LF field shape plus three consecutive pastes. A blocking
UserPromptSubmit hook captures actual submitted text in an isolated configuration.
Differences stay failures; historical TAB/final-LF/control transformations are
not renamed to passes. Tests use hosted Windows Server 2022, not the user's OS.

## Release gate

No merge, version bump or automatic release is part of this integration work.
The previous user-confirmed comparison package remains distinct from an installed,
ordinary (non-tracing) build. The integration/CLI/installation results must be
recorded at an exact commit before calling a new installer release-ready.

References:
- https://v2.tauri.app/develop/resources/
- https://v2.tauri.app/reference/config/
- https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-loadlibraryexw
- https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcrypthash
