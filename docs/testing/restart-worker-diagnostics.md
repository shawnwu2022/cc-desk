# Restart worker failure observations

This is test-only instrumentation for the two `RestartLifetime_` cases. It does
not repair process recovery, admit a historical Return, or certify installed
product behavior.

- Preserve the original worker wait limit of 300000ms, initial exit-code/job-count
  decision, failure record, final error, owned handles, and cleanup behavior.
- Only after a restart worker fails that decision, admit at most three additional
  read-only samples within both the remaining worker wait budget and a 20ms
  diagnostic window. Use at most two 5ms pauses; recheck the budgets before each
  sample and pause. Native queries and zero-duration process waits are synchronous;
  the deadline controls new observations, not OS scheduling or API completion.
- Each record exposes only schema/sample numbers, elapsed milliseconds, an optional
  job count, the held worker's optional terminal state, and optional membership
  completeness/worker-presence/other-member count. Raw process identifiers remain
  local to the fixed eight-entry query buffer and are never logged or reopened.
- Query errors remain null. A truncated or inconsistent member list is incomplete,
  with unknown membership values. The accounting, process wait, and member-list
  observations are separate calls, so the record explicitly is not an atomic snapshot.
- A later zero count remains diagnostic evidence; it never changes the initial
  failure to success. The unnamed containment job is not the historical named job,
  and none of these observations authorizes durable recovery.

The pure policy tests in `version_history_payload/terminal_diagnostics.rs` cover
sampling limits, exhausted/sub-millisecond budgets, complete lists, and unknown
membership. They can run in a portable Rust test harness importing that module.
Frontend source-contract tests check the failure-only hook and emitted field
allowlist. Actual Win32 behavior still requires the ordinary Windows tests; the
existing focused recovery workflow runs the same two restart cases serially.
