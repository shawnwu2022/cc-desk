# Session close reconciliation

## Reported failure

On Windows, a first close could stop interaction while leaving the session open. The row subsequently showed stopped, and a second close removed it. A newly closed history row could also remain at the bottom until manual refresh.

## Corrected boundaries

- Legacy Windows control duplicates the exact owned child handle, uses the correct nonzero-success `TerminateProcess` convention, and requires the retained process object to become signalled before reporting successful stop. It does not reopen a PID or search process names.
- The pinned portable-pty 0.8.1 cloned Windows killer uses the inverse BOOL interpretation. Native `OwnedPty` already avoids it; legacy now does too.
- Termination waits run outside global PTY maps and outside the async command worker. Cleanup checks the registration's retained control identity. Reader and waiter callbacks carry that same identity, so an old callback cannot kill or remove a same-ID replacement.
- Reader failure or unsuccessful wait is not exit proof. Those paths retain unconfirmed ownership and emit an error instead of manufacturing `pty-exit`.
- A lost legacy control reply can be reconciled by the exact PTY exit event within a bounded wait. An unrelated exit or missing proof keeps the terminal open with `LEGACY_STOP_UNCONFIRMED`.
- Native close continues exact, read-only receipt reconciliation through a transient cancellation/status response failure within its original five-second deadline. No start is replayed. Missing exit proof retains unknown ownership.
- A completed close refreshes the affected project's activity-order snapshot after catalog reconciliation. Ordinary background output/status publications keep the existing row order. Explicit stop still retains output; close disposes it only after stop is confirmed.

## Evidence

Behavioral regressions were run RED on the released source and GREEN after the fix:

1. Legacy component → adapter → catalog receives a rejected kill response after the exact exit event; one close must remove and dispose the terminal
2. Native App → tree button → host → component → adapter → catalog receives one failed status read after accepted stop; one close must finish without another stop/start
3. A closed row's history identity initially appears below older rows; completed close must produce the same order as manual refresh, with subsequent background activity remaining stable

Final frontend verification: 167 Vitest files / 2294 tests passed, Vue typecheck passed, Vite build passed. Node policies passed after using the scoped Rust toolchain. Focused independent review found no remaining blockers.

The Windows maintenance subprocess additionally checks accepted first termination, duplicate close, retained live ownership after injected reader/control and wait failures, actual waiter settlement, and stale callbacks against a same-ID replacement. These platform checks require Windows CI. Full local Cargo check/test could not run because this Linux environment lacks GLib/GIO/GTK system libraries. Source review and frontend tests are not real Windows, ConPTY, or Claude/Codex CLI acceptance.

## Platform follow-up

On the Windows candidate, close both legacy Claude and native Claude/Codex while running, while starting, after natural exit, and from an unknown state. Verify one click removes the open terminal, recent history order agrees with refresh, duplicate clicks do not emit another stop, and failed control keeps the live row with a meaningful error. Confirm late output/exit and newer attempts cannot restore or remove the wrong terminal.
