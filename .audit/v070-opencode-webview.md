# 0.70 OpenCode real WebView archive/restore acceptance

## Result and scope

PASS on WSK-MSI, 2026-11-12. Production baseline d4e401491f334934254ec4db654101c98a8493d5. No production source was changed. The real OpencodeArchivePanel, history/catalog stores, authenticated document IPC, production scope authorization, owned helper process, bundled official Session metadata subset and refreshed history ran against a newly copied synthetic database.

The successful fresh NoNative driver took 17.98 seconds. NoNative now means no native CLI/model launch; it DOES run the production restricted Node metadata helper. The executable configured for an OpenCode CLI remains deliberately nonexistent. OPENCODE_DB explicitly selects the freshly created fake database through the supported production selector, avoiding CLI channel discovery.

## Reproduction prerequisites

Before building/running the ignored WebView harness, run node scripts/prepare-opencode-webview.mjs <verified OpenCode 2.2.6 SDK node_modules> from this checkout. It invokes the existing official-SDK synthetic fixture script, requires a newly returned single-component .audit/opencode-sdk-recovery-* directory, and records the seed database/Node executable hashes in .audit/opencode-webview-seed.json. It never accepts a database path. This manifest is now a prerequisite for both ignored WebView driver variants.

This execution used existing verified modules at E:/source/github/cc-box/.audit/task-worktrees/provider-deletion-20261112/.audit/opencode-sdk-probe/node_modules. Node was v24.18.0, resolved physical executable C:/nvm/v24.18.0/node.exe. SDK seed FvSsev was created by the helper, not taken from a user database. The current production helper was rebuilt from d4e40149 sources, 4,658,002 bytes; no stale executable bundle was substituted.

The worker reads the seed through a fixed no-follow file capability, checks its hash and bounded size, then copies it into its fresh fixture/opencode-data/opencode/opencode.db. Only synthetic project directories are rewritten. The selected Node executable is hashed and held deny-write/rename with its ancestor directory capabilities until metadata ownership is settled. XDG_DATA_HOME and OPENCODE_DB point only to this new fixture, while profile PATH contains only the verified Node directory. The production helper retains env_clear, Node permission restrictions, suspended creation and owned Job lifetime handling.

## Exercised flow

1. Existing real Desk archive/restore/rearchive and Claude recoverable removal/restoration steps execute first.
2. Real projects.archiveSession prepares the Desk tag required by the OpenCode panel. This is fixture setup through production persistence, not an OpenCode Desk-row UI claim.
3. The real OpenCode panel opens, selects the synthetic target and proves submit disabled without acknowledgement.
4. Acknowledge and double-click archive. Exactly one authenticated cli_set_opencode_archive request executes; refreshed native history and the actual SQLite state both show archived.
5. Select again, acknowledge again and double-click restore. Exactly one further request executes; refreshed history and SQLite both show unarchived.
6. Real projects.restoreSession removes the fixture tag. Final Desk state still contains only the intended Claude record.

Checkpoints review/archived/restored verify request counts 0/1/2 and real SQLite postconditions. Target fields other than time_archived/time_updated stay fixed; original target event rows stay byte-for-value equal and exactly two correctly sequenced native archive/unarchive events are added. Child/unrelated session rows and non-target events stay unchanged. Snapshots also cover migration, job, schedule, blob, blob_ref, session_message, session_inbox, session_part, credential and project tables. event_sequence is not fully snapshotted, so this is not a whole-database equality claim.

## Evidence

- .audit/opencode-webview-verified.log: PASS, 17.98 seconds.
- Parent fixture I1WDyt; worker eHSeLH under this worktree's .audit/tmp.
- Final success=true; OpenCode checkpoints review/archived/restored; opencodeCalls=2.
- Desk archive/restore/upsert counts 3/2/1, including one setup/removal pair for OpenCode; existing Claude recovery removes/restores 1/1; confirmIpcCount=6 and inspectIpcCount=0.
- Both repository and process metadata admission ledgers settled. actualUdfCount=2. Driver workerExit=0, rootReaped=true, jobEmpty=true, browserCount=2, forced=false, isolationFailed=false.
- One transient browser-lifetime pending notice occurred while the existing gate waited; it resolved before successful completion, with no forced cleanup.
- Report SHA256: ef57382c20ab7c3aba015d5e9997b6d8ec5584d3c4f961bbb2e7fe53f40f57c7.
- Ten screenshots at .audit/tmp/v027-codex-delete-live-I1WDyt/temporary/v027-codex-delete-live-eHSeLH/reports/. OpenCode archived/restored screenshots were visually inspected and show the production native-state-verified message and limitations copy. Database assertions distinguish the two states, which intentionally have similar post-submit visuals.
- Final frontend fixture: 163 transformed modules, 98 pinned production modules. Typecheck passed.
- Final Rust WebView tests: 15 passed, 3 ignored. The successful real ignored driver is reported separately above. Strict all-target Clippy and fmt passed.
- Independent read-only review passed, including final explicit OPENCODE_DB selection and bounded safe-code diagnostics. No review result is substituted for runtime evidence.

## Failed attempts retained

The first attempt o5SNLF/gtr2jX failed before any WebView creation because a fixture PathBuf component contained forward slashes. The strict capability gate was unchanged; only joining separate Windows path components was corrected. The driver reaped the child and observed its Job empty before reporting missing final output.

The next attempts 7yLWkI/JazehW and KfIzWh/VHRgQb passed Desk/Claude but had zero OpenCode mutation requests. Initial generic diagnostics were narrowed to SCOPE_UNKNOWN using fixed allowlisted codes. Without explicit OPENCODE_DB, production channel selection attempted to resolve the deliberately nonexistent fixture CLI. Explicitly binding the new fake database fixed the setup without relaxing any gate. Both failed drivers reported worker reaped, Job empty, two browsers exited, no forced cleanup and no isolation failure. Every retry used a new worker fixture; no uncertain mutation was replayed.

## Remaining boundaries

This proves the production OpenCode archive/restore component to real document IPC to actual restricted official metadata helper and disk/history roundtrip. It is reversible native archive, not permanent deletion. It does not prove full installed-app startup, external active-session behavior, real model lifecycle, release installation/upgrade/rollback, hosted exact-main CI, or three-platform signing. The Native 120-second matrix was not repeated. The existing 70-second NoNative watchdog was retained; only the actual successful result is counted.

No user installation, real session, account, security setting, branch main, push, version or release was changed. Final hashes: .audit/opencode-webview-final-pins.json. Generated resources/schema changes are excluded from the commit.
