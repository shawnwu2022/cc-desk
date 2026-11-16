# Codex archive latency

The archive command remains the official `thread/archive` operation. A displayed
active Codex row now retains a backend-only launch locator (native ID and project
cwd), scoped to the exact document/profile/root. This avoids rereading every
unrelated transcript merely to recover information the history reader just had.

Locators are not write authority. The existing fresh `thread/read`, held-root
containment and `session_meta` ID/cwd checks still run before archive dispatch.
Changed identity, revoked scope and a mismatched profile are rejected. No native
command is replayed after an unknown result. A completed attempt retires the locator; cold
lookups and archived-result reconciliation retain their fresh observation path.

Retention is bounded to 512 entries and approximately 128 KiB per scope. A fresh
first page replaces older hints; obsolete read generations cannot publish over a
newer read. No persistent cache or duplicate transcript is created.

`NativeArchive_DisplayedLocatorAvoidsRescanButRechecksTarget_074` compares full
observation with point lookup in the same isolated fixture and verifies changed
target metadata, wrong caller, stale profile and revoked scope. The isolated
`NativeArchivePerf_IsolatedStageMatrix_029` measures the complete wrapper with a
synthetic RPC process; its timings are not Codex Desktop measurements.

Per-operation app-server startup, native RPC verification and owned-process
cleanup remain. Connection reuse and background cleanup are separate lifecycle
changes, not claimed by this optimization.

## Isolated Windows comparison, 2026-11-16

Same binary, fixture and warmed disk state; `CC_DESK_ARCHIVE_PERF_COLD=1`
selects a fresh service with no locator after the initial history read. `0`
uses the displayed-row locator. One sample per cell, not a percentile claim:

| Sessions / projects | Full-scan fallback | Displayed locator |
| --- | ---: | ---: |
| 1 / 1 | 228.36 ms | 197.63 ms |
| 128 / 1 | 305.90 ms | 201.44 ms |
| 1024 / 1 | 1039.64 ms | 220.47 ms |
| 1024 / 8 | 1012.04 ms | 207.35 ms |

These are archive wrapper durations including helper startup and cleanup, using
a synthetic Node RPC peer. They exclude the separately measured background
history refresh and do not claim native Codex Desktop latency. Initial history
loading is unchanged. Entries outside the bounded locator cache use the fallback.

Validation: registry 38 tests and archive 19 tests; the ignored stage matrix was
run explicitly in both modes. The point-lookup regression also compares 128-row
full observation (91.551 ms) with retained lookup (0.053 ms) while still rejecting
changed target metadata, foreign callers, stale revisions and revoked scopes.
