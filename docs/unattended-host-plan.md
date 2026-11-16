# Client-owned unattended access implementation plan

> For agentic workers: execute one reviewed stage at a time. The client feature
> must remain unavailable until its real authorization and revocation chain is
> integrated. A pure state-machine test is not product acceptance.

**Goal:** A manually enabled, bounded and revocable Host delegation for one
explicitly installed phone and an explicitly selected target, with safe reconnect
and validated App-restart recovery.

**Architecture:** Separate native authority owner from one-shot approval. Issue
fresh per-attempt permits through the original network/material custody chain;
persist consent and revocation, never old handles or claims.

**Tech stack:** Existing Rust/Tauri/Vue, Windows native storage/crypto adapters,
original TLS/AppBind/grant protocol and Browser WebAuthn. No new dependency planned.

**Spec:** [unattended-host-design.md](unattended-host-design.md)

## Global constraints

- Default disabled. No real phone identity is known or implicitly enrolled.
- Absolute consent end at most 30 days; reconnect/restart never rolls it forward.
- Host explicit local consent; phone new-channel authentication remains mandatory.
- No cloud, true credential, network-security or OS startup-setting changes.
- Preserve existing one-shot behavior, exact run/owner boundaries and Unknown lock.
- No automatic input replay, new device, broader role or arbitrary future terminal.

## Review focus

1. Disable races with material supply or late network publication: old attempt
   never becomes usable after the authority gate is closed.
2. Durable commit is uncertain: UI cannot claim reboot-safe revocation or enabled
   status, and this process cannot retry into authorization.
3. Phone removed or trust changes while connected: cancel existing authority and
   prevent new attempts, not only hide a pairing row.
4. App restart changes run/document identity: resolve a verified durable target,
   never deserialize old handles or substitute another terminal.
5. Backoff, suspend and clock regression: no lifetime extension or unowned retry.

## Stage 1: single-run authority baseline

Files: `src-tauri/src/remote_unattended.rs`, its focused test file if split,
and the matching `remote-host` module registration in `src-tauri/src/lib.rs`.

- [x] Implement internal-only fixed binding, bounded deadline, sticky revoke,
      attempt fencing and single-owner retirement state. No IPC approval boolean,
      serialization, networking or real identity provisioning.
- [x] Tests: zero/over-30-day/overflow end; exact binding mismatch; wall rollback;
      monotonic cutoff; concurrent begin; stale attempt after revoke; Unknown
      retirement; retry only after exact retirement; no reenable of old owner.
- [x] Run the focused module tests and compile under the App's actual feature gate.
      Standalone rustc evidence, if used, is additional and not App integration.
- [ ] Independent security review, then local commit with the inactive boundary
      documented. Do not announce unattended access as complete.

## Stage 2: protected consent store and exact target policy

Create `src-tauri/src/remote_unattended_store.rs` and focused tests. Use a separate
storage domain from the public pairing suggestion store. Review the existing
`version_history/windows/pairing_store.rs` handle-relative private NTFS locking,
durable staging/replace and uncertain-commit conventions; do not repurpose its
pairing-specific files, leases or authorization type.

- [ ] Persist strict schema/domain, consent revision, exact binding/target,
      absolute end, observed clock floor and revocation marker. Keep secrets and
      LaunchSnapshot/env/argv out of records and UI. Define authoritative native
      consent and durable restore constructors separately from parsed data.
- [ ] Write fault-injection tests before implementation: missing/corrupt/duplicate
      records, changed target, competing writers, replace failure, post-publish
      uncertainty, rollback and restart after revoke. Verify real Windows adapter
      behavior in a test-owned directory, without changing ACLs or true stores.
- [ ] Gate restore before key supply/network; block uncertainty, clock rollback
      or lack of a trustworthy target. No reset-to-enabled recovery.
- [ ] Review and commit only after those exact tests pass.

## Stage 3: native admission and owned reconnect

Modify the actual `remote_host_controller.rs`, `remote_host_network.rs`,
`remote_host_activation.rs` and `remote_host_driver.rs` only at reviewed native
delegation entrypoints. Keep old one-shot APIs strict. Add fresh native profile
suppliers rather than cloning FrozenHostSupplier or ActivationTicket.

- [ ] Add a delegation-specific native admission owner that proves the selected
      live run and derives a fresh short-lived permit. Do not manufacture caller
      or document identity. Recheck revision/trust at key and publication fences.
- [ ] Add one owned retry scheduler with bounded backoff, cancellation and actual
      join. Retry only transient transport loss after proven full retirement.
- [ ] Test against fixture Relay through the real host path: loss, generation
      change, exact fresh proof, revoke-before-publish, revoke-connected, epoch
      change, concurrent start, old completion, Unknown drain and no input replay.
- [ ] Test original one-shot rejection/approval unchanged, then review and commit.

## Stage 4: local UI and device lifecycle

Use existing `src/components/workspace/RemoteHostPanel.vue`, authenticated IPC in
`remote_host_ipc.rs`, registration in commands/lib and the existing typed API.
Keep ordinary pairing suggestions visibly distinct from authority.

- [ ] Add disabled-by-default controls selecting an installed verified phone,
      exact target, supported role and finite end, backed by native confirmation.
- [ ] Add visible status, verification-needed, disconnected/retrying/retiring and
      disabled states. A single Disable closes authority and persists revoke;
      do not label a normal Disconnect as permanent disable.
- [ ] Wire actual device Forget/removal and trust invalidation to revoke first.
      Current public tombstone alone does not revoke an existing session.
- [ ] Test strict DTOs, forged approval, stale replies, double enable, cancellation,
      failed durable revoke, late publication and click-to-owned-network effects.
- [ ] Run affected Vue tests, typecheck, Rust integration and independent review.

## Stage 5: App restart and phone acceptance

- [ ] Native startup starts Locked. Resolve the approved stable target against
      current workspace/profile/device state and recreate fresh authority without
      restoring serialized old run/document handles. Missing or changed target
      stays Locked; do not automatically choose another session.
- [ ] Tests cover successful exact target recovery, absent target, changed profile,
      old consent revision, revoke before restart, expired/suspended state and
      no network before recovery checks complete. Never reset the absolute end.
- [ ] Real Browser fixture verifies preserved enrollment but fresh channel proof,
      expected WebAuthn/presentation behavior and no silent authority widening.
- [ ] Record real phone/Host GUI acceptance as outstanding until actual identities
      are enrolled and those user-device tests run. Do not substitute mocks or
      the earlier 11-case one-shot compatibility result.

## Current progress

Stage 1 private model and design review are complete. The Windows App library
compiled with remote-host and its 13 focused tests passed (2,253 other tests were
filtered, not run). A separate rustc test harness also passed 13/13. First Cargo
attempts encountered a local registry-cache mismatch, resolved by using the
existing project cache from the repository root without changing configuration;
the first actual compile exposed duplicate module registration, which was removed
before the passing run. Strict App remote-host library Clippy (-D warnings),
focused rustfmt and diff whitespace checks passed. Independent read-only security
review found no blocking Stage 1 defect. No UI/network/restart acceptance is implied.

Verified test executable SHA256:
19159973180BFD50FE4415EFEE6649C52DB4816E9FB73CD84591DCC795519FA9.

Stages 3–5 are not implemented or verified. No user-facing switch is enabled.

Stage 2 storage baseline: a distinct strict record store and Windows adapter were
added under remote_unattended_store and windows/unattended_store. Their
records remain Locked descriptions, not executable consent or restored authority.
The adapter uses its own directory/files/leases; the pairing storage path and
pairing publication primitive are unchanged. Tests use a fake sealer with the
real Windows file/lease adapter, never the user's true store or credentials.

The temporary WSK-MSI transport disconnection was followed by a read-only file
audit: core/DPAPI/registrations existed, but adapter/helper did not. Only missing
files were then written. This is not evidence that an uncertain write was safely
replayed.

Review found two persistence hazards before activation: failed read must latch
this Store closed, and an observed expiry/rollback must persist Revoked plus the
wall floor before returning its reason. Whole-store rollback by a local principal
with storage control is outside the adapter's guarantee. Post-publication Unknown
poisons this instance; safe cross-restart recovery of uncertain transactions is
still a separate required gate, not inferred from parsing a later valid blob.

Stage 2 validation: the first actual App compile succeeded but 15 filesystem
tests failed during fixture setup (one DPAPI-disabled test passed). The fixture
used mixed Windows path separators rejected by the unchanged production path
validator. Normalizing only the test-owned manifest path and joining components
separately fixed setup. The final actual App library run passed all 30 focused
tests: 13 Stage 1 and 17 Stage 2 (2,253 unrelated tests filtered, not run).
This includes real NTFS adapter tests with fake sealing, not real DPAPI runtime
acceptance. Storage CAS revision is now separate from authority revision; ordinary
wall observations cannot change the grant epoch. Both known wall and authority
high-water marks reject regression even under a larger storage revision.

Strict non-test App remote-host library Clippy (-D warnings) passed, so the real
DPAPI adapter compiles, but it was not invoked. Focused formatting and whitespace
checks passed. Read-only security review found no blocker for this inactive
storage stage; publication-vs-revoke atomicity and cross-process live cancellation
remain real integration gates.

Before Stage 3/5 activation, implement Stage 2b transaction evidence: a fixed,
initialized, bounded append journal under the same private-directory control lease,
strict Pending/Clean records linked to store/target/storage revision/authority
revision and exact sealed blob digest. Flush Pending before blob publication,
verify/flush blob before Clean. Missing/torn/unmatched/Pending history stays Locked;
no automatic journal repair/reset. A recovered matching Clean can provide commit
evidence, not evidence the old caller received an acknowledgement. Define that
recovery distinction explicitly, retain same-process known-Unknown fencing, and
do not claim tested power-loss durability or defense against full local rollback.

Stage 2b implementation: fixed bounded journal, strict full-chain verification,
Pending/blob/Clean flush ordering and identity-bound same-process Unknown fencing
are implemented. A matching Clean is recovered only after reflush, and remains a
locked description, never authorization. The design above records the exact
linearization and local rollback/power-loss limitations.

Validation history is retained rather than overwritten. Initial compile caught
an extra core publish argument. The first completed 40-test run had 37 passing:
003 reopened an uninitialized journal before initialization (now correctly
rejected by the production contract), and 015/017 expected pre-journal error
categories. The competing-writer fixture now initializes before reopening;
independent empty-journal rejection remains covered. Pending-era failures expect
Unknown. The core high-water rejection is tested independently as well as the
real adapter's earlier journal/blob mismatch rejection. A retry of the old
executable selecting zero tests is not counted as evidence.

The corrected 40-test run passed, then the expanded 42-test run passed (13 Stage 1
plus 29 store/journal cases; 2,253 unrelated tests filtered). New cases cover
same-read double commit and abandoning initialization, alongside partial/full
Pending/Clean failures, post-publication failure, separate child-process recovery,
same-process new backend fencing, malformed/reused transactions, capacity,
identity replacement, CAS, cancellation and the poison/lease race. Fake sealing
and project-owned temporary NTFS directories were used, never production data.

Non-test Clippy caught a test-only import and closure error-type inference issue
hidden by fault injection, followed by strict style findings in the chain parser.
These were fixed without suppressing warnings or weakening validation.

Final Stage 2b verification on WSK-MSI (2026-11-16), from the repository root:
- `cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host -- -D warnings`: passed on the final source (non-test, real DPAPI code compiled but not invoked).
- `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host remote_unattended -- --test-threads=1`: passed again on that same source, 42 passed / 0 failed, 2,253 filtered.
- Focused rustfmt check and git diff whitespace check passed.

Logs are retained under the worktree's .audit directory: final
`unattended-stage2b-clippy-third.log` and `unattended-stage2b-seventh.log`,
with earlier attempts separately preserved. Existing offline cargo home and build
cache were reused; these are fresh executions, not old test artifact claims.
No production activation, whole-App suite, real DPAPI execution or power-cut
acceptance is implied. Stage 3a delegated native admission remains next.
