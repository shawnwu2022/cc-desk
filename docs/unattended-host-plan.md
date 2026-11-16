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

Stage 3a captured-native-run implementation is separate from one-shot admission.
The new capability consumes genuine native admission, exact original resource
identity and test-only sealed consent. It reuses Stage 1 state instead of adding
a second permission state machine. The original producer input/resize path is
used, with a lock spanning the synchronous effect interval and a nonblocking
atomic close request. No production consent, JS command or network route exists.

Actual isolated Node/PTY regression first passed 4 of 5 cases. Stop regression
exposed a real window: supervisor shutdown was set synchronously but per-run stop
was set by a later worker. The delegated check now rejects the original
supervisor's shutting_down flag immediately; the test was not weakened to wait.
Review additionally closed clock-error recovery, an escaping generic fence,
and revocation/expiry during original-resource validation. Error/partial-result
classification is tested separately and is not claimed as physical PTY
partial-write fault injection.

The expanded six actual Native tests cover captured effects after document
retirement with old caller input rejected, exact six-byte child receipt and
resize, stale capture, wrong supervisor and wrong resource, View/device mismatch,
expiry, rollback and clock-error recovery, actor-boundary revoke/expiry races,
and stop/reap rejection. Rejected control operations must leave the child's
actual dimensions unchanged. Existing Stage 1/store tests are rerun separately.

Final Stage 3a verification on WSK-MSI (2026-11-16), same final source:
- `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host DelegatedNative_ -- --test-threads=1`: 6 passed, 0 failed, 2,296 filtered.
- `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host remote_unattended -- --test-threads=1`: 43 passed, 0 failed, 2,259 filtered (previous 42 plus uncertainty classification).
- `cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host -- -D warnings`: passed.
- Focused rustfmt and git diff whitespace checks passed.

Logs: .audit/unattended-stage3a-fourth.log,
.audit/unattended-stage3a-baseline-final.log and
.audit/unattended-stage3a-clippy-final.log. The first failed stop-window
regression remains in unattended-stage3a-first.log. Independent review requested
the clock/fence/validation ordering fixes before this final verification.
This is 49 focused tests, not the complete App suite or phone/network acceptance.
No task-join, cross-restart consent or real user consent is inferred.

## Stage 3b-1 progress: local retirement custody

The Host-local fact API is implemented separately from service adoption/rebind.
Its exact Host/channel/attempt token closes only original channel admission and
retains original bind replies, dispatch results and coordinator JoinHandles.
Each attempt has a separately sealable custody registry; full shutdown traverses
all of them. One Host-wide 64-cell budget also counts ready results held by old
Wait references. This is stricter than the old registry-only limit, not a claim
that the earlier implementation already bounded all retained results.

Four real-channel fixture tests cover close-before-bind, bind-before-close,
late reserve after authority check, observer cancellation, original join success
and panic, and lost original bind reply. The existing genuine wire input test
also retires while its original writer result is pending, without replay.
These use original actors and actual cryptographic channels with an in-memory
writer; they are not physical PTY or real-phone acceptance. Two shared-budget
cases verify independent sealing, old result retrieval, 64 unconsumed ready
results remaining Full, and exactly one freed result restoring one credit.

Initial attempts exposed a missing locked registry cache, then fixture compilation
and premature GrantGate-drop errors. Official registry downloads supplied only
locked versions; the fixture retained its original gate instead of weakening the
timeout or authority checks. The first new tests and the 23-case Host-focused run
then passed. A strict Clippy needless_bool finding in pruning was simplified.
No lockfile, actual credential, cloud deployment or security configuration changed.

An earlier full Host library run had 206 passed, 1 failed and 1 ignored:
AuthorizedPty_BoundIo_001 failed at initial PTY view sync with Err(Sync). A single
exact-case comparison on clean baseline 08081ade passed after preparing the
existing pinned ConPTY runtime; one exact current-case rerun also passed. This
does not establish a baseline fault or erase the failed full-suite run. Full Host
suite acceptance remains an unresolved publication gate; no timeout or assertion
was relaxed and no repeated full-suite loop was used to obtain a green claim.

Local Retired cannot issue complete retirement proof. Original network socket,
App PeerPump and material tasks remain separate required evidence. Unknown keeps
full shutdown custody. Prepared-channel branding, same-reservation delegated
adoption/rebind and production consent remain unimplemented; unattended access
is still unavailable.

Final same-source focused verification on WSK-MSI (2026-11-16), repository root,
existing project Cargo home and build cache, offline/locked:
- `cargo test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-host --lib --features experimental-host-owner local_writer::owner:: -- --test-threads=1`: 44 passed, 0 failed, 167 filtered.
- `cargo clippy --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-host --lib --features experimental-host-owner -- -D warnings`: passed.
- `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host DelegatedNative_ -- --test-threads=1`: 6 passed, 0 failed, 2,296 filtered.
- `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host remote_unattended -- --test-threads=1`: 43 passed, 0 failed, 2,259 filtered.

Fresh final logs: .audit/unattended-stage3b1-owner-verified.log,
unattended-stage3b1-host-clippy-verified.log,
unattended-stage3b1-app-native-final.log and
unattended-stage3b1-app-baseline-final.log. Earlier attempts and the failed full
Host suite remain separately preserved. The owner-strict 44-pass run preceded
the final Clippy expression simplification; owner-verified reran that final source.

- `cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host -- -D warnings`: passed on the same final source; log .audit/unattended-stage3b1-app-clippy-final.log.
- Focused rustfmt check with the remote workspace's Rust 2024 edition and git diff whitespace check passed. An initial check selected Rust 2021 incorrectly and was corrected without rewriting unrelated files.

Independent read-only review accepted the local retirement boundary and the
stronger last-Arc budget. No full Host/App suite, phone acceptance or production
authority activation is claimed by these focused results.

## Stage 3b-2a progress: sealed first-channel adoption

Implemented a separate network-attempt capability, stable session clock domain,
sealed prepared-channel brand and mutual exclusion in the original service
reservation/HostCheckout. Original producer adoption is consuming and occurs only
once. Normal disconnect closes Starting as well as active admission but leaves
the live slot occupied. No complete RetirementProof, second-generation rebind,
material supplier, production consent, UI or retry scheduler is implemented here.

Review found and closed a preparation race: disconnect before adoption previously
saw no active entry and did nothing. The fixed clock domain now closes atomically
before any active-entry lookup. Another review narrowed native create/adopt to the
sealed prepared type rather than accepting an arbitrary RunningChannel.

Actual tests exposed two distinct integration issues. Concurrent clock reads
could return Owner Busy, and full resource checks could encounter the canonical
checkout mutex. The network-only state segment now serializes its short checks;
its private Attempt cannot be used as a local effect Ticket, and the single-live
invariant prevents a valid blocking local effect from coexisting. Resource checks
remain outside that lock. Independent lifecycle review established the original
input's irreversible seal and exact captured identities as the mutex-free clock
lifetime fence. Full native handoff/actor slot and scope checks were retained.
Tests cover concurrent sampling, close while sampling waits, local-ticket exclusion,
held resource/canonical mutexes and actual input seal/shutdown/natural reap.

The next factory rejection was separately diagnosed: every precondition was true
and authority.check succeeded, but the original factory returned Rejected. Its
registry intentionally rejects a second Host for the entire process, including
when its old Weak has expired. The fixture had attempted two fresh Hosts in a loop.
Normal disconnect and revoke are now separately named cases, each executing the
same test executable with --exact in a bounded child process. This follows the
project's existing native subprocess pattern. The parent requires success and one
actual passed case; timeout fails rather than claiming retirement. No factory,
identity/incarnation, timeout assertion or production registry was relaxed/reset.

Final same-source verification on WSK-MSI (2026-11-16): 91 distinct focused cases
passed. Child isolation wrappers and their worker runs are not double-counted.
All commands use `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml
--lib --features remote-host <filter> -- --test-threads=1` from the repository root:
- DelegatedChannel_: 7 passed, 2,307 filtered; actual Node/PTY + TLS/AppBind channel
  cases, including original remote input, disconnect/revoke without replay,
  preserved local input, wrong binding/scope, late bind and Starting cancellation.
- DelegatedNative_: 6 passed, 2,308 filtered (existing Stage 3a actual effects).
- remote_unattended: 48 passed, 2,266 filtered (existing 43 + new network-clock 5).
- remote_host::tests: 14 passed, 2,300 filtered (original custody/activation/exit).
- HostCandidate_: 5 passed; HostPairingScope_: 2 passed.
- NativeRemote_ActualAdmissionInstallsOriginalOwnNode_001: 1 passed; original
  one-shot real producer adoption and same-Host rebind were not replaced by mocks.
- publication_callback_close_cannot_commit_success: 1 passed.
- HostStart_PhonePolledFirstOriginalNode_001, HostStart_HostInitiatedFirstOriginalNode_002,
  HostStart_CancelAfterRegistration_003, HostStart_CloseAfterRegistration_004,
  HostStart_NetworkClockRejectsOnce_005, HostStart_BeginClockPanicKeepsUnknown_006,
  HostStart_AsyncNoPeerFailureCannotRetry_007: each passed in a fresh test process.

`cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml --lib
--features remote-host -- -D warnings` passed on the same source. Focused Rust 2021
format and git whitespace checks passed. Existing project Cargo home/target were
reused; no dependency or lockfile change was needed.

Final logs: .audit/unattended-stage3b2a-final-*.log,
unattended-stage3b2a-relay-final-*.log and unattended-stage3b2a-clippy-first.log.
Earlier compile, clock/resource, factory and fixture-setup failures are separately
preserved. The first HostCandidate run passed four parser tests but its real-start
case lacked Relay fixture environment variables; the corrected five-case run is
recorded separately, not a rewritten log. Relay regressions used only the existing
local executable, verified against the earlier compatibility record with SHA256
39E54732DE9EF9D5B7FAAE179148EF2852B27F490956D880C144C106323F8CB3, and repository
public test certificates. CC_DESK_TEST_RELAY_ROOT/EXE were set only for those
commands. No real credential or cloud service was touched.

The earlier Stage 3b-1 full Host PTY-suite failure remains an unresolved publication
gate. These 91 passing tests are not a full App/Host suite or real-phone acceptance.
Stage 3b-2b must still retain and join original preparation/material/socket/PeerPump
and Host facts before any complete receipt or second attempt can be issued.

## Stage 3b-2b-i result: original single-generation socket custody

- [x] Share original setup/teardown without weakening one-shot authorization.
- [x] Add sealed delegated runner and material/release custody; production
      material and consent constructors remain absent.
- [x] Register coordinator before material work; retain it across Session Drop,
      cancelled observation and failed go publication.
- [x] Fence start against shutdown; release all startup guards before last-owner
      Session Drop. Independent review found no remaining P0-P2 blocker.
- [ ] Stage 3b-2b-ii: aggregate exact Host-local token, original PeerPump,
      socket/MUX and material facts into the sole complete receipt constructor.
- [ ] Consume receipt before clearing live, selecting the next clock attempt and
      same-Host rebind. Never reconstruct or consume the original producer again.

Final same-source validation on WSK-MSI (2026-11-16): 119 distinct focused tests
passed, plus strict non-test App Clippy. Commands ran from the repository root:

    cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host <filter> -- --test-threads=1
    cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host -- -D warnings

Filters/counts: DelegatedSocket_ 8; DelegatedChannel_ 7; DelegatedNative_ 6;
remote_unattended 48; remote_host_network::tests 21; remote_host::tests 14;
HostCandidate_ 5; HostPairingScope_ 2;
NativeRemote_ActualAdmissionInstallsOriginalOwnNode_001 1; the seven HostStart_
cases listed in Stage 3b-2a each passed in a separate invocation. The publication
callback case is included in network 21, not counted twice. Isolated child tests
and their wrappers are also counted only once.

The new eight cover real localhost Relay socket/MUX adoption and remote nat
input followed after disconnect by local ive (exact native child report);
cancelled observation with late material; shutdown before start and between
registration/go; material spawn failure and real worker panic; Session Drop with
material in flight; and last external Session Drop while start retains keepalive.
The last case explicitly orders guard release, final keepalive destruction and
go, rather than depending on scheduling speed. These are single-generation tests,
not complete receipt or reconnect acceptance.

Logs: .audit/unattended-stage3b2bi-final-01.log through final-16.log and
unattended-stage3b2bi-clippy-final.log. Earlier compilation/lint attempts and the
six-case first failure remain separately recorded. That failure assumed start
must succeed after shutdown had discarded go. The corrected test retrieves the
same retained custody and checks its true terminal fact and sticky Unknown on
publication failure; send failure never means that no task exists.

The existing Relay executable was rechecked at SHA256
39E54732DE9EF9D5B7FAAE179148EF2852B27F490956D880C144C106323F8CB3.
Existing Cargo home/target and public fixture certificates were reused. No new
dependency, lockfile, real credential, cloud or security-setting change occurred.
Focused formatting and git whitespace checks passed.

The prior full Host PTY-suite failure remains an unresolved publication gate.
These results are not a full App/Host suite, real-phone acceptance, production
unattended activation or complete per-attempt retirement proof.
## Stage 3b-2b-ii completion: original receipt and bounded same-Host renewal

Implemented the opaque Host Installed-readiness fact, Session-only complete
retirement receipt, single-consumer NextStarting transition, same-Owner atomic
retire/begin, fixed-clock generation fencing, and original Host rebind without a
second producer checkout. Production native consent/material/UI/retry entrances
remain unavailable. Only a previously Installed generation can renew; the current
64-history limits and 60-second attempt maximum are unchanged. The separate
30-day sustainable-history design remains open.

Final same-source verification on WSK-MSI, 2026-11-16:

- App filters: Delegated 44; remote_unattended 52; remote_host_network::tests 21;
  remote_host::tests 14; HostCandidate_ 5; HostPairingScope_ 2;
  NativeRemote_ActualAdmissionInstallsOriginalOwnNode_001 1; the seven HostStart_
  cases listed above each passed in a fresh invocation.
- Host ChannelReadiness_ 2 passed.
- Strict non-test App and Host Clippy both passed with -D warnings.
- Actual successful parent test names were extracted from the final logs and
  unioned: App 136 unique, Host 2 unique, total 138. Delegated/unattended overlap
  and isolated child execution are not counted twice. The exact name set is in
  .audit/unattended-stage3b2bii-final2-testnames.txt.

Commands, from the repository root with the existing Cargo home/target and
project-local TEMP/TMP/TMPDIR:

    cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host FILTER -- --test-threads=1
    cargo test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-host --lib --features experimental-host-owner ChannelReadiness_ -- --test-threads=1
    cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features remote-host -- -D warnings
    cargo clippy --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-host --lib --features experimental-host-owner -- -D warnings

Final logs: .audit/unattended-stage3b2bii-final2-01.log through final2-14.log,
unattended-stage3b2bii-host-final2.log, unattended-stage3b2bii-clippy-final2.log,
and unattended-stage3b2bii-host-clippy-final2.log. Final hashes are preserved in
unattended-stage3b2bii-final2-hashes.txt:

- App test executable: 9A713FC7B9438AEA4AA0111F18182945B3CE2AAF6DA685BB36E64A22738A6CB8.
- Host test executable: AB0BCF4671C92FCCDBD7C60BD9FCD2183B360E9E53CF523BC4F7A06B8C43B8B6.
- Existing Relay executable: 39E54732DE9EF9D5B7FAAE179148EF2852B27F490956D880C144C106323F8CB3.

New behavior coverage includes original network and PeerPump joins independently
held across cancelled observation; a late original bind actor that cannot mint
Installed evidence; duplicate observers and receipt consumers; observation during
NextStarting; old-client closure; revocation before/after atomic selection;
absolute expiry; wrong device; sequence overflow closing the Owner gate before
unlock; and rejected rebind becoming sticky Unknown rather than perpetual Pending.
The successful two-generation localhost test requires remote na, the real original
receipt ACK/settlement, new operation/sequence 2 writing ti, local ve, exact child
native output, and one retained producer pin. A separate conflict case proves
changed payload with the old operation remains Unknown and does not write twice.

Earlier failures remain in separate first/second/third/fourth logs. They exposed
fixture mistakes rather than grounds to weaken production semantics: reusing an
operation with changed payload produces the original conflict result; routing an
old-window frame through a new channel did not promise a receipt; and a new
operation before settling the original pending receipt was InputNotAccepted.
The success fixture now follows the existing receipt settlement and continuous
sequence contract. The original journal and producer were never reset. Rebind
failure-state coverage uses a duplex prepared channel only for rejection injection;
it is not substituted for the real successful second-generation socket/MUX joins.

No dependency/lockfile, real credential, cloud, security-setting, production
supplier or UI change was made. The earlier full Host PTY-suite failure remains
an unresolved publication gate. These 138 focused cases are not a full App/Host
suite, real-phone acceptance, 30-day endurance test or production activation.

## 容量第一片验证记录（2026-11-16）

- RED：相同原 Host/producer 的真实 duplex TLS channel 第 65 代触发 Capacity，
  保留 .audit/unattended-history-host-red-first.log。
- GREEN：原 Host/producer 完成 130 代真实安装、原 channel join 和 rebind，
  每代原本地 input 仍可写；稳定序号连续，已删除首 ID 不能别名新条目。
  保留 64 个原退休 token 的独立用例仍在第 65 代 Capacity，旧 token 仍观察
  对应原 Retired，未提高上限。
- 同源码 Host peer_coordinator 26/26、custody 12/12，共 38 个不重叠具名用例；
  包含新 7 项容量/取消/并行观察/跨 Host/溢出/外部结果背压，以及原 late-bind、
  response-cancel、撤销和输入托管回归。日志为
  .audit/unattended-history-host-coordinator-first.log 与
  .audit/unattended-history-host-custody-first.log。
- Host 非 test Clippy -- -D warnings 通过，记录
  .audit/unattended-history-host-clippy-first.log。首轮机械编译遗漏与参数化
  fixture 替换错误保留在 green-first/second、boundaries-second 日志；
  没有放宽生产检查来让测试通过。
- 独立只读审查无 P0–P2 阻塞。App 兼容验证单独记下，不将既有旧结果累计为
  本轮验证。原全 Host PTY-suite 失败发布门槛保持，不能据本片宣称全库绿或发布。
- 下一片仅 App 已真实 joined 的历史容器回收；长期连接窗口预算另片验证，
  不与本片一起扩展生产授权或 UI。

补充同源码 App 兼容结果：DelegatedRenewal_ 9/9，含真实 localhost Relay
两代 socket/MUX、同 Host/producer、原输入 settlement 及撤销/到期/错设备/
耗尽失败路径；日志 .audit/unattended-history-app-renewal-first.log。
本轮共 Host 38 + App 9 = 47 个不重复父用例；未重算 contained 子进程内部用例。

## 容量第二片验证记录（2026-11-16）

- RED：130 个真实 PeerPump 任务依次 join 后，原未回收列表在第 64 项
  （零基 generation63）仍耗尽 admission；日志
  .audit/unattended-history-app-red-first.log。
- GREEN：新 4 项 AppHistory 全通过，含 130 次原任务 join/原 reservation，
  64 个终局但未消费 handle 仍占容量、取消 observer/slot 锁竞争仍托管、
  panic Unknown 保留与关闭不重开。原 network 历史用例扩为 130 次真实
  coordinator join，并持有旧观察 Arc 验证旧结果不指向新条目。
- 最终同源码 Service 18/18、network 21/21、Delegated 44/44。实际父用例名
  去重 83 项，写入 .audit/unattended-history-app-final-testnames.txt；
  记录在 service-final/network-final/delegated-final 日志，不重复累计
  contained 子进程或最初 AppHistory 单独结果。
- App 非 test Clippy -- -D warnings 通过，记录
  .audit/unattended-history-app-clippy-final.log；rustfmt 与 diff check 通过。
  本片只改原 PeerPump 历史 prune，network 仅扩测试，materials 无重复 GC。
- 本轮含原真实 localhost Relay 双代、原生产 actor 输入与 receipt settlement、
  取消/迟到 bind/材料与退出边界回归，但不是完整 App/Host suite、真实手机或
  30 天 endurance。旧 PTY 全库发布门槛仍未解决，无 push/merge/RC 发布。

## 预算 security 首片验证（2026-11-16）

新增 GrantCeiling 5/5；同源码 grant 过滤合计 38/38（含新5项及原签名、
management/browser/期限回归），security 非 test Clippy -- -D warnings 通过。
日志 .audit/unattended-budget-security-feature-second.log、
unattended-budget-security-grant-first.log 与
unattended-budget-security-clippy-first.log。最初漏 experimental 的 0 项结果
及测试 fixture 可见性编译失败保留但不计入成功。独立只读审查无阻塞。
此提交限定 ceiling 能力；App fixed window/AttemptElapsed/wire 接线和真实
超过60秒输入仍在后续切片，不能声称已完成长期连接。
