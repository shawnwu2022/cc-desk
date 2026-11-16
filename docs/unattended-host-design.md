# Client-owned bounded unattended access

Status: design, not an enabled product feature. No real device, key, server or
authorization is provisioned by this document.

## User direction and implementation choices

The user wants the development-machine client to decide unattended access
explicitly and revoke it at any time, with convenient everyday use and automatic
reconnection where this does not weaken authentication. The coordinator selected
WSK-MSI, one explicitly identified paired phone, a maximum 30-day delegation and
candidate site ccdesk.wushangkun.cn. The phone's real identity is NOT known.
These are constraints for an eventual local consent, not an active grant.

First baseline scope: one selected live native run and the selected installed
phone. The user's later direction also requires evaluating recovery after App
restart, so the complete feature must have a separate durable target policy and
native restore owner. Persist only explicitly authorized target/profile identity,
scope, device, role and absolute end. On startup begin Locked, resolve the target
against current trusted native workspace/profile state, then issue fresh runtime
scope and keys only after all checks succeed. Never restore old document/run
handles or broaden a missing target into any available terminal. An unverifiable
target remains Locked with a specific reason, not a claim of completed recovery.
No new OS boot registration or blanket future-process authority is implied.

## Alternatives

1. Extend the current one-shot approval to 30 days: rejected. Its supplier and
   permit are consumed once; controller ownership is exact document/caller/run.
   Reusing it would conflate a connection grant with a persistent delegation.
2. Let the Relay approve reconnects: rejected. Relay availability is not endpoint
   authority and cannot implement local device revocation.
3. Add a native delegation owner issuing fresh bounded per-attempt permits:
   selected implementation direction. Retain the original strict default path.

## Required authority and persistence

Default is disabled. Enabling requires an authenticated local Host UI operation
and native confirmation displaying the installed phone's verified fingerprint,
selected live run, role/capabilities, exact origin/RP/Relay route and absolute end.
Never derive a phone from last-used order or accept a JavaScript approved=true.
Missing installation, identity ambiguity or unsupported trust data stays Locked.

Store a versioned, bounded public delegation record, not keys or old assertions.
It binds the native installation, selected phone and credential/key fingerprints,
enrollment revision, issuer/principal, all relevant epochs, exact origin/RP/route,
selected run scope, allowed role, consent revision and absolute expiry. Separate
the 30-day delegation from the short-lived capability generated for a connection.
Changing any authority-bearing field requires a new local consent.

Durable state must distinguish Disabled, Enabled and Revoked, retain a monotonic
consent/revocation revision and observed wall-clock floor, reject malformed or
partial records, and serialize writers. Persistence succeeds before Enabled is
published. On disable, close the in-memory admission gate immediately, persist
the revocation and cancel existing activity. If persistence fails, remain blocked
in this process, report the failure and do not claim reboot-safe revocation.
Startup must check durable state, current installation/trust/run and clock before
any key supplier or network task is created. No absent-file auto-initialization
may restore Enabled. Define a protected local storage trust boundary explicitly;
local administrator rollback is not solved by a JSON revision or wall clock.

Anchor the absolute end to both wall and monotonic time without extending it on
reconnect. A backwards clock observation, expiry, deleted device, changed trust,
missing state or uncertain record closes admission. Restart never resets the end.
Do not extend the delegation silently when refreshing a per-attempt capability.

## Owner and reconnect state machine

States: Disabled, EnabledIdle, Starting, Connected, Retiring, Backoff, Revoked,
Expired and BlockedUnknown. A single native owner serializes admission, revision,
attempt identity, cancellation and retry decisions. At most one attempt is live.
Each attempt carries the exact consent revision and scope; recheck these before
key supply, before network creation and before publishing the connection.

For each attempt create fresh source material, channel authentication, nonce,
exporter binding and short-lived grant. Never clone consumed ActivationTicket,
FrozenHostApproval, native material supplier, grant, input buffer or assertion.
Reuse the existing service reservation/driver custody and exact run admission;
do not impersonate a stale document caller to make a background path work.

Retry transient transport loss with bounded backoff only after the original
socket, key/material owner and tasks have verifiably retired. Pending or Unknown
retirement blocks retry; a timer expiring is not proof of join. Retry timers are
owned/cancelled/joined too. Authentication, trust, policy or storage rejection
does not retry into success. Never replay pending or uncertain terminal input.

An ordinary user Disconnect stops the current connection. A separate prominent
Disable unattended access both stops it and prevents reconnect; make this
distinction visible rather than surprising the user with an automatic reopen.
Device deletion and trust invalidation call the same disable/cancellation path.
Existing pairing Forget only writes a public tombstone and is NOT sufficient:
wire that operation to the delegation owner before claiming device-level revoke.

## Phone-side convenience without implicit authority

The existing Browser channel requires fresh presentation and WebAuthn UP/UV for
its new binding. Keep this validation. Do not replay the previous assertion or
replace UP/UV with a stored-key possession check. Device capability determines
whether a fresh operation needs a prompt; no cross-device silent UX promise.
Retain enrollment and prepare the next authenticated connection conveniently,
but show verification-needed state instead of repeatedly opening prompts.
Phone background resumption cannot become permission to select a new Host run.

The existing public Host configuration uses a 60-second grant, not a convenient
long-running session. Stage 1 preserves that bound to avoid silently widening an
old capability. Delivering the complete low-interruption feature requires a
separately reviewed per-connection budget/continuation design inside the approved
delegation, with revocation checks throughout the real data path. Reconnecting
every minute with another prompt is not acceptance of the convenience goal.

## Revocation semantics and offline limits

Local disable prevents new attempts and cancels existing authority before any
success UI. Distinguish authorization revoked from resources retiring/unknown.
It cannot undo bytes already accepted by the real CLI or stop unrelated CLI jobs.
The original input-effect and task-custody boundaries still apply.

An offline or suspended Host cannot receive a phone-side revoke immediately.
Do not invent an online revocation service. A local durable revoke made while
network-offline takes effect on the local process and is checked before future
connections. If the application is closed, no in-memory connection is live;
restart must check the durable tombstone first. Exact reachability and resume
behavior require real suspend/network tests.

## Implementation sequence and evidence gates

1. Process-local native policy model: strict binding and disabled default; finite
   deadline, revision, revocation, competing attempt and failure tests. No
   persistence, production consent constructor or networking exposed by this stage.
2. Separate protected consent store and exact durable target policy: atomic
   persistence, concurrent writers, uncertain commits and restart revocation tests.
3. Integrate original live-run admission, fresh material suppliers, cancellation,
   actual task join and one-at-a-time reconnect. Exercise the real call chain
   against fixture Relay, including Relay generation change and clock rollback.
4. Local Host UI and authenticated IPC: installed-device selection, finite scope,
   explicit native consent, status and single-action disable. Wire device deletion
   and trust changes. Verify stale replies, double-enable, disable-during-start,
   document replacement, changed run and failed persistence, not only state getters.
5. Restart/storage tests and actual Browser/native test: fresh phone proof, loss
   and reconnect, no replay, revoke while connected, revoke while starting, old
   revision after restart, expiry while suspended and unknown-retirement closure.

   Durable target owner: distinguish a stable approved target from a transient
   run/document. Build a dedicated native admission path, not a synthetic webview
   caller. Prove restart reconstruction against current workspace/profile/device
   versions, old revision rejection, absent/moved/changed target rejection and
   revocation before network on startup. The single-run baseline alone does not
   fulfill this stage or the user's cross-restart convenience goal.

No stage may present the feature as usable until the UI-to-network path and
revocation chain pass. No preselected real phone, DNS/server change, new key
configuration, startup registration or security-setting change is authorized by
an implementation test. Record exact test artifacts and unresolved platform gates.

## Existing code anchors

- remote_host_configuration::freeze currently constructs a 60-second candidate
  and Controller role, with a one-shot native material supplier.
- remote_host_approval::ApprovalMachine consumes the description once; its
  native lease revokes key/source authority and existing channel work.
- remote_host_controller::prepare_owned_mode authenticates document/caller/run
  and reserves the service-owned driver before publishing an attempt.
- remote_host_network replenishment still requires the exact original registered
  source/run/scope and rejects Unknown; it is not a generic reconnect API.
- remote_native_forget_approval currently persists a public identity tombstone,
  not revocation of an independently approved running connection.
- browser-run-handoff retains per-channel presentation/WebAuthn and does not
  overwrite the stored enrollment with the newly authenticated run scope.
