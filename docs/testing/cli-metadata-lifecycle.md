# Three-CLI offline metadata acceptance

This Windows-only experiment supplements TODO 4 for the 0.27 work (the earlier 0.70 task label was corrected). It does not change the application version.

## Scope

- Claude Code: the installed binary's version and hash, then the pinned official Claude Agent SDK reads, renames, lists, and rereads one synthetic session in a second SDK process. This is not a Claude TUI launch/resume test.
- Codex: the installed app-server reads, renames, and lists one synthetic CLI rollout, then proves the title persists in a second server process. No thread/turn start or resume is sent.
- OpenCode: the installed HTTP server creates an empty session, renames, reads, and lists it, then proves persistence after server restart. No prompt or model request is sent. The driver handles the v1 and v2 metadata API shapes; the recorded real run determines which version was exercised.
- Every check requires exactly one matching session ID and the expected title and project directory. These checks do not certify the CC Desk frontend list or its source/profile binding.

`METADATA_PASS` always retains `nativeLifecycleAccepted`, `guiAccepted`, `modelCalled`, and `installerRoundtripAccepted` as false. It must not be relabeled a full native lifecycle or release acceptance pass.

## Safety and ownership

The ignored Rust tests in `native_cli_metadata_live.rs` are the only supported live entry. They reuse the existing disposable `Fixture` and Windows Job owner. The child is created suspended, assigned to its Job, and then resumed. Input executables and the SDK are hash-pinned and held with read-only sharing. A fresh environment allowlist excludes credentials and explicitly supplies synthetic Windows identity defaults. SDK import is rejected unless its fixture, specification, and complete environment agree.

Only newly generated temporary directories are used. Never aim the driver at an existing user session or launch a copied production CC Desk executable as an isolation workaround. The production application resolves Windows KnownFolders; changing HOME alone does not isolate it. No account, security setting, installation, real credential, or existing session is changed.

The HTTP server binds loopback with a new random password. Responses and child output are bounded. Redirects are rejected; metadata mutations are never retried within a fixture. The Codex method allowlist excludes model execution, login, resume, creation, and deletion. Update/plugin/model-fetch flags and proxy settings reduce unnecessary traffic; they are not a network sandbox and do not prove zero packets were emitted.

## Running

Prepare the repository SDK resources and verified ConPTY runtime as required by the repository instructions. Use the pinned Rust toolchain and set:

- `CC_DESK_NATIVE_METADATA_LIVE_GO=isolated-metadata`
- `CC_DESK_NATIVE_METADATA_NODE` to the exact pinned Node executable
- `CC_DESK_NATIVE_METADATA_SDK` to the exact pinned official SDK module

Run `cargo +1.98.1 test --locked --manifest-path src-tauri/Cargo.toml --lib NativeMetadataLive_ -- --ignored --nocapture --test-threads=1`.

The installed paths and accepted hashes are explicit in the test source. A mismatch fails closed. Changing a pin requires reviewing the new official input and rerunning the checks; do not silently accept whichever executable is on PATH.

Run portable driver contract tests with `node --test tests/scripts/metadataLifecycle.node.mjs`.

## Evidence

Each newly retained fixture has a reports directory. `metadata-result.json` is the child result and cannot prove process cleanup. `metadata-owner.json` is written only after the outer owner has confirmed root reap and an empty Job; it also records forced cleanup and exit status. Missing owner evidence is unresolved ownership, not success. `metadata-owned-result.json` is emitted only after both the protocol checks and normal outer closure pass, and includes executable/driver/SDK hashes.

Failed fixtures remain failed. A corrected driver always gets a new fixture; reports are never overwritten to manufacture a pass.

## Remaining release gates

Authenticated model-backed launch, close, resume, rename, and duplicate-list acceptance through the CC Desk UI remains separate. A disposable Windows VM/account or equally verified isolation boundary is required before stable 0.26.0 -> RC -> stable installation and data roundtrip tests. No qualifying VM was verified on the current host.

A synthetic Codex metadata pass on WSK-MSI does not certify the separately reported SXY real-session rename failure. That incident requires its own source/profile, runtime, and writer diagnosis; this driver must not mutate the reported real record.

## Recorded result (2026-11-12)

The final owned run passed 3/3 on WSK-MSI: Claude Code 2.1.319 with SDK 0.3.301, Codex 0.167.0, and OpenCode 2.2.6. Each reports normal root exit, an empty Job, and no forced outer cleanup. The related Node suites passed 41/41, including 10 new driver contract tests; the new Rust file passed rustfmt checking. An independent read-only review found no blocking issue.

The checked-in [evidence summary](cli-metadata-evidence-2026-11-12.json) preserves the executable, SDK, and exact driver hashes plus outer-owner results. `sourceCommitBuild: local` is only the build stamp, not a Git commit assertion. The base commit and driver hash identify this run; the final commit is recorded by Git. Full GUI/model/native lifecycle and installation roundtrip gates remain open as described above.
