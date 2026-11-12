# 0.70 Desk archive/restore persistence acceptance

## Scope and result

PASS on WSK-MSI, 2026-11-12. This extends e595f0209a292ffbba042030b7b68c5e25e2dbe5 (integrated production baseline f699f0b0) with a test-only held-capability projects store and the unchanged production SessionList/ArchivedSessionsDrawer, catalog adapter, Tauri commands, state transaction/serialization and admission ledger.

The fresh NoNative host exercised: real inline archive button -> archive_session -> disk read/reload -> real drawer restore button clicked twice -> one restore_session and one upsert_session_ui_record -> disk read/reload -> archive again. It then repeated the real Claude recovery component remove/restore path. There was no in-memory archive-label seeding, CLI launch, model call, installed-app launch or access to a real user data root.

## Test-only capability boundary

The cfg(test, windows) projects fixture has a process-once held directory, a fixed held lock identity and independent transaction lock handles. Once the worker marker is observed, absence/failure of installation cannot fall back to ambient paths. The worker tests missing and repeated installation before loading any page. The normal GUI config resolver rejects use in this worker.

All projects IO checks exact capability paths, uses relative no-follow handles, rejects reparse/hard-link reads and bounds bytes. Writes create a unique private file, write/sync/verify the original held handle, and publish that same handle to the one fixed leaf with relative NtSetInformationFile. No path reopening, fallback, rollback or automatic replay is added. Production rename_to remains non-overwriting. The original non-test OpenOptions sequence was extracted unchanged to open_projects_lock.

This proves production UI/commands/state logic with test-scoped IO. It does NOT prove the original production path-based ReplaceFileW IO or a production path override. The new capability code is excluded from normal builds.

The original document proof, own Job, exact browser handles, UDF checks, parent directory leases and worker reap gates remain. Both the repository admission ledger and process_admissions used by formal projects transactions must settle before final success/worker exit.

## Validation

- Frontend build and npm run typecheck passed: 157 transformed modules, 94 production source modules recorded and pinned.
- Final Rust WebView unit suite: 15 passed, 3 ignored (real GUI tests excluded from this count).
- Final store/capability selection: 36 passed, including 9 new capability negative tests.
- Strict all-target Clippy (-D warnings): passed.
- Independent read-only review passed after fixing irreversible install observation, both-ledger settlement, exact state identity and normalized project-path assertions.
- New negative tests cover independent lock contention, held root/lock/temp resistance, wrong paths, real synthetic hardlinks and junctions, same-name target changes, pre-publication failure, post-publication unknown receipt with one issue/no rollback, and failed installation remaining required. Junction creation must actually succeed before the rejection checks.
- The local OnceLock install-failure test checks the latch primitive; fresh worker separately asserts absent and duplicate installation. These are not a real crash/restart recovery test.

Logs: .audit/desk-webview-tests.log, .audit/desk-store-tests.log, .audit/desk-capability-tests.log, .audit/desk-clippy.log.

## Fresh NoNative evidence

.audit/desk-no-native-final.log: PASS in 6.26 seconds. Parent Zx05oB; worker VQlp0M, all under this worktree's .audit/tmp.

Final report: success=true; deskCheckpoints=[archived,restored,rearchived]; deskArchives=2, deskRestores=1, deskUpserts=1; confirmIpcCount=6, inspectIpcCount=0; recycleRemoves=1, recycleRestores=1; actualUdfCount=2; metadataOwnerSettled=true.

Driver: workerExit=0, rootReaped=true, jobEmpty=true, browserCount=2, forced=false, isolationFailed=false. Both admission ledgers were checked before the settled field is reported.

Each Desk checkpoint read persisted state through the production locked transaction reader. It verified the exact normalized project key, complete native-history-v2 row key (including context revision and actual source root key), corresponding metadata key/runtime/CLI/project/adapter/native identity, saved lastOpenedAt, and no unexpected pins, titles or launch preferences. The Claude transcript, child sentinel and index controls remained as expected.

Reports/screenshots: .audit/tmp/v027-codex-delete-live-Zx05oB/temporary/v027-codex-delete-live-VQlp0M/reports/. desk-archived.png and desk-restored.png were visually inspected: the synthetic Claude row moves out of the archive drawer into the live list on restore. Seven captures include the final rearchive and recovery steps.

webview-result.json SHA256: 99c32a1ca815a0595debaef3031df9cffd9f27be9643187d27b1212d7f2770c3.

An earlier fresh attempt (.audit/desk-no-native.log, parent KUsXO6/worker 24bC6I) failed before any Desk mutation (0/0/0) because the driver selected a nonexistent context-menu archive item instead of the production inline archive button. Its final generic failure is retained. All owned processes settled (no forced cleanup). Only the test selector was corrected, rebuilt/typechecked and independently reviewed; the successful run used a new fixture, not a replay.

## Remaining RC boundaries

No Native long matrix was repeated for this incremental Desk extension. The earlier e595f020 Native proof remains documented in .audit/v070-recycle-webview.md, but is not claimed as validation of these new Desk steps.

This is not full production-app startup, a signed installed-app test, upgrade/rollback, a real-model session lifecycle, or original KnownFolder/ReplaceFileW persistence evidence. An independent Windows VM/account remains unavailable for the installation boundary. Existing installed programs, user sessions/accounts and security settings were unchanged. No push or release was performed.

Final source, assets, binary and report pins: .audit/desk-final-pins.json. Generated resources/schema files and all previous failure evidence remain outside this commit.
