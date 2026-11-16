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

## 预算 App 基础收口（2026-11-16，长期输入仍未完成）

- 最终同源码：NetworkBudget 短测 10/10（显式 --skip 已失败的
  NetworkBudget_Real61SecondsInputThenElapsedReceiptRenewal_005），
  remote_unattended 56/56、Delegated 44/44、Service 18/18、network 21/21。
  原 one-shot HostStart 七项按历史各 fresh --exact 进程执行，7/7 通过。
  实际父用例名称去重 142 项，保存在
  .audit/unattended-budget-final-testnames.txt；不重复计算包含关系或子进程。
- 命令沿用 cargo test --offline --locked --manifest-path src-tauri/Cargo.toml
  --lib --features remote-host FILTER -- --test-threads=1；HostStart 加 --exact。
  最终 strict 非 test cargo clippy 同 manifest/features -- -D warnings 通过，
  日志 unattended-budget-app-clippy-final.log。git diff --check 通过。
- 保留原始失败：network-first/real61-second 为长测快照 Closed；IO 诊断
  unattended-budget-idle-diagnostic-first.log 在 1,978 ms 记录 Ok(Io)。
  network-second 的 fixture 总截止分类、毫秒舍入、重复请求序号与尾输入问题
  已精确修正；network-third 为 9/10，61 秒持续 Snapshot+ACK 后 Window
  Deadline，子进程 62.98 秒。该长测失败断言原样保留，不标通过或忽略。
- 新 ControlLease15SecondsStillLimitsWindow 以原测试时钟推进 15,001 ms，
  证明仍有效 grant/快照不等于控制 lease；Window 拒绝且原任务回收、本机
  native 输入正常。长期输入缺少认证 wire 续租，不能靠内部 fixture 直调。
- HostStart 合批日志 unattended-budget-final-HostStart_.log 保留 6/7，
  phone-first install 失败；原进程终身唯一工厂要求隔离，随后七个独立 exact
  日志各 1/1。不据隔离成功宣称合批或全套测试通过。
- 最终短测日志 unattended-budget-network-final-short.log；其他聚焦回归
  使用 unattended-budget-final-*。仅预算安全基础可收口，不是完整长期能力。
  后续先审查显式双端能力协商、受限 lease 续租和独立 outer 心跳，再实现。
  生产授权/UI、真实手机、长空闲、30 天 endurance 及原 PTY 发布门槛仍未验收。

## 受限续租第一片：认证能力事实（2026-11-16）

仅新增精确 control-lease-renew-v1 AppBind 模式；内部数值模式改为封闭
BindMode，旧 Base/Grant/Peer 保持原能力列表。原完整 offers 进入 transcript、
exporter 与双向 MAC，裁剪副本仅供 negotiate_peer 复用严格校验。
NegotiatedControlLease 字段私有，仅本 bind 模块验证成功后生成并沿原
ManagedChannel/driver/BoundDataAuthority 传递，不是授权或公开 DTO 开关。
即使协商成功仍需要 Controller、原活跃 channel 和 Installed fence 才能续租。

RED 为新 negotiate_control_lease 缺失的编译错误，见
unattended-lease-capability-red.log；record appbind 全部 7/7，security
bind 全部 18/18（包含新增真实 mutual-RPK/AppBind 三项），strict 非 test
Clippy 通过。日志 capability-codec-green、capability-native-third、
capability-bind-final、capability-clippy-first 均带 unattended-lease- 前缀。
首次测试编译漏改 browser fixture 的旧数值模式及客户端错误直接 install
NativeGrant 的 Role 拒绝均保留；后者改为真实 grant_client/grant_host 交换。

本片只暴露显式 native 协商工厂，不自动启用原 one-shot，不含手动 lease wire、
调度器、outer 心跳或浏览器/WASM/手机支持；后续每项仍独立验证，生产授权关闭。

## 手动 lease wire 与 native App 验证（2026-11-16）

原认证能力提交 2c82e29d 后，新增严格 LeaseState/LeaseRenew wire，原 actor
派生状态/幂等续租、AuthorizedPeer/Installed/Controller 检查和原 retained custody。
NativeClient 明确请求类型并关联 request/lease/fence/renewalSequence，回复截止
要求 grant.created < deadline <= grant.expires。只有 sealed delegated App wire
选择新工厂，测试客户端显式同模式，旧 one-shot 原行为保留。

最终同源码按父测试名称分库去重 223 项：App 144、record 13、Host 30、
security 18、client 18。实际集合保存在 .audit/unattended-lease-final-testnames.txt。
App 包含 ControlLease 2 项、原 Delegated 44、预算短测 10、unattended 56、
service 18、network 21 及七项各 fresh --exact one-shot；重叠只计一次。
Host 包含 wire 9、control 状态 5、custody 12、真实 pipe OwnerControl 4。
原 control_io_tests 文件名过滤得到 0 项日志保留且不计，随后用实际 OwnerControl_
名称执行 4/4。App/Host/Client 最终 strict 非 test Clippy 和格式/diff 检查通过。

新独立 ControlLease_Real61SecondsManualRenewalThenOriginalRetirement_015
已实际通过：fixture 显式每 5 秒调用手动续租，Snapshot+ACK 每 500 ms 保持
原 fixture IO 活动；等待至少 61 秒后真实远端 na 输入、完整原 receipt 回收、
第二代 ti 与本机 ve 组成精确 native 输出。两项 App lease 测试总计 63.93 秒。
这是 native 客户端/真实 localhost Relay/原 Host 与真实输入路径的活动长测，
不是无流量空闲、生产 scheduler、WASM 浏览器、真实手机或 30 天 endurance。

原 NetworkBudget 无续租的 61 秒失败 case 和断言原样保留，最终批次显式 skip；
不能据 223 项称全 suite 通过。新的手动续租正例独立证明先前 15 秒控制 lease
缺口已有受限协议路径，不意味着自动保活已交付。此前 PTY 全库发布门槛仍保留。

保留所有首次失败：wire 未实现时 decode RED；fixture signer 临时值编译失败；
Observer/codec API 拼写与原 1 秒 dispatcher budget 的 Bounds 拒绝；App 原
NativeClient expected() 不识别新消息的 Codec 拒绝。均未放宽原期限/角色或
绕过真实客户端。最终日志前缀 unattended-lease-app-final2、record-final、
host-final、host-ownercontrol-final、security-bind-final2、client-final；三个
clippy-final 日志（App 使用 final2）均保留。无真实凭据、云服或安全设置改动。

后续只在下一审定阶段实现自动 owner 调度与独立 outer 心跳；固定 lease/fence、
单 outstanding、同序号同截止、丢回复不重置状态、关闭后完整回收等边界不变。


### Browser 手动模式第一片：配置与真实 WASM 认证

- 已实现可选 manual-v1 的严格字符串模式、Controller + application 限制、
  原 AppBind factory 接线和 PairingProfile 显式字段贯通；旧模式缺省不变。
- 新源码 wasm32-unknown-unknown 构建、官方 wasm-bindgen 0.2.129 后处理
  和 Native/WASM 严格 Clippy 均通过。当前生产 wrapper 与 TEST_ONLY
  wrapper 分开保存在本机审计目录，未替换发布资源。
- 实际去重父测试 98 项：Browser/配置 71，Native security 27。
  Node 配置入口 20/20；Browser data 40/40、peer 33/33 的工具计数含子例，
  不能再次累加。去重名单 browser-lease-mode-final-testnames.txt。
- 匹配模式、双向不匹配、显式 Pairing→原 WASM owner、hidden/pagehide/freeze
  和 peer-only 拒绝已实跑。原 bind 18、config 3、data 3、owner 3 全通过。
- 保留 RED：旧配置拒绝新模式；serde enum 接受对象形式被严格字符串测试
  抓住；首次 WASM 编译揭示 Native tokio Instant 与 Browser Instant 不匹配，
  通过平台边界隔离修复而非伪造转换。
- 保留首次 data 39/40：误用生产 wrapper 缺少旧 uncertain 注入测试要求的
  TEST_ONLY_uncertain_start；改用原测试 wrapper 后同源码 data 40/40。
  原断言未改，生产 wrapper 上新认证与生命周期用例也已经通过。
- 日志前缀 browser-lease：wasm-data-fixture-final、wasm-peer-final、
  node-config-final、security-final-*、native-clippy、wasm-clippy。
  这不是全 App/Host suite 或真实手机验收，原 PTY 发布门槛仍保留。

下一片才在 TerminalClient 原唯一 request/epoch/Busy owner 内实现手动
LeaseState/LeaseRenew，不能另起 Rust counter，也不能清 input pending/
NeedsSettlement。自动续租及 outer heartbeat 仍未实现。


### Browser 手动 owner 第二片：统一计数与实际续租

- 前片 757abe8d 后，仅 wire.ts 与 client.ts 增加生产逻辑；手动方法走原
  NativeConnection.query → exchange → NativeClient，没有新 transport API。
- 初始四项 RED 保留在 browser-lease-owner-red.log；codec、共享 request、
  Busy/旧 epoch、错误身份/截止及 timeout 保留 pending 的定向行为已绿。
- 最终 Node owner/native/transport/Worker 33/33；真实 Node-WASM Browser data
  集成 45/45（含子例），独立父例为 37。两组去重合计 70 个父例，名单为
  browser-lease-owner-final-testnames.txt，不与前片 98 项重复相加。
- 真实手动 case 已证 state→renew→snapshot/window→一次写入→未结算时 renew
  →原 receipt query/settle；另旧 mode、到期和 freeze 拒绝且零写入。
  定向两父 case/五计数首跑约 3.04 秒，最终 data 集合约 19.40 秒。
- 受影响 TS 文件 typecheck、Host 严格 Clippy、Rustfmt/diffcheck 通过。
  Rust 生产 WASM 未变，复用前片同源码、已实际构建的测试 artifact；仅 Host
  TEST_ONLY 增加 manual-expired 700ms 策略以检验原截止，不扩大原期限。
- 日志 browser-lease-owner-node-final、wasm-final、typecheck、host-clippy；
  未运行全 App/Host suite，仍有原 PTY 发布门槛，不推送/发布 RC。

后续只读设计原 owner 自动受托管续租与 outer 心跳，不能把手动方法通过
直接等同自动、空闲或 30 天可持续验收，也不能另起计数/无主 timer。


### 自动续租 A 独立片（B/C 心跳尚未实现）

- client.ts 新显式 startAutoRenewal/stopAutoRenewal 与状态观察，默认关闭；
  沿原 counter/Busy/epoch，固定 5s 节拍、3s 严重迟到关闭，不新增配置持久化。
- 13 个确定性自动测试覆盖首调用同步 stop、原 Promise 晚到、close 重入、
  原 waitClosed 方法绑定/拒绝、Due 先于 timer、完整 Snapshot/ACK、暂停回退、
  NeedsSettlement、首次失败与首 reply 暂停后抢 timer，均通过。
- 最终 Node 集合 46/46；真实 Browser data 46/46（含子例），独立父例 38。
  两组去重合计 84 父例，browser-auto-owner-final-testnames.txt；不与历史
  70/98/223 集合重复累计。TS typecheck、Host Clippy、格式检查通过。
- 真实自动 case 首跑约 5.854s，最终约 5.747s；新 TEST_ONLY manual-auto
  明确选 12s 合成 grant，不改变生产预算。首 renew 后实际五秒第二次续租，
  原一次输入、原 settlement、stop→原 waitClosed 后 Stopped 均验证。
- 保留首 RED/API 缺失、局部 operation 重名语法失败、settle 语法校验移位
  导致原 Unknown case 的状态回归。修复为占 counter/enter 之前纯语法校验，
  保留原 NeedsSettlement 拒绝语义，没有修改旧断言。
- 日志 browser-auto-owner-node-final、wasm-final、typecheck-final、clippy-final。
  WASM Rust 生产源码未变，复用已构建且已记录 hash 的原测试 artifact；新建
  Host harness 来覆盖新增合成策略。无真实凭据、云服、安全设置或发布改动。

下一步 B/C 仍须独立设计/审核：显式双端认证心跳能力、原 MUX writer/timer
和固定 cutoff/任务托管。当前不能称无流量空闲或生产无人值守已交付。

## B1 交付记录

- live-v1 严格配置新增 RED（原解析拒绝）后 GREEN，旧模式断言不改。
- 最终同源码 Native bind 19/19、配置 4/4；Node 配置 15/15；
  新构建 WASM 经 Node Worker 原 TLS/Host BrowserData+BrowserPeer 84/84
  （含子测试），57 个父 case。实际去重共 95 个父测试，名单保存
  .audit/browser-live-final-testnames.txt，不重复累计 child wrapper。
- 双方向删/换/重排认证能力、manual/live 不匹配、Controller 与 Observer
  grant 拒绝、application/peer-only 边界以及原生命周期回归已覆盖。
- security Native、client WASM、Host 严格 Clippy 与受影响 TS typecheck
  通过。首非 test 编译 DataRole 路径失败保留，改显式类型路径后最终重建
  WASM/Host 并重新跑全上述集合，不把旧产物算本轮结果。
- 日志 browser-live-*-final*；真实平台是 Node Worker WASM/TLS，非手机、
  Chromium 或 Relay 纯空闲验收。B2/C 未实现、没有激活 heartbeat/A。
  既有全 Host PTY 发布门槛仍未解除。

## B2 交付验证记录

- Rust transport 全 36/36（新机制 8 项），Node 新/旧 Relay/registration
  30/30（新机制 9 项），Node 实际 WSS→原 Rust oracle 1/1。按日志
  父测试名去重共 67 项，名单 .audit/data-keepalive-final-testnames.txt；
  不累计 oracle 子进程内部重复 pass。
- 严格 transport Clippy（test-only-loopback）及 Browser Relay/Source/
  Transport 生产类型检查通过。新增模块已格式化，旧大文件保持最小差异。
- 保留 RED：满应用 queue 的空帧错误 Backpressure；Dormant 已阻塞
  send/flush 激活短 end 仍超时；Browser timer 迟到时原写越短 end；
  heartbeat 未 drain 时合法 TLS write 被拒。分别修正并定向复验，
  未通过放宽原截止、扩大队列或重发帧掩盖。
- 终局覆盖固定 end 持续入站、背压原 writer join、取消 observer 不丢
  原句柄、物理 close、回退/坏期限、独占/重复/错实例及忙 tick 不补发。
- 日志 data-keepalive-transport-final、browser-final、original-wss-final、
  clippy-final2、typecheck-final2。未接 C/生产授权/UI/真实凭据/云服。
  实际 Relay 空闲长测、Worker→Window challenge、手机后台与原全 Host
  PTY 发布门槛仍未完成，不能称长期无人值守已交付。


## C1 阶段验证记录

- 真实 Relay LiveHost 6/6（14.93s）：无业务流量超过 IO/总截止 join、activation 前后 revoke、前后 shutdown、晚 Installed 被断开拒绝。
- 同源码 Delegated 44/44、remote_unattended 56/56；原 HostStart 7 项逐项 fresh --exact 全通过，未合批绕过终身 Host 工厂约束。
- security Native 全库 136 passed / 1 ignored；App remote-host 与 security experimental 严格 Clippy 均通过。
- 日志实际父测试名去重 239 项，名单 .audit/live-activation-final-testnames.txt。首轮 fact API 缺失 RED、Relay 5/6 及 D15 错误观察点记录均保留；修复后日志 live-activation-relay-second 与各 *-final。
- 本片未修改 Browser C2、自动 lease owner、生产 supplier/UI、真实凭据或云服。尚未证明真实 Relay + WASM 无业务流量或 A 低流量 61 秒，亦未解除原全 Host PTY 发布门槛，不能称长期无人值守已完成。


## C2 阶段验证记录

- 受影响 Node 48/48；新真实 WASM 应用 50/50（40 父 case）；真实 Relay/WASM 1/1（10.95s）；C1 六项 6/6。日志父测试名去重共 95，.audit/browser-live-c2-final-testnames.txt，不重复计算子 case。
- 新 WASM 实际构建，WASM client 与 App remote-host 严格 Clippy、受影响 TS typecheck 通过。日志 browser-live-c2-node-final2、wasm-data-final3、relay-final2、LiveHost_-final、wasm-clippy-final、app-clippy-final、typecheck-final2。
- 负测包含回退/迟到、默认无 activation、取消保留原 fact Promise、原 Rust app 终局解除 pending、ack 前空 pending 窗口不占 seq，以及 poll/event/cancel export 调用异常注入后等物理退休且保 Unknown。异常注入以替换 export 的 JS throw 模拟调用失败，不是实际 WASM 引擎 trap 证明。
- 保留首次 Node 缺失机制 RED（失败后因未构造方法留下测试端口而停止进程）、真实链首轮错误 CWD、第二轮 Native-only fixture browser_origins=[] 拒绝 Browser 的记录；仅本地新 fixture 显式允许 https://desk.invalid，旧默认/部署不变。WASM Clippy 首轮遗漏既有 clang 路径，补原官方工具路径后通过。
- 旧裸 TLS Live 用例现在明确验证缺少原 outer activation 时不发布 descriptor；不塞 no-op activation 冒充保活。真实 Live 正例由上述新 Relay 链提供。A 低流量 61 秒留独立后续片；生产 supplier/UI/真实手机/公网与原全 Host PTY 发布门槛仍未完成。
