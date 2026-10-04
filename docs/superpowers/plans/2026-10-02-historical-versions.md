# Historical Version Installation Implementation Plan 

> For agentic workers: Use superpowers:subagent-driven-development for this approved product design. Each task requires focused independent review and an atomic, verified source publication; development commits skip CI. Run concentrated Windows/visual CI only after the cohesive implementation is ready.

Goal: Implement official historical-version selection, authenticated package preparation, recoverable Windows switching with safe Desk-data contexts, and return to the preserved current installation.

Architecture: Rust owns release identities, verified bytes, maintenance admission, snapshots and a crash-recoverable journal. A durable restricted maintenance manager survives replacing the ordinary application. Vue presents versions, compatibility, transaction progress and explicit recovery; it cannot supply executable/download/restore paths.

Spec: ../specs/2026-10-02-historical-versions-design.md

## Reconciled status (2026-10-04)

The original checkboxes below are the implementation brief, not a current claim
that no work has started. This status review uses source
`277d2bedb02da9f047a707a7ca8b0abb7afc907b` and the
[local-to-cloud handoff](../../local-to-cloud-handoff-2026-10-04.md).
Source-connected, host-tested and actual native acceptance are separate outcomes.

| Task | Current source status | Acceptance still required |
| --- | --- | --- |
| 1: catalog | Implemented in `version_history/{catalog,policy,types,commands}.rs`; catalog and frontend contract tests cover the nine observed releases, bounds, platform filtering and selection ownership | Listing or publisher verification does not authorize installation; only 0.17.7 currently has measured payload policy |
| 2: preparation | Implemented in `download.rs` and `verified_package.rs`, with real signature fixtures, bounded private storage, metadata/byte rechecks and document-bound cancellation | Keep exact-source Windows results separate from portable signature-fixture checks and from roundtrip admission |
| 3: snapshots/journal | Normal context preservation/return, startup barrier, retained leases and durable journal are connected; transaction and native custody/checkpoint tests exist | General crash recovery remains incomplete: only sealed, unclaimed checkpoints with reacquired actual ownership can return; unknown/unsealed/claimed-interrupted states remain read-only |
| 4: manager/installer | Restricted manager entry/document/worker and native handoff/coordinator/restore modules are connected; scope, process, fence, bundle, registration and shortcut tests exist | Independent unelevated manager survival, actual historical lifecycle, exact restore and final reopen remain unaccepted; do not bypass an external Job |
| 5: UI | `HistoricalVersionsPanel.vue` uses shared dialogs; `src/manager/VersionManagerApp.vue` is the isolated recovery UI. Stores/contracts cover original-owner actions, stale requests, explicit review and no replay | The plan-only backup destination summary is not displayed; the approved design requires data-consequence disclosure, which is present, but does not require exposing a location. Actual WebView accessibility and history/manager pixel acceptance are not established |
| 6: acceptance | Deny-only compile-time binding, disposable driver, pure contracts, hosted compile policy and synthetic visual fixtures exist | Both native scenarios are `BLOCKED_EXTERNAL_TARGET`; the nine-version install matrix is incomplete. History/manager PNGs are unapproved captures; 13 approved workspace baselines do not certify them |
| 7: delivery | Draft PR34, test-package workflow and scoped recovery/handoff documentation exist | Final evidence must identify source, tested merge, workflow and artifact bytes. Test-only diagnostic changes do not require replacing the installed build; no final roundtrip-ready delivery is claimed |

The Task6 proposal to use a hosted runner for genuine roundtrip acceptance is
superseded by [the disposable-target contract](../../testing/historical-roundtrip-disposable.md).
Hosted compile/fixture checks cannot establish a job-free native target. Keep
`SUPPORTED_ROUNDTRIP_ENABLED=false`, ordinary publication disabled and PR34 draft.
Do not create obsolete planned component filenames when the existing shared
dialog and isolated manager already own those surfaces.

## Global constraints

- Current repository/branch/PR: shawnwu2022/cc-desk, feat/unified-workspace-ux, Draft PR34; starting head006fa33; version0.18.0
- No merge, tag, Release, updater publication, actual user-machine switch, or secret access
- Existing ordinary update eligibility and publication remain disabled
- Only available official, signature-verified and platform-eligible historical artifacts may be installed
- Fresh Desk-data mode must preserve current state for return; no guessed shared-data compatibility
- No automatic restoration of Claude/Codex roots, project contents or user-generated data from a later context
- No changes to the previously excluded AGENTS.md or broad execution ledger

## Review focus

- Mutable/deleted release assets and redirects must never substitute a different installer after review (Tasks1–2)
- Older binaries ignore new locks; multiple application instances and pending CLI ownership must block destructive switching until verified quiescence (Tasks3–4)
- Disk-full, reparse points, corrupt journal and interruption during snapshot/restore must preserve at least one complete recoverable state (Tasks3–4)
- Old versions cannot acknowledge the new protocol or expose its recovery UI; the manager must survive independently and report uncertain outcomes honestly (Task4)
- Navigation, repeated clicks and stale frontend promises must not change selected version or issue duplicate installation/recovery (Task5)

## Task1: Official history catalog and explicit eligibility

Files: create src-tauri/src/version_history/{mod.rs,catalog.rs,types.rs,policy.rs}, src/types/versionHistory.ts; tests in src-tauri/src/tests/version_history_catalog.rs and tests/config/versionHistoryContracts.test.ts.

Interfaces: HistoryRelease (backend identity, version, date, platform availability, verification state, compatibility mode and safe blocked reason); HistoryCatalogPage (rows and backend pagination cursor). list_history(cursor) accepts no URL. select_history(release_id, asset_id) yields an opaque expiring selection token tied to immutable metadata.

- [ ] Write failing parser/policy tests for all nine observed releases, pagination/rate limit, missing/duplicate assets, invalid version/platform, drafts/test artifacts and mutable asset identity
- [ ] Implement bounded official-repository reads, exact product/platform filtering and safe DTO projection
- [ ] Keep shared-data mode blocked unless a reviewed capability rule exists; expose fresh-state mode honestly
- [ ] Verify focused tests and no change to release/promotion policy; independent review; commit/push with skipCI

## Task2: Verified immutable package preparation

Files: create src-tauri/src/version_history/{download.rs,verified_package.rs}; tests src-tauri/src/tests/version_history_download.rs; add only needed existing-lockfile cryptographic dependencies explicitly.

Interfaces: prepare_history(selection_token) -> PreparedPackageSummary; cancel_prepare(transaction_id); Rust-only VerifiedPackage binds selection, exact signature-verified bytes/path, SHA256, size and package identity. It cannot be constructed from frontend input. No install API accepts arbitrary bytes or paths.

- [ ] Add real key/signature fixtures and failing tests for modified bytes/signature, wrong key/platform/version/asset, replaced metadata, redirect policy, oversize/truncated stream and cancellation races
- [ ] Download to private create-new transaction storage with bounded HTTP/time/size and atomic ready transition
- [ ] Verify publisher Minisign signature plus official digest/size and preserve exact verified pairing; revalidate before handing off
- [ ] Test cancellation and stale selections; independent review; atomic skipCI publication

## Task3: Consistent snapshots, leases and recovery journal

Files: create src-tauri/src/version_history/{maintenance.rs,snapshot.rs,journal.rs,compatibility.rs}; integrate pre-bootstrap persistent markers, runtime launch-admission freeze and result-bearing process quiescence checks; snapshot only after exact source App/WebView exit; tests src-tauri/src/tests/version_history_transaction.rs.

Interfaces: MaintenanceLease; SnapshotManifest (allowlisted roots/files, absence, hashes, relevant permissions and context identity); SwitchJournal (transaction ID, source/target identity, context IDs, phase, observed outcome). begin_switch(prepared_token, reviewed_mode) creates durable intent; recovery reads by token only.

- [ ] Red tests for concurrent writes, multiple instances, retained lock identity, active/starting/unknown sessions, symlinks/reparse points, disk full, partial copy and every journal-boundary crash
- [ ] Add backend launch-admission freeze and persistent startup barrier with non-destructive review/download cancellation; require positively reaped owned PTY roots, exact source App/WebView exit and no relevant Desk/UDF users before snapshot cutover
- [ ] Snapshot complete Desk and actual WebView contexts using exact bytes/existence, including unknown files, providers and disabled skills/agents; block reparse points or configured CLI/project root overlap; keep sensitive contents out of diagnostic DTOs
- [ ] Implement fresh contexts and exact return, with no semver-based compatibility guess and no merge of incompatible contexts
- [ ] Test idempotent resume/recovery without repeating uncertain destructive steps; independent review; atomic skipCI publication

## Task4: Durable Windows version manager and installer execution

Files: create src-tauri/src/version_history/{manager.rs,windows.rs,recovery.rs}; modify main.rs/lib.rs narrowly for maintenance-only startup before ordinary runtime initialization; add CI runner harness under scripts/version-history/ and backend tests.

Interfaces: start_manager(prepared_token) -> ManagerLaunchReceipt; commit_switch(transaction_id); inspect_switch(transaction_id) -> SwitchStatus; restore_previous(transaction_id) -> RecoveryStatus. Manager command line carries only validated transaction identity, no general shell/file operation.

- [ ] Red tests for current bundle copy/hash mismatch, missing ConPTY companions, instance races, installer launch denial/cancel/nonzero exit, uncertain outcome, interrupted restore and new-data conflicts
- [ ] Admit only the verified same-user registered x64 NSIS scope; reject elevated/machine-wide/ambiguous/relocated installations. Preserve the complete current installed bundle, exact product registration/shortcuts and source context outside installation replacement; never assume a published0.18 installer exists
- [ ] Provide restricted independently accessible manager UI and isolated WebView data, no ordinary Native/Legacy bootstrap or session creation
- [ ] Invoke the selected verified installer via controlled Windows process APIs, observe actual launch/exit, verify installed identity; do not rely on updater2.10.1's unconditional exit handoff
- [ ] Keep installed-unconfirmed distinct from success; offer explicit verified restoration only after quiescence, retaining post-switch context separately
- [ ] Independent review of process/path/IPC trust and recovery guarantees; atomic skipCI publication

## Task5: Updates history and recovery UI

Files: src/components/settings/sections/UpdateSection.vue; create src/components/settings/{HistoricalVersionsPanel.vue,VersionSwitchDialog.vue,VersionRecoveryPanel.vue}, src/stores/versionHistory.ts, src/api/versionHistory.ts; update localized strings and scoped user documentation.

- [ ] Failing real-component/composition tests for browse/filter/select, unavailable releases, explicit fresh-settings consequences, backup destination summary, review/cancel/progress and recovery
- [ ] Never display install enabled solely from frontend counts; backend eligibility is authoritative
- [ ] Protect version/transaction ownership across navigation, repeated clicks, reconnect, outdated catalog and late asynchronous completion
- [ ] Keep ordinary update/publication controls unchanged; disclose native CLI roots remain shared and older application features may modify them
- [ ] Preserve keyboard/focus/accessible state; run session creation/close/resume/menu/icon regressions plus full frontend/typecheck/build
- [ ] Independent scoped review; atomic skipCI publication

## Task6: Actual Windows and rendered acceptance

- [ ] Add a restricted isolated Windows CI harness using verified official historical packages and fresh temporary install/data contexts, never the runner's ordinary user state
- [ ] Verify install older version, identity, manager survival, snapshot retention, return to current complete bundle and safe cancellation/failure recovery
- [ ] Assert release/tag/updater publication remains disabled and no private data leaves backups
- [ ] Capture actual new UI states through existing isolated visual workflow; inspect every changed PNG before baseline acceptance, then no-update rerun
- [ ] Run full frontend and Windows Rust tests/lint/format, prior session regression gates, packaging and installation checks on exact final SHA

## Task7: Final test package and evidence

- [ ] Independent whole-feature review after focused gates; fix confirmed regressions without broad unrelated refactoring
- [ ] Verify exact remote commit, workflow source/merge identities and installer hashes
- [ ] Deliver one native Library test package through parent with concise Chinese usage/recovery instructions and unsupported-platform/external-validation limits
- [ ] Update PR34 with only scoped feature and verification evidence; keep Draft/unmerged

Execution authorized on 2026-10-02 using the existing cloud implementation/review workflow. Exact helper mechanics remain subject to scoped correctness and Windows acceptance.

## Binding recovery review

Tasks3–4 must carry the scoped recovery mechanisms into their implementation briefs: distinct manager basename, exact-image exclusive handle/rename before context rotation, persistent pre-bootstrap marker surviving crashes, verified direct-owned-child/App/WebView quiescence, complete typed registration/shortcut restoration, installer-to-payload manifest binding, and durable installer job/process ownership. These are required acceptance properties, not optional refinements.
