# 0.70 production recycle drawer WebView acceptance

Base: f699f0b0defb4b1349a11b53a1dfc4cf29e934d7. WSK-MSI, 2026-11-12.
Worktree: E:/source/github/cc-box/.audit/task-worktrees/recycle-webview-20261112.

## Scope

The existing isolated test host now renders the actual production ArchivedSessionsDrawer and its NativeSessionRecyclePanel. It uses the production frontend API, history/recovery stores, authenticated document bridge, projection service and Windows recoverable-removal implementation. No production source file was modified. The asset manifest records 94 production modules and verifies their source hashes before serving the immutable bundle.

A synthetic Claude profile explicitly selects a nonexistent executable and a new fixture-owned CLAUDE_CONFIG_DIR. The host never starts Claude. One synthetic Desk archive label is inserted in memory as initial state; this is not Desk label persistence acceptance. Only the original Claude root transcript is moved to a same-volume recovery area. Child-directory and sessions-index bytes remain at their original paths and are checked unchanged. The UI reads the real stored record, restores it, refreshes history and shows the original bytes again.

The original parent-held directories, actual main/peer UDF checks, exact browser handles, owned Job containment, worker reap, and metadata-owner settlement remain intact. Both successful finish and post-owner-settlement verification enforce complete Codex/Claude file and dispatch postconditions. Double-clicks must produce exactly one remove and one restore. NoNative still has no runnable CLI. Native separately uses the previously pinned Codex 0.167.0 solely on newly created synthetic data.

## Validation

- Frontend fixture build: passed, 157 transformed modules, 94 recorded production modules.
- npm run typecheck: passed.
- Final Rust WebView module: 15 passed, 3 ignored. The ignored cases are real GUI/CLI tests and are not included in this count.
- Strict all-target Clippy: passed.
- Independent read-only review: passed after fixing command macro qualification, capture guard lexical scope, and final postcondition revalidation.
- Test 015 injects failures into the final-check composition. It is not a real late file-write integration test.

Setup/compile failures were not counted as passing tests: new-worktree ConPTY extraction failed in the local PowerShell Expand-Archive implementation; the same pinned runtime was copied from the prior project-local worktree and successfully verified by prepare-conpty. Missing SDK resource nesting was corrected from existing project-local resources. A Tauri command macro registration error and a MutexGuard-across-await compile error were fixed before final verification. No system installation, security-setting change or cleanup was used.

## Real NoNative run

.audit/recycle-no-native.log: PASS, 5.12 seconds.
Parent fixture KZKoqS; worker vlAfnC under .audit/tmp/v027-codex-delete-live-*.
Final report: success=true, confirmIpcCount=6, inspectIpcCount=0, recycleRemoves=1, recycleRestores=1; recycle checkpoints review/stored/restored; actualUdfCount=2; metadataOwnerSettled=true.
Driver: worker exit 0, rootReaped=true, jobEmpty=true, browserCount=2, forced=false, isolationFailed=false.

Screenshots in .audit/tmp/v027-codex-delete-live-KZKoqS/temporary/v027-codex-delete-live-vlAfnC/reports/:
- recycle-review.png: missing acknowledgement prevents removal.
- recycle-stored.png: real recovery record and restore action; archived source row absent.
- recycle-restored.png: restored status and refreshed archived source row.
Stored/restored screenshots were visually inspected; scope/risk text and actions were visible.

## Native run

.audit/recycle-native.log: PASS, 155.22 seconds. Parent VQE8hF / worker gtN0Fk.
All 12 Codex checkpoints and all 3 Claude recycle checkpoints passed. Final counts: confirmations=10, inspections=1, Claude removes=1, restores=1; actualUdfCount=2; metadataOwnerSettled=true. Driver: worker exit 0, rootReaped=true, jobEmpty=true, browserCount=2, forced=false, isolationFailed=false.

This run actually repeated the production Claude UI recovery path, then performed the fixed Codex 0.167.0 synthetic deletion, duplicate-confirm receipt checks, read-only inspection and the production 120-second preview expiry. The post-owner-settlement verification also rechecked the restored Claude original and unchanged child/index controls.

Report and six screenshots: .audit/tmp/v027-codex-delete-live-VQE8hF/temporary/v027-codex-delete-live-gtN0Fk/reports/.
webview-result.json SHA256: 6AB3CBC1354B5FB06F862B006B6CA7EBF765A7DCDC5A7DAFD23E8DCDFE827BE6.
assetBuildHead is exactly f699f0b0defb4b1349a11b53a1dfc4cf29e934d7; all production source hashes are pinned and unchanged. Only test host/driver/build fixture code differs from that integrated base.

## Remaining boundaries

This establishes the production drawer/recovery component path through real document IPC, not the installed full app. It does not prove ordinary Desk archive-label persistence, the drawer's ordinary restore-archive event, CLI picker behavior, full application startup, installer upgrade/rollback, real-model lifecycle or descendant deletion completeness.

The ordinary Desk archive/restore commands use commands.rs apply_projects_state_blocking -> store::data_and_lock_paths, which resolves the real KnownFolder path. They remain deliberately unregistered in this host. A future test-only, process-once capability-held path injection could be reviewed to exercise those unchanged persistence operations; overriding HOME alone is insufficient and was not used.

Pins: .audit/recycle-final-pins.json. Original worktree and all failure evidence are retained. No user installation, real session, account or security configuration was changed. No push or publication.