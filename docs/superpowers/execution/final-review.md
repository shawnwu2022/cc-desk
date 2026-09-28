# Native CLI v3 final self-review and adversarial review

## Scope

This review covers the complete stacked Native CLI v3 implementation through D27 plus the finalization branch.

Review dimensions:

- product entry and main user journeys;
- profile/workspace storage and mixed-version rollback;
- authenticated document/IPC admission;
- project registration and scoped resource projection;
- launch idempotency, recovery and explicit restart;
- ordered/staged input and no-replay semantics;
- output offsets, ACK ownership, backpressure and fairness;
- run supervision, natural exit, drain and cleanup;
- observer capability isolation;
- D20 real-CLI evidence integrity and anti-forgery;
- release/publishing fail-closed policy;
- Windows candidate packaging, installation and bundled ConPTY behavior.

The review is self-review plus adversarial source/test analysis. It is not independent third-party review and does not convert D20 Layer-C status to PASS.

## Findings corrected before the final gate

### P0 — D20 could accept evidence that was self-consistent but not the orchestrator-owned fixture

The evidence validator proved internal consistency, but the matrix runner did not compare a structurally valid PASS record against the exact fixture and project root that it had written for that cell.

An adversarial driver could therefore substitute another nonce-bearing payload, recompute its hash, produce a matching hook envelope, and remain internally valid. That would undermine the certification gate even though it would not compromise the production runtime.

Correction:

- bind every PASS record to the exact orchestrator-owned fixture hash, nonce, original text, host payload and transform;
- bind oracle cwd to the planned per-cell project root using real-path identity;
- reject CLI-kind mismatch against the plan;
- retain the actual selected binary SHA-256 check.

Regression evidence:

- `D28_D20_SelfConsistentSubstitutedFixtureCannotPass_02`;
- `D28_D20_OracleCwdMustMatchPlannedCellProject_03`.

### P1 — native input-frame evidence was not bound to the outer run

The CC Desk lane required a native-frame-shaped record, but the frame's `runId` could differ from the outer evidence run. Sequence and mode counters also accepted zero and were not capped to u64.

Correction:

- require `frame.runId === record.runId`;
- require generation in `1..=u32::MAX`;
- require positive canonical u64 `inputSeq`, `modeEpoch` and terminal-driver `writeSeq`;
- require observer off/on records for each lane to carry the same host identity.

Regression evidence:

- `D20_Evidence_NativeFrameBindsExactRunAndPositiveCounters_03f`;
- `D20_Evidence_ObserverPairsRequireSameHostIdentity_07b`;
- `D20_Evidence_ComparisonValidatesStructureBeforeFingerprinting_07c`.

### P1 — a delayed old-generation status completion could mutate the new terminal generation

The tab store rejected stale launch status, but `NativeCliTerminal.recover()` ignored that rejection and still changed component-local `launched` and `inputEnabled`. A delayed failure could also mark a newly restarted tab failed, and a delayed input rejection could pause the new run.

Correction:

- capture immutable request/run/generation identity before each asynchronous operation;
- ignore success and failure completion when the tab no longer matches that identity;
- mutate local writability only after exact launch-status adoption;
- apply the same boundary to input failure and stop completion.

Regression evidence:

- `D28_Tabs_AsyncCompletionCannotCrossExplicitRestart_15`.

### P1 — Native Workbench “Add Project” never registered the selected directory

The UI reopened the current workspace after directory selection and only searched the existing list. A new directory was therefore never persisted and the visible button was effectively a no-op.

Correction:

- expose profile-bound `registerProject` on the native workspace store;
- invoke the actual `cli_register_project` mutation;
- adopt the returned project ID and refresh read-only enrichment;
- keep registration failures as bounded safe codes without invalidating already usable workspace state.

Regression evidence:

- `D28_Project_AddButtonUsesRegistrationMutation_01`;
- `D28_Project_SelectedDirectoryIsPersistedAndAdopted_02`;
- `D28_Project_RegistrationFailureExposesOnlySafeCode_03`.

## Areas reviewed with no unresolved code blocker found

- workspace revision/CAS and atomic replacement;
- explicit inherit/set/unset semantics and Codex/legacy-Claude separation;
- authenticated document bridge and output-channel admission;
- backend-held scoped filesystem capabilities and bounded projection DTOs;
- input staging ownership, partial/unknown write freeze and no automatic replay;
- output owner/run/generation/stream checks, parsed-boundary ACK and budget release;
- supervisor attach-failure, shutdown-before-adopt, route-loss and root-exit drain paths;
- observer token/run binding, bounded body allocation and owner-only metadata delivery;
- DOM interpolation-only boundary and public error-code redaction;
- candidate-only release workflow and disabled publication policy.

## Final verification gates

The final branch must pass, on the exact final head:

1. ordinary CI:
   - TypeScript typecheck;
   - complete frontend/adversarial test suite;
   - Node release-policy tests;
   - production Vite build;
   - complete locked Rust tests;
   - rustfmt;
   - strict Clippy;
   - actual Windows application loader probe.
2. Windows test-package workflow:
   - locked dependency install;
   - npm advisory report capture without mutation;
   - frontend/adversarial tests;
   - bundled ConPTY guards;
   - strict Clippy;
   - ordinary NSIS build with updater artifacts disabled;
   - install, reinstall and relocation probes;
   - missing/corrupt runtime fail-closed probes;
   - test-only artifact manifest and SHA-256.

The exact workflow results and package artifact are recorded on PR #33 after execution. No PASS may be claimed from this document alone.

## Remaining external boundary

D20 real Claude Code / Codex CLI Layer-C execution remains BLOCKED until an explicitly authorized target environment supplies:

- real selected Claude Code and Codex CLI binaries;
- authorized test-account authentication;
- real CC Desk and system-terminal lane drivers;
- four-cell hook/host evidence for each product.

Synthetic drivers remain unit-test material only. They cannot promote the product certification status.

## Release boundary

The generated installer is a test-only candidate:

- no version bump;
- no dependency mutation;
- no tag;
- no GitHub Release;
- no updater publication;
- no merge implied.
