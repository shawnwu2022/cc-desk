# Fake Resize actor qualification

Scope: one existing memory writer actor with an explicitly injected bounded fake
Resize backend. Production factories inject no backend, and the only State
constructor is test-only. No native MasterPty implementation, lifecycle route,
Create/Close, ATC2 change, database migration or persistent grant is supplied.
The legacy LocalInputHandle::resize remains Unsupported.

## Admission and ordering

The internal request uses the existing lifecycle Resize shape, without changing
the wire schema or imposing a CLI software-version gate. It must pass wire
rows<=128, columns<=256, cells<=16384 and the backend/core rows<=100,
columns<=240, MIN_TERMINAL_COLUMNS intersection. Nothing is truncated. Invalid
wire shapes return Bounds; valid wire dimensions outside the core return
Unsupported before any fake call. Within an enabled fake capability, wire bounds
precede core dimension capability checks; a default-off request is Unsupported.

Resize shares the bounded actor queue and credits with input, output, snapshot,
ACK, control and rebind. Processing checks exact live PeerScope identity,
generation/fence/mode/dimension/sync state and the existing grant/lease clock.
The fake capability is explicit, not inferred from Controller role alone.
There is one per-actor memory replay ledger: at most 64 accepted Resize records,
no eviction, increasing sequence floor, exact identity/payload match required.
An exact duplicate returns the recorded fact without re-execution. Conflicting
operation/sequence reuse rejects; reaching capacity rejects before side effects.
There is no cross-process durability or restart recovery claim for this ledger.

Before an admitted operation can call the fake, it revokes the current issued
snapshot, ACK eligibility and all windows, without resetting receipt/pending
state, window identity counter, sequence floors, sync counter or ownership fence.
Dimension/structure overflow is preflighted before the fake and freezes the actor.
Input queued before Resize may complete first; input using an old view queued
after it cannot commit. Unsettled input receipts still block different input.

## Commit, cancellation and disclosure

Only an exact fake size success is followed by TerminalCore::resize in the same
actor turn. Changed size increments dimension and structure once; generation,
mode and ownership fence stay unchanged. Same size calls no fake and changes no
terminal epochs, but still invalidates old eligibility. Sync advances only on a
later fresh snapshot capture. Neither result nor historical replay grants input:
fresh complete delivery, exact ACK and a new window remain necessary.

KnownNotApplied retains the old core and a replayable negative fact. Unknown,
wrong size, unexpected core failure, dropped/expired wait or response loss seals
the actor; there is no rollback or automatic re-execution. If authority is lost
during a successful fake call, local size/epochs are still committed as facts,
but the current result is not disclosed under expired/revoked authority.
Authority sampling is followed by fresh cancellation/deadline checks before the
effect and final response. A queued reply cannot be consumed as success after
the absolute monotonic budget (0<budget<=1s).

A new-effect response must be observed within that budget. Until observation,
the gate refuses new snapshots/ACKs/windows/input/rebind/new Resize; actor loop
checks before and after receive seal an unobserved expired operation. Pending
drop/timeout also seals the exact actor immediately. This is intentionally
conservative, including abandoned replay waits. A timely observed reply's drop
does not seal. Historical replay creates no new-effect barrier and never
restores a view/window or replaces an outstanding barrier.

These are local candidate semantics, not preemption of a blocking native call.
Only bounded fakes are injected, including bounded fault pauses. No detached
thread timer is treated as cancellation, quiescence or resource retirement.

## Qualification and remaining gates

ResizeCandidate_001..013 cover actor commit, size/default-off rejection,
duplicates/conflicts/floor/capacity, wrong actor and stale state, same size,
old view/ACK/delivery/window rejection, input ordering and receipt retention,
failure classes, expiry/revocation, cancellation/late results, paused authority
sampling and unread reply deadlines. The full terminal tests cover actual
dimension/structure MAX preflight overflow; a forged MAX request in Host tests
only proves stale-request refusal. All IO is memory/fake and SQLite is temporary.

Native wiring remains blocked on exact master/reader/writer ownership,
independent output drain, blocking-call outcome isolation and full old-IO
retirement. ResizePseudoConsole success is not proof of CLI redraw completion,
and queued output has no proven native size-transition causal marker.
The synchronous vendor call is not safe to plug directly into this actor.
See Microsoft's [ResizePseudoConsole contract](https://learn.microsoft.com/en-us/windows/console/resizepseudoconsole)
and [ConPTY synchronous IO/deadlock guidance](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session).
All prior PTY concurrency/handle, OS5, WebView, platform/device and production
acceptance gates remain open. Diagnostic qualification is not product delivery.
