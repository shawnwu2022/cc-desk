# D12 implementation verification and handoff

## Verified code revision

Code commit: `9b8e24a53477ec971f58058325dec5cf0b7ed1eb`.
Code tree: `ec5316c07cd8c8bc1428b9484cc2fc6d1de0b182`.
D11 base: `bc8ef7362328227cff49859d86a10eaf894bc3c7`.
PR #20 remains a stacked draft, not merged or released.

The D12 backend/read-chain implementation gate is satisfied at this code revision.
This record and the accompanying AGENTS.md correction change documentation only.
Latest-head CI results belong in the PR record; do not attribute a previous run to a newer commit.
The architecture, reader inventory, supported formats and limits remain in [D12.md](D12.md).

## Acceptance mapping

| Requirement | Production path and verification |
|---|---|
| Actual document admission before untrusted decoding | Formal native_get_scope/native_list_resources commands, NativeRuntime, nine-case Wry/WebView2 fixture |
| Backend-owned profile/run scope | ProjectionService and ScopeRegistry; real profile revisions/deletion, authorized frozen run snapshot, document revocation and exact SourceRef checks |
| Root confinement and invalidation | Retained cap-std handles; real two-root fixtures, replacement or Windows delete-sharing denial, symlink/junction escape, invalid text, bounded scans |
| No default-root fallback | Selection tests for explicit invalid roots, unrelated home, shell/raw/unsupported launcher or argument semantics; unavailable instead of guessed authority |
| Scoped readers | Kind-specific history/messages/search/config/MCP/skills/agents/plugins/instructions catalog with no CLI execution or arbitrary ambient descendant reads |
| Derived cache isolation | Existing schema-v2 source/epoch/project partition and v1 rebuild regressions retained; authenticated catalogs use fresh bounded reads |
| Stale frontend replies | Exact source/kind/requestEpoch validation; selection ownership rejects late success/error/finally and caller mutation |
| Workspace-authoritative home | Registered projects retained when native enrichment fails; native reads never grant a project from transcript cwd |
| No generalized native deletion | New API/store boundary tests; workspace remove/hide is independent of native history files |

## Executed verification

Full CI #213: run `35936165965`, Windows Rust job `107433578834`, frontend job `107433579052`.
The PR merge preview tested was `4b0b58b00b0034c3a643aaec261448006778995a`; this does not merge the PR.

- Frontend typecheck, complete frontend suite, Node release-policy tests and production build passed.
  The same code was rerun locally: 679 tests in 59 files, 3 Node policy tests, typecheck and build passed.
- Windows `cargo test --locked`: library 608 passed / 0 failed / 20 ignored; main 6 passed;
  paste_claude_e2e configuration tests 2 passed / 3 ignored; paste_transport 8 passed; doc-tests 0.
- D12_Webview_FormalCommands_001 passed through actual Tauri/Wry/WebView2 and both formal commands.
  The passing parent requires exactly nine ordered observations and a nonempty actual engine version:
  independent roots, missing proof, raw body required, forged path, forged reference, body budget,
  profile revoked, peer rejected, reload rejected. The ignored isolated worker is executed by that parent.
- `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` passed.
- Actual Windows application build and loader passed:
  `D11_APPLICATION_LOADER ok=true backend=bundled ptyLifecycle=true`.
- Core run `35936165975`: ubuntu-22.04, windows-2022 and macos-14 all passed tests and strict Clippy.
  The harness compiles the actual production reader/registry/selection sources, not a second implementation.
- Author self-review, exact Git tree verification and `git diff --check` completed.
  No independent reviewer is claimed.

## Failures preserved, not hidden by the final result

- CI #210 (`35934790646`): library 601 passed / 4 failed / 20 ignored.
  Two Windows fixtures assumed PermissionDenied instead of checking the actual sharing-denial code;
  both now assert the original bytes/title remain readable. D11 and D12 live workers also timed out.
- macOS core initially failed while creating a raw-byte filename (EILSEQ), before the production reader.
  The fixture now always tests production invalid-path rejection, and either tests stored-name rejection
  or explicitly verifies macOS rejected creation. The Unix symlink escape test still runs independently.
- CI #211 (`35935474364`): library 607 passed / 1 failed / 20 ignored; only D12 live worker timed out.
  D11 launch passed unchanged. That earlier D11 timeout remains an unexplained intermittent observation,
  not a defect falsely claimed fixed by this change.
- D12 live helpers used synchronous window construction/native admission. They now use async commands;
  reload assertions observe revocation caused by actual navigation rather than relying on Finished.
  The fixture never manually revokes authority, keeps all nine assertions, and retains its 90-second deadline.
  Both full CI #212 (`35935934981`) and #213 passed after these corrections.
- Three additional metadata regressions were observed RED before their fixes: missing project-local MCP
  under a custom root, custom plugin layout silently looking empty, and duplicate installations collapsing
  into a single version. The complete local core suite then passed 38 tests with strict Clippy.

## Decisions and limits

1. The coherent recovered tree `4c6ddf7` was selected over the divergent delivered `de0b297` continuation.
   It already integrated frozen-run authority and formal WebView tests. The delivered snapshot remains
   preserved for comparison; its 685/138 test counts are not attributed to this implementation.
   Cost of choosing the wrong continuation would be a lost non-equivalent edge case, so targeted metadata
   regressions were evaluated against the selected production path rather than mixing incompatible APIs.
2. Multiple matching installations for one plugin ID return SOURCE_AMBIGUOUS instead of inventing native
   precedence. Legitimate multi-installation metadata can consequently be unavailable until represented separately.
3. Custom plugin resource paths return SOURCE_UNSUPPORTED instead of claiming an empty complete catalog.
   Native CLI functionality is not blocked; the cost is unavailable GUI projection for that layout.
4. SourceRef basis is configured-profile or launch-environment, not verified effective live CLI identity.
   Unknown shell/raw/shim/argument semantics remain unknown. Filesystem confinement is not a CLI sandbox,
   an immutable content snapshot, or proof of effective config precedence. Hard-link and uninterruptible
   filesystem-call limitations remain documented in D12.md.

No D12 self-review finding was silently deferred. Independent review is still absent, and dependency
advisories reported earlier remain untriaged; passing functional CI is not a clean security audit.
Real model-backed Claude/Codex workflows, packaged installations, and full macOS/Linux WebView applications
were not certified here. Ignored real-CLI tests are not counted as passing.

## Next boundary

D13 has not started. D22-D24 own complete dual-CLI tab/UI adoption; the old Claude UI remains legacy-only.
New dual-CLI code must use the authenticated API/store, never the old default-root/delete commands.
NATIVE_RUNTIME_NOT_READY remains closed for the later transport/lifecycle gates. No main merge, tag,
release, version change, native-history/config mutation or unrelated locked-dependency upgrade is included.
