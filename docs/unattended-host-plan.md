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

Stages 2–5 are not implemented or verified. No user-facing switch is enabled.
