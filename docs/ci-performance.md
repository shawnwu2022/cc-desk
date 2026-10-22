# CI runtime work

## v0.24.0 observed baseline (2026-10-22)

The [source-bound run/job/step ledger](ci-runtime-v0240-baseline.json) was captured
from GitHub metadata. Workflow intervals below use creation to last update, including
queue/coordination; job intervals use actual start/end. These distinct sources and
cache states are observations, not a controlled comparison, p95 or a ten-minute SLA.

| Run/attempt | Source | Workflow interval | Critical observations |
| --- | --- | ---: | --- |
| [Stable PR CI 39717281444/1](https://github.com/shawnwu2022/cc-desk/actions/runs/39717281444) | PR head `6229bae`, tested merge `1890c374` | 13:41 | Producer 5:11; shard 6 8:10 |
| [Stable main CI 39718592492/1](https://github.com/shawnwu2022/cc-desk/actions/runs/39718592492) | `24316ad9069df76f2de669fe7b17eeca2a2228be` | 11:24 | Producer 4:08; shard 6 6:55; aggregate 0:12 |
| [Stable Release 39718592542/1](https://github.com/shawnwu2022/cc-desk/actions/runs/39718592542) | `24316ad9069df76f2de669fe7b17eeca2a2228be` | 12:20 | Windows 9:35, signed build step 8:15; exact-CI wait 1:35 |
| [RC4 main CI 39704890833/1](https://github.com/shawnwu2022/cc-desk/actions/runs/39704890833) | `08261b92956f45cdde1740bfee470110f86f4bb4` | 11:25 | Producer 3:39; shard 6 6:26; shard scheduling delay about 0:54 |
| [RC4 Release 39704890848/1](https://github.com/shawnwu2022/cc-desk/actions/runs/39704890848) | `08261b92956f45cdde1740bfee470110f86f4bb4` | 12:26 | Windows 10:14; signed build step 8:38; exact-CI wait 1:02 |

The stable main producer's 17-second pinned-metadata preparation includes npm
preparation; it is not all SDK compilation. Its original inventory compilation took
103 seconds and doctest/inventory step 43 seconds. Shard 6's execution step took
388 seconds. A late output line from one test is not an isolated duration.
Removing the entire stable Release CI wait still leaves 10:45 of workflow time;
rebalancing shards alone cannot establish a complete release below ten minutes.

[PR 43](https://github.com/shawnwu2022/cc-desk/pull/43), head
`56569595e15fd521f722c2d041726578404729ee`, has reusable calibration and validation
ideas, but its single-sample timing table belongs to an older 1,361-name library
inventory and toolchain/source. Its final CI interval was 10:58; the reported 7.3%
weight estimate is not measured wall-time improvement. Do not merge its old runner
or timing data over current source/upload-evidence bindings. Any adoption needs new
exact-source executable/inventory evidence, conservation tests and a controlled
hosted comparison; absent that evidence, retain the current placement policy.

### Prepared exact-name calibration candidate

The next-version candidate ports PR43's bounded measurement idea, not its stale
weights or runner. `scripts/windows-rust-calibration.mjs` uses the current
`verifyBundle` and `executeHarness`, preserving upload-evidence content hashes,
exact checkout/source/run/attempt, four original inventories, executable bytes,
compiler/profile/OS/architecture and actual Job/elevation observations. Calibration
code and target-file bytes must also match their committed source. An incomplete,
failed, ignored, timed-out or foreign result cannot supply a weight.

Calibration is opt-in only: a CI dispatch with `calibrate_rust=true`, or a PR title
containing `[calibrate-rust]`. The default is false. The existing first four shard
jobs each run two of the eight advisory groups after ordinary execution and original
evidence upload, with one shared eight-minute measurement window per pair. The
original 30-minute job cap remains; a long ordinary job may therefore leave too
little time for calibration, which is failure/incomplete evidence, never a weight.
No extra skipped job is introduced: the unchanged release policy requires every CI
job to succeed. At most four calibration processes run concurrently, and optional
measurement does not debit the original 25-minute shard-execution window.
This is warm measurement after ordinary work, not the old PR43 fresh-job condition.
Each original name gets its own exact invocation; launch and complete shutdown
are included in its duration.
Raw listing/output hashes and actual executable identity are retained with the
receipt. `readCalibrationWeights` rereads those logs rather than trusting summary
counts. Its input plan must come from `verifyBundle`; structural JSON or synthetic
parser tests are not authentic measurements.

The 16 predefined names are candidates to remeasure, not evidence that their old
durations remain valid. Current ordinary producer/verification calls continue to
use the original two-argument `partitionNames` policy. A third argument supports
explicit offline comparisons only, with stale/invalid weights rejected and
unmeasured names still selected exactly once using the conservative heuristic.
No measured table is adopted and no net speedup is claimed by this patch.

The ordinary `Rust checks` gate still requires producer, static and all 16 shards;
calibration is neither native coverage nor release admission. Its failed raw output
uses the existing immutable upload helper. Hosted execution requires a separately
authorized candidate push/PR; none has occurred in this local implementation.
First compare one matched control/candidate pair, then at least three matched pairs
within an approved runner budget if promising. Preserve source/run/attempt, queue
and cold/warm distinctions. Retain the current placement unless the approved
whole-path improvement and regression/runner-budget criteria are actually met.

## Post-0.23.1 CI efficiency batch (2026-10-19, local patch)

Baseline source is main `37fef8b730660c42f0967fb3e0e0b93809174dd5`.
Public v0.23.1 was independently observed at `2026-10-19T05:01:09Z`, with
nine assets. No remote write had occurred at the initial local-review checkpoint.
The user subsequently authorized commit/push, a Draft PR and exact-source hosted
validation; merge and release remain outside this task. The original
checkout remains untouched; the patch is on `perf/ci-efficiency-oct19`
in a separate checkout under the ignored `.cache/ci-efficiency-oct19` directory.
Managed worktree creation was unavailable for this task and writing the original
Git metadata was blocked, so isolation uses a local shared-object clone.

Read-only GitHub job/step metadata, immutable job IDs, exact run/attempt and
source identities are retained in [the baseline ledger](ci-efficiency-2026-10-19.json).
The intervals below span the first job start to last job completion, excluding
initial queue time. Runner minutes sum job durations, not billable rounding.
These distinct attempts and sources are separate observations, not a controlled
before/after speed comparison.

| Workflow/run/attempt | Job span | Runner minutes | Outcome |
| --- | ---: | ---: | --- |
| PR CI `39197993040/1`, source `7d5cfdf` | 12:40 | 89.40 | Shard 5 failed; aggregate failed |
| Same PR CI `39197993040/2` | 10:13 | 94.82 | All 23 jobs passed; all jobs rerun |
| Main CI `39200098896/1` | 12:46 | 100.00 | Shard 1 artifact upload failed; aggregate failed |
| Same main CI `39200098896/2` | 12:22 | 94.05 | All 23 jobs passed; all jobs rerun |
| Failed Release `39200098954/1` | 13:01 | 27.08 | Three signed builds passed; preflight failed |
| Release `39201292937/1` | 13:09 | 25.33 | All six jobs passed |
| Windows package `39197993029/1`, source `7d5cfdf` | 21:32 | 21.83 | Both original jobs passed |

The main attempt-2 producer took 252 seconds, the slowest shard 469 seconds
(439.263 seconds in the library harness), frontend 142 seconds, ordinary
roundtrip release exclusion 381 seconds and debug scenarios 356 seconds.
The **7:49** metric is that single shard job; **12:22** is the entire job span.
The latter consists of one second before producer start, 252 seconds of producer,
four seconds to shard 6 start, 469 seconds of that shard, two seconds to aggregate
start and fourteen seconds of aggregation. These are not two shard measurements.
Shard 6's source-bound job log reports plan hash
`e7f16f11ca6612c5e6b4c94361c7ed9efda186a0358c49f03d0ae100316fa549`;
RestorePresentImage was the last pending original name, but its output-observation
timestamp is **not** an isolated test duration. Most shards started 3–4 seconds
after the producer; changing matrix order cannot remove this tail.
The successful Release's Windows/macOS/Linux builds took 590/330/410 seconds;
Windows signed compilation accounted for 509 seconds, with no profile changes
adopted here.

The failed main attempt's actual shard-1 job log (`117892196441`) reports a
completed receipt, exit zero, 92 passed and two original ignores across its four
harnesses. All five `CreateArtifact` attempts then timed out. This is a transport
failure, **not** a verified coverage pass: the artifact bytes were not recovered.
No ignore, test assertion, required check or timeout was changed to turn it green.

### Implemented graph and dependency changes

- Windows package verification and actual installer construction now start in
  independent **Windows** jobs. Every original full frontend/adversarial test,
  runtime guard, strict all-target Clippy, release-policy assertion, NSIS build,
  install/reinstall/relocation and missing/corrupt runtime probe remains.
  The original `installer` check name is a mandatory final gate over formatting,
  validation and package jobs. It rejects failure, cancellation, skipped or missing
  results before downloading the exact same-SHA/run/attempt candidate and audit
  report. It then rechecks manifest identity and installer SHA-256 before uploading
  the original verified-package name. Intermediate candidates clearly remain
  test-only and pending full validation; no publication permissions were added.
  Debug validation has a separate dependency cache namespace; package compilation
  keeps the original no-LTO cache/profile. Both exclude workspace crates.
- Release admission reuses the existing `ciWaitState` rule for a single cheap
  exact-source CI read. Already failed/cancelled CI prevents redundant new signed
  builds; pending/missing CI still permits parallel builds. API errors reject.
  This does **not** prevent a CI failure that appears after admission, recover
  failed uploads, or reuse old signed builds. Full post-build preflight, its
  120-minute wait, fresh coverage/run/attempt checks and signatures are unchanged.
- Ordinary CI's producer/static setup now caches locked npm download dependencies,
  as the frontend and packaging jobs already do. Both still execute `npm ci`,
  rebuild the metadata helper, and compile/check their original source. No
  executable, test result, coverage or source-bound receipt enters this npm cache.
- The three authoritative ordinary-CI uploads (bundle, shard evidence and final
  coverage) use a bounded immutable-upload composite. A failed upload acknowledgement
  first triggers a read of the **same exact artifact name in the current run**.
  Only equal file inventory, sizes and streaming SHA-256 digests confirm a completed
  unknown upload. An unreadable/missing artifact permits one more upload using the
  original name, with `overwrite: false`. Different readback bytes fail without
  retrying or replacing anything. Unconfirmed transfer, cancellation and any original
  test failure still fail the workflow; this does not certify coverage by itself.
  Original upload is still attempted after failure/cancellation, as before. Recovery
  does not run on cancellation. The workflow's outer upload step has a five-minute
  bound; the original 25-minute shared execution budget and 30-minute shard job
  remain. No artifact, permission, credential, assertion or ignored set is deleted
  or weakened. Both local action files enter the exact plan content hashes.

For the measured package job, the moved verification steps sum to **599 seconds**;
all retained package/setup/post steps sum to **691 seconds**. Ideal overlap of
the original work would change 1,292 seconds to roughly 691 seconds, before the
second runner's setup, new cold cache, scheduling and final transfer/gate costs.
That is a graph estimate (~11.5 minutes plus overhead), **not a measured saving**.
Extra checkout/toolchain/npm preparation and artifact transfer may increase total
runner consumption. Ordinary CI npm caching also has no measured net hosted gain.
The ten-minute objective remains unmet by evidence in this batch.

### Tail placement investigation and independent review

All sixteen authenticated decoded main-attempt-2 job logs were read and reconciled
against the same SHA/run/attempt/plan hash. Their selected library inventories are
retained in [the advisory observation record](ci-shard-observations-2026-10-19.json).
The **1,587 unique names** reproduce every original partition exactly. This is
decoded-log evidence, not archive-byte validation or per-name timing calibration.

An isolated-library-partition candidate for the last pending RestorePresentImage
case preserves all names but moves the remaining workload into fifteen partitions.
The existing heuristic maximum rises from **1333 to 1414** (about 6.1% in heuristic
units, not seconds). A local implementation was reviewed and **withdrawn from the
production partitioner** because the CI-4 net-gain adoption requirement is not met.
`scripts/windows-rust-shards.mjs` remains byte-identical to the base source.
The separate comparison tool prepares that candidate without executing it or
granting coverage authority:

```powershell
node scripts/windows-rust-shard-placement.mjs docs/ci-shard-observations-2026-10-19.json
```

It first requires all sixteen baseline partitions to match the default algorithm,
then reports `advisoryOnly: true`, `durationPrediction: null` and
`hostedNetGainVerified: false`. It is referenced only by its CLI and tests; no
workflow, producer, executor or release gate invokes it. No old calibration
weights are adopted and no unsupported wall-time estimate is emitted.

Independent read-only review found no blocker in the Windows package split,
early CI rejection or cache changes. Review of the upload composite caught an
actual schema error: `timeout-minutes` is not supported on internal composite
steps by the [runner metadata schema](https://github.com/actions/runner/blob/main/src/Runner.Worker/action_yaml.json).
The key was removed from the action, the total bound moved to the workflow call,
and a failing-then-passing regression added. Review also identified the unsupported
production placement adoption above; default placement was restored. Final review
confirmed both items resolved and found no additional blocker. Hosted composite
execution and real transport-fault recovery remain unverified.

### Validation and remaining evidence

The two initial ConPTY graph/gate regressions failed on the old workflow; three
Release admission regressions reproduced known failed/cancelled CI and API errors
being admitted. After the change the complete ordinary frontend job's Node policy
command initially passed **250 tests, six existing Windows skips, 256 total**. This includes
real child-process orchestration, the original signature/coverage/refusal cases,
and new executable package-gate/identity/attempt/hash/traversal checks.
The two affected Vitest config suites passed **44 cases**. Typecheck passed.
Workflow YAML and extracted PowerShell syntax were checked separately.
Local Node is **24.18.0**; hosted Node 22 validation is still required.
The final second-batch ordinary Node policy command passed **254 tests with six
existing Windows skips (260 total)** in 20.11 seconds. Added cases execute real
filesystem/CLI byte identity, CRLF/binary/equal-length drift, extra/missing/linked
files, privacy and recovery-result handling, plus the offline candidate/default
layout comparison. No real GitHub write or platform installation ran locally.

Dependency installation initially hit a missing offline npm-cache entry; a normal
`npm ci` with a writable task-local cache succeeded (293 packages, nine seconds).
Vitest initially failed before discovery because esbuild could not read ancestors
of the temporary C: checkout. Moving the isolated checkout into the ignored E:
workspace cache resolved that startup limitation; it was not an application test
failure. No ACL, token, installed application or real session state was changed.

The old `perf/ci-duration-balanced-p1` branch still has unique calibration/weight
commits `189650d` and `5656959`. No weights or calibration workflow were copied.
Direct artifact ZIP reads returned HTTP 401; the connected artifact tool returned
a file reference whose local HTTPS transfer failed. API digests and decoded job
logs alone do not verify archive bytes, so historical per-name calibration remains
unverified here. Same-attempt shard/bundle/coverage validation is unchanged;
failed-only cross-attempt reuse remains deliberately unimplemented.

Minimum hosted follow-up, owned by the parent after review and authorization:

1. Run one exact-source ordinary PR CI and Windows package workflow on existing
   runners/Node 22; require all sixteen shards, unchanged full inventory/ignores,
   existing aggregates and original install probes. Compare against this ledger's
   full wall spans and runner totals, separating cold/warm caches. Record the
   package candidate/download/final hash checks and composite action loading.
2. In a disposable current-run artifact test, exercise primary success, a completed
   upload with lost acknowledgement, no artifact plus one retry, different readback
   bytes, original test failure and cancellation. Confirm immutable names/IDs and
   no overwrite/delete. Local mocked statuses cannot establish these service facts.
3. Before adopting any tail placement, obtain archive bytes and source-bound exact
   calibration, then run a comparable candidate/control using the same test inputs,
   toolchain, runner class and complete gates. A pending-only log is not a duration.

Next evidence must compare matching inventory/toolchain/runner/cache conditions.
Retain first-start-to-final-gate time, queueing, actual fresh/rebuilt compiler
metrics, transfer time and runner totals. Signed Release timing needs a separately
authorized future release; local scripts do not certify real CLI, WebView2,
installed-platform acceptance, D20 or new signed artifacts.

## PR62 projection and optional-observer regressions (2026-10-19, local)

The first strengthened unchanged-history case reproduced two project catalog
rescans for one forced read, although the row array stayed identical. Removing
`loaded` and `error` from the row signature reduces that case to zero rescans;
failure diagnostics and absence-evidence revocation still update independently.
An automatic Legacy title refresh with 25 cached sessions also reproduced an
intermediate 20-row cache. Pages now publish only as a complete replacement;
later-page failure retains all 25 trusted rows, while a cold failure publishes none.

Native resumed sources and Legacy PTYs with proven default roots now accept owned
OSC invalidations when the optional hook observer is off or unavailable. Hook
events remain gated, disabling observation cancels hook retries, and unproven
sources cannot request reads. Nine additional cases plus the strengthened existing
case cover these paths. The five affected suites passed all 285 cases, typecheck
and production build passed. These are local synthetic-store results; fresh hosted
CI and Windows package gates are required, and final real WebView2 title-sync
acceptance remains unverified.

The shared-dependency follow-up adds seven cases. Three RED cases reproduced
stale archive ambiguity after another project's same-ID row was added, removed or
invalidated. Native-ID uniqueness changes now refresh only archives that depend
on those IDs; an unchanged-cardinality title update stays local. A trailing-slash
archive key reproduced three rescans during an unrelated unchanged read; using
the adapter's canonical path/merged-key semantics reduces that case to zero.
Two further RED cases reproduced stale folds in project A when project B detected
replacement or loss of the shared root proof. Revocation now queues projects
bound to that old root, retaining unrelated-root projections. The four affected
suites pass 160 cases. The full local inventory at this checkpoint passed 2,863 cases with two
existing skips across 193 files in 88.33 seconds using four workers, including
the unchanged 5,000-session/30-owner/120-change workloads. The continuity loop
recorded 27,793 ms within that run; this is a workload pass, not a comparative
speedup claim. Typecheck and production build passed. Fresh exact-source hosted
CI and Windows package gates remain required.

A further RED regression showed two distinct valid title snapshots colliding
under delimiter concatenation, leaving the catalog at the previous title.
History row tuples and sorted archive-ID arrays now use structural JSON encoding.
The four focused suites pass 161 cases; the fresh full inventory passes 2,864
cases with two existing skips across 193 files in 78.46 seconds with four workers.
The unchanged stress continuity loop recorded 23,194 ms. Typecheck and production
build passed; exact-source hosted gates and final WebView2 acceptance remain
separate requirements.

## Runtime performance audit and falsified candidates (2026-10-17, local)

A full application-runtime audit was run on `perf/runtime-batch1` (based on main
`351f5397`). Three optimizations were planned from the audit and **all three were
falsified by measurement before adoption**. This entry records them so the
negative results are not re-litigated.

Baseline measurements on the author's machine, all under the debug test profile
unless noted. These are local samples on a busy workstation, not hosted numbers,
and the stress cases are load-sensitive (the same case measured 22.6 s / 28.4 s
across adjacent runs).

- **Vue rendering dominates the multi-session stress case.** Instrumenting
  `Stress_30Owners120Changes_002` for one run split its 120 changes into
  `refresh=435 ms`, `projection=18,774 ms`, `render=22,499 ms`. The catalog
  refresh that the audit proposed to optimize is 1% of the case; component
  rendering is 54%. `refreshCatalog` is additionally a no-op on those paths:
  it returns early while `loading` is true, and the per-tab watchers are
  coalesced by `queueCatalogProjects` + `nextTick`.
- **Append-resume name parsing saves less than it costs.** The largest real
  transcript on this machine is 37.3 MB / 17,058 lines. Its cost split
  (Node, same file, cold): read 49.6 ms, UTF-8 decode 169.7 ms, full
  `JSON.parse` 166.2 ms, whole-file SHA-256 80.9 ms. Any safe resume needs a
  prefix/head guard against rewrites, so the guard alone is roughly half the
  full-parse cost, and it only pays off for large files. Cost also scales with
  the largest files, not the session count. Not adopted.
- **Legacy 4 KiB `pty-output` emits are not measurable.** A 1 MiB burst through
  `pump_pty_output` produced 257 emits in 21 ms; batching to 64 KiB produced 17
  emits in 19 ms. The decoder itself (43 ms) is only 24 ms above raw
  `decode_output` (19 ms), so the `to_vec` copy has no separable cost here.
  Not adopted. Real WebView IPC serialization on the frontend is not covered by
  this measurement.

Two audit claims were also disproved during the same pass:

- **i18n splitting is not worth it.** Extracting the `zh` locale to a dynamic
  import moved the main chunk from 513 kB to 511 kB (gzip 154.4 kB): CJK strings
  compress too well for the uncompressed-byte estimate to hold.
- **`get_home_data` is not double-scanned at startup.** `loadCache` has no
  production caller (only tests reach it); the real startup scan is the single
  `loadManagedProjects` → `get_projects` path.

The audit's static severity ranking did not survive measurement. Future runtime
performance work should measure the share of the actual critical path first, and
only then choose what to change.

`BenchmarkActiveFullScan_Real_004` was repaired in the same branch. It read the
live `~/.claude/projects` tree, so active development changed `last_active_at`
and left half-written JSONL lines between rounds; the three-variant equivalence
gate failed with a round/variant that drifted per run (round 2
`SnapshotStreamName`, then round 1 `SnapshotLegacyName`). Phase 0A now copies the
real tree to a one-shot static snapshot before comparing variants, matching the
existing tempdir-copy pattern in `benchmark_active_full_scan_real`. The gate had
also been asserting a hardcoded `variants_are_equivalent: true` rather than the
comparison result. With the snapshot in place the benchmark passes (47.85 s) and
reports `append_resume_required = true` for the debug profile.

Delivered on this branch, each independent of the falsified candidates:
dependency removal (10 never-imported packages), 11 blocking store reads moved
onto the blocking pool, an 80 ms trailing debounce on the native terminal
`ResizeObserver`, the benchmark repair, and the matching documentation updates.

## Session-list repair follow-up (2026-10-15, local evidence)

The review started from `aed8abbf4594925bc547fe7dcef6efe2836f3752` on
`fix/session-list-title-status`. At this local measurement checkpoint, changes were
uncommitted and no hosted CI had been triggered.
The pre-optimization full Vitest run used installed dependencies and existing OS/
Vite caches: **179 files, 2,590 tests, 2,587 passed, one failed, two existing skips;
40.00 seconds**. The failed `WorkspaceClose_SelectRemaining_002` directly changed
`tab.title`; it now exercises the real catalog rename/metadata writer while retaining
the same expected saved display name and terminal ownership assertions.

Two bounded optimizations preserve verification rather than cache its outcome:

- The ordinary `frontend` job no longer runs an independent `npm run build` before
  `npm run test:production`. `tests/production/serve.mjs` still unconditionally awaits
  a fresh normal Vite build before preview, with the actual production CSP; the
  Playwright configuration still refuses to reuse an existing server. This removes
  **one of two explicit normal-entry builds** in that job. Other fixture builds,
  typechecking, all Vitest/Node policies and production browser assertions remain.
  A standalone local build measured **4.49 seconds** of Vite build time; removing
  it is a graph saving, not a measured hosted-run wall-time reduction. No `dist`
  reuse option or cross-source build cache was added.
- Nine new debounce/revocation/failure cases (`History_061`–`065`) keep the same
  real stores/runtime, 20-event burst, busy reads and assertions, using a local
  fake clock to advance 249/1/350 ms rather than sleep. The recorded case sum was
  **5,192.38 ms before, 190.15 ms after** within the two full-suite runs; an isolated
  after sample was 86.51 ms. All nine passed. Whole-suite wall time was 40.00 seconds
  before and 42.87 seconds after while other checks also ran; this does not establish
  an end-to-end CI speedup. Full inventory remains
  the verification gate. Real process, HTTP, pressure and CLI tests keep real time.

Cleanup inventory: only the redundant ordinary-CI build step was removed; its
replacement coverage is the existing mandatory production startup server/build
and CSP browser suite, protected by `FrontendBuild_ProductionOwnsBuild_006` in
`tests/scripts/ciRustWorkflow.node.mjs`. No tests, workflows, required checks,
release dependencies, original assertions, ignored classifications or evidence
archives were deleted. No other old item was proven obsolete.

The unmerged `perf/ci-duration-balanced-p1` branch has two unique commits
`189650d`/`5656959`. Its old-inventory weighted model estimated 7.3% lower maximum
shard load, but its original calibration artifacts/current-inventory comparison
were not available here. No merge, checkout or activation of those weights was
performed. Exact same-source/run/attempt receipts and target-runner cold/warm CI
timings remain prerequisites; the old model cannot certify this working tree.

Final local checks: full Vitest **179 files / 2,588 passed / two existing skips**;
the ordinary frontend job's Node policy inventory **243 passed / six existing
Windows skips**; typecheck and standalone build passed; production startup/CSP
passed both language cases using installed Chrome through the existing
`PLAYWRIGHT_CHROMIUM_EXECUTABLE` option. The default bundled Playwright browser
was absent. Preview children from these runs required explicit owned-PID cleanup
after assertions completed; no unrelated process was stopped. Scope-core compiled
the production Rust source and passed **140 tests**, format checking and strict
all-target Clippy with Rust **1.95.0**.

Follow-up verification compiled the complete Tauri library harness with
`cargo test --locked --offline --lib --no-run --message-format=json` in 40.61
seconds. The executable identified by that compilation listed **1,532 tests**,
including all original eighteen Job-free tests. The repository's read-only
`windows-rust-tests.ps1 -Action Probe` observed **inJob=True / elevated=False**;
the unfiltered library suite remains unverified on this host. No Job/token
settings, exclusions or test contracts were changed. The library's actual
`cli::native_projection::` subset passed **138 tests / 1,394 filtered out**,
and complete-crate format checking passed.

Complete-crate `cargo clippy --locked --offline --all-targets -- -D warnings`
failed on Rust 1.95.0 with three `nonminimal_bool` diagnostics in
`updater_http.rs:104`, `version_history/windows/context.rs:2170` and
`version_history/windows/manager_process.rs:205`. Those three files match both
HEAD and local main; no independent main-baseline lint run was performed. The
already-installed CI toolchain **Rust 1.98.1 / Clippy 0.1.98** passed the same
complete-crate command (using `cargo +1.98.1`) in **41.70 seconds**, exit zero.
`cargo +1.98.1 test --locked --offline --lib cli::native_projection::` then
compiled the complete library harness and passed the same **138 tests / 1,394
filtered out**, exit zero, in **88.15 seconds** including compilation.
No toolchain was installed or default changed, and no production code was altered
for this expanded verification. Hosted CI, the unfiltered full library suite,
actual CLI and installed WebView acceptance remain separate evidence gaps.

This change is prepared on `perf/ci-runtime`, based on main
`db517757eefd323b3eaa4c69224bdd8f74b7feb6`. The target is an actual ordinary
daily CI completion within ten minutes on the existing GitHub-hosted runners.
That is a measurement target, not a timeout change or a claim that a hosted run
has already met it. No test exclusions, native admission checks, required-check
configuration, publication permission, or release source policy are relaxed.

## Workflow audit

The table covers every workflow, including dormant diagnostics. Existing event
and path filters stay unchanged; no additional path-based skips are introduced.
Caches contain dependencies, never a substitute test result, payload receipt,
coverage archive, or signed candidate from a different source/run.

| Workflow | Work and bottleneck before this change | Implemented change | Coverage and remaining boundary |
| --- | --- | --- | --- |
| `ci.yml` | Full frontend checks, Windows Rust inventory/execution/lint, and independent roundtrip compile policy; Windows execution dominates ordinary CI. | See the ordinary CI implementation and measured validation appended below. | The final required check must fail when any shard/compile/lint/coverage validation fails. Required check names and original coverage archive filenames remain fixed. |
| `release.yml` | Full exact-main CI preflight completed before any of three platform builds began. Rust and npm dependency caches already existed. Each platform already built its frontend exactly once through Tauri. | A read-only exact-current-protected-main/checkout/version admission job precedes parallel full preflight and three signed builds. Separate signed-platform dependency cache keys; workspace crates explicitly excluded. Already-compressed installer/image/update archives upload with compression level zero. | Publication still needs `[preflight, build]`; exact CI run/attempt artifact bytes, source rechecks, same-run platform inventory, all three actual updater signatures, recovery backup, and final mutation gates remain required. No parent-SHA, recent-green, or cross-run binary reuse. |
| `conpty-integration.yml` | Formatting plus Windows package job; full frontend/typecheck/policy tests, runtime guards, lint, debug/release builds, and actual install/reinstall/relocation probes. Existing dependency caches and latest-branch cancellation. | Isolate the no-LTO test-package dependency cache from signed Release/trace builds; exclude workspace crates explicitly. Skip recompression of the compressed NSIS package at artifact upload. | Every original check and package probe remains. The full frontend suite is intentionally retained on this Windows package job; sharing a recent ordinary-CI pass would not establish this job's verification. |
| `d11-document-isolated.yml` | One exact-source Windows five-supervisor comparison with full/selected inventories and original assertions. Existing Rust cache. | Share the ordinary Windows debug-dependency cache namespace with focused recovery diagnostics; exclude workspace crates explicitly. | Each source-specific comparison and original five-test assertion stays intact. Independent comparison runs are retained, so cancellation is not added. |
| `d12-scope-core.yml` | Linux/Windows/macOS production-source test+strict lint matrix; every run started with uncached dependencies. | Cache the `scope-core` workspace dependencies and cancel a superseded run for the same PR. | All three OS entries, tests, `--locked`, and strict `-D warnings` lint remain. Workspace source is recompiled. |
| `d13-observer-core.yml` | Same three-OS test+lint matrix, uncached dependencies, floating Rust compiler. | Cache the `observer-core` dependencies, cancel superseded same-PR runs, and use the already-validated Rust 1.98.1 pin. | HTTP/reporter/observer tests and strict lint remain on every OS. Pinning avoids compiler drift and unrelated cache churn. |
| `d14-transport-core.yml` | Same three-OS test+lint matrix, uncached dependencies. | Cache `transport-core` dependencies; superseded same-PR cancellation. | Original FIFO/owner/ACK/output assertions and strict lint remain on every OS. |
| `d17-input-core.yml` | Same three-OS test+lint matrix, uncached dependencies. | Cache `input-core` dependencies; superseded same-PR cancellation. | Original staged-input/identity/no-replay assertions and strict lint remain on every OS. |
| `history-payload-evidence.yml` | Nine versions × two cases: eighteen independently fresh Windows jobs, each compiling the same dependency graph; no Rust cache. | Add one shared dependency-cache namespace across the matrix. Only obsolete PR-triggered runs are cancelled; explicit reviewed-SHA dispatches retain separate run identities. | Eighteen fresh jobs, all exact fixture/source/run/attempt bindings, native capability gate, receipts and evidence remain. No runtime/install state or evidence is cached. |
| `history-recovery-diagnostics.yml` | Windows production-library focused probes and original inventory counts. Existing Rust cache; obsolete PR runs could consume capacity. | Same-PR cancellation and a shared Windows debug-dependency cache namespace with the isolated supervisor comparison. | All probe selectors, inventory counts, failure propagation, custody/no-replay/security assertions, and original deadlines remain. |
| `paste-cli-acceptance.yml` | Pinned real Claude CLI, historical/field-shape generation, exact release gate, separately labelled characterization, lint and evidence. Existing caches and concurrency. | Isolate its ordinary dependency cache namespace; explicitly exclude workspace crates. | Real CLI version, release-gate cases, characterization boundaries, platform checks and metadata remain. A prompt or launch mode is never removed to speed up acceptance. |
| `paste-conpty-comparison.yml` | Downloads one fixed existing diagnostic executable; validates the pinned official Microsoft runtime/API; no frontend or Rust build. | No speculative cache or artifact reuse change. Existing concurrency already avoids obsolete runs. | Fixed diagnostic run/executable identity and runtime API checks stay exact. It has no repeated dependency compilation to remove. |
| `paste-runtime-trace.yml` | Full frontend/typecheck, ordinary+diagnostic Rust tests, lint, and a separate diagnostic no-LTO executable; existing caches/concurrency but floating compiler. | Pin Rust 1.98.1 and isolate dependency caches from ordinary/signed/test-package variants; explicitly exclude workspace crates. | Both ordinary and trace-enabled boundary suites remain, as do the distinct trace build flags and tracked-file check. |
| `unified-visual.yml` | Official Chromium and system dependencies, an immediate second `apt-get update` for fonts, contracts/typecheck and complete rendered verification. Existing npm cache/concurrency. | Remove the second package-index refresh; the official Playwright `--with-deps` install already performed it. | The official locked Chromium install, all three Noto font packages, original 13-image inventory, every geometry/interaction test, exact zero-pixel baselines, failure logs, and final original-outcome gate remain. |
| `windows-hosted-qualification.yml` | Two five-minute Windows image observations with no checkout, install, compilation, or tests. | No change: there is no duplicated install/build/cache work. | Both image observations, read-only desktop access, bounded child lifecycle, and explicit diagnostic-only status remain. |

## Savings that follow from the graph

The measured pre-change ordinary run is `37894886145`: the Rust check took
3,028 seconds (50 minutes 28 seconds), including 2,776 seconds (46 minutes
16 seconds) executing the suite. The roundtrip compile policy took 897 seconds
(14 minutes 57 seconds), including 434 seconds for release compilation and
189 + 170 seconds for debug/test compilation. These are the previous source's
hosted timings, not measurements of this unpushed performance branch. They show
that Rust execution and serial roundtrip compilation must be addressed before
an ordinary ten-minute result can be claimed.

Release previously took approximately `C + B + P`, where `C` is the exact-CI
preflight/wait, `B` is the longest platform build, and `P` is publication and
independent byte/signature verification. It now takes approximately
`A + max(C, B) + P`, where `A` is the cheap admission job. Ignoring runner queue
variation, the removed serial work is `min(C, B) - A`. For example, a ten-minute
preflight and twelve-minute build would remove about ten minutes less admission
overhead. This is arithmetic, not an observed hosted timing. If CI fails, the
overlapped candidate builds may consume runner time, but cannot admit publication.

Twelve core matrix jobs and eighteen historical payload jobs can now restore
compiled dependencies on warm runs instead of recompiling them from scratch.
Every source crate and test still compiles/runs. Cache keys retain the action's
OS/compiler/Cargo-manifest/lockfile/environment identity and add a workspace or
build-variant namespace. The first run after introducing each namespace is cold;
restore/upload cost, dependency size and eviction determine the actual saving.
The existing artifact uploads still retain the exact original files; compression
level zero avoids a second compression pass for the three signed platform
packages and one Windows test package. Visual setup performs one fewer explicit
package-index update per run. Six supporting PR workflows stop obsolete work
instead of occupying hosted-runner capacity after a newer source is submitted.

No changed timeout is presented as a speedup. The signed Release profile,
production safety assertions, all native exclusions/ignore classifications,
matrix platforms, exact selectors, and suite inventories remain unchanged.

## Release preflight runner occupation follow-up (2026-10-10)

The minimal follow-up changes only `preflight.needs` to `[admission, build]`.
All three signed builds still depend only on the exact protected-main admission;
publication still depends on `[preflight, build]`. Full preflight now starts
after successful builds, avoiding an idle CI-polling runner during those builds.
No script, compiler/profile, test inventory, version or provenance rule changes.

On released main `a02f944f8bb4933fd7ac22cf437ae0e27c4a5526`, Release
[38055870540](https://github.com/shawnwu2022/cc-desk/actions/runs/38055870540)
attempt 1 started preflight at 13:29:07 UTC; its last signed build ended at
13:38:48. Main CI
[38055870683](https://github.com/shawnwu2022/cc-desk/actions/runs/38055870683)
ended at 13:41:02. Delaying preflight would avoid approximately 9m41s of runner
occupation on that timeline, subject to scheduling and polling-phase variation.
This is a counterfactual runner-cost estimate, not a measured CI speedup or a
claim that ordinary PR CI is under ten minutes. Freed shared-runner capacity
may reduce queues when release and main CI overlap; actual results remain pending.

When signed builds determine the critical path, preflight setup/verification
becomes additional serial work (about 18s in that observed job). Failed/cancelled
builds skip preflight and publication. The existing 120-minute CI wait starts
when its step runs; a stalled CI can therefore be diagnosed up to one build
duration later (the unchanged build timeout is 60 minutes). The accepted tradeoff
does not relax exact main/SHA/run/attempt coverage or same-run signature gates.

## Local supporting-workflow validation

- The new build-admission behavioral tests were observed failing before the gate
  existed. Eighteen cases now execute the gate's actual policy and CLI, including
  advanced/unprotected main, wrong checkout/event/ref, invalid/mismatched versions,
  unavailable API, output injection, and no admission output after refusal.
- Existing release fixtures still exercise all three real signature verifications,
  tampered platforms, current-main/CI/attempt mismatch, authenticated archive
  digests/bytes, caller-forged coverage, archive traversal/bounds, draft backup and
  last-publication rechecks. The staged-release fixture copies the new shard
  validator module alongside its parent validator rather than weakening its checks.
- `actionlint` 1.7.12 validated every edited supporting workflow. A YAML graph
  check confirmed admission precedes both builds and full preflight, and publication
  still depends on both. Existing advisory triggers were compared with the base.

Hosted cold/warm ordinary CI and signed packaging wall times still need actual
runner evidence on the final reviewed source. The local Linux environment cannot
establish Windows installed-product, Job-free, WebView, ConPTY, real CLI, or
three-platform signed-release acceptance. This branch does not dispatch or mutate
the separately running 0.18.1 release.

## Ordinary CI implementation and timing

The ordinary-CI changes and final verification evidence are appended here with
the source/run identity once observed.

One `cargo test --locked --no-run` compiles the original library, binary and two
integration harnesses. The compiler job also retains formatting, strict Clippy,
the real application loader, executable PowerShell contracts and unfiltered
doctests. It publishes only the four executable harnesses, runtime DLLs and
all three pinned ConPTY manifest files beside both `debug` and `debug/deps`, plus
original full/ignored/selected inventories. CI debug symbols are disabled using
Cargo profile environment overrides; debug assertions, production code, release
profile, original deadlines and durability behavior are unchanged. This reduces
the size of the real executable copies/hashes made by existing security tests.

Sixteen Windows jobs restore the exact compiler artifact from their own SHA/run/
attempt. Source-file and payload hashes, original checkout path, observed Job
containment, exact assigned libtest listings, native exit codes and final outer
counts must agree. New unclassified tests are included automatically. Original
ignored workers remain ignored. Missing, repeated, cancelled or failed shard
receipts fail the existing `Rust checks` aggregate. The coverage artifact retains
its original report and seventeen raw-log filenames; execution logs include each
actual shard listing/output and a verified aggregate summary. Authenticated
release archive validation checks these raw proofs, all sixteen bound receipts,
and the exact eighteen unchanged Job-free disclosures.

Twelve test-only scenario loops are now fifty-nine independently schedulable
cases (+47), with unchanged assertion sites and inner retry/recovery loops.
`docs/ci-scenario-mapping.json` records every old name, tuple, new name and
preserved body digest. The former roundtrip policy now runs ordinary release
compilation/rejection and both inert debug scenarios in parallel, then gates the
same required policy check on both jobs. PR supersession cancels only the same
PR's ordinary CI; main and release runs have separate concurrency identities.

PR-to-main successful-result reuse is deliberately absent: no existing trusted
proof establishes identical merge tree, compiler, dependencies, workflow and
accepted execution provenance. Main still performs the complete accelerated
suite, so a previous PR pass cannot falsely certify another source.

Local verification before the first hosted batch: 2019/2019 frontend tests across
152 files; 178/178 Node policy/runner contracts (including executable PowerShell
wrapper and real libtest arguments); typecheck and frontend build; pinned Rust
formatting; D12/D13/D14/D17 standalone tests (49/30/11/5 passed); PowerShell Windows
CI entry contracts (25 passed); all workflow actionlint checks. The independent
review found an incomplete ConPTY runtime bundle; the corrected regression deletes
the entire original build directory before restoring and executing eight shards.
The final review has no remaining actionable findings.

The local npm install needed temporary official-registry URLs because this
execution environment cannot fetch the lockfile's two mirror hosts. Package
versions and integrity hashes were retained, and the checked-in lockfile was
restored byte-for-byte; no dependency or registry policy change is part of this PR.
The first Windows host run completed successfully; timings below bind its exact
source and attempt. Signed Release timing requires a separately authorized future
release; this optimization branch never dispatches one.

## First hosted measurement and tuning

Draft PR [#35](https://github.com/shawnwu2022/cc-desk/pull/35) first ran head
`40a2f47df024854aebbfaa03563f692a6894e831` as PR merge source
`36190369078a18c50cc9947ff080eda66867848d`, run
[37902592210](https://github.com/shawnwu2022/cc-desk/actions/runs/37902592210),
attempt 1. All ordinary CI jobs and the strict eight-receipt `Rust checks`
aggregate succeeded. Compared with baseline run 37894886145:

| Measurement | Prior warm CI | First optimization, cold namespaces |
| --- | ---: | ---: |
| Workflow start to final job completion | 50:31 | 24:27 |
| Sum of all job elapsed time (runner minutes; not rounded billing) | 68.00 | 94.68 |
| Rust compile/static checks | Included in 50:28 Rust job | 10:10 |
| Longest Windows test shard | Unsharded library execution 44:54 | 13:51 |
| Frontend job | 2:35 | 2:23 |
| Complete roundtrip policy critical path | 14:57 | 13:24, including aggregate |

Wall time fell about 52%, but cold runner consumption rose about 39%. The
source profile/cache namespace changed, so this is explicitly a warm-before /
cold-after comparison, not a claim of equal cache conditions. The compiler
bundle was 125,939,224 ZIP bytes; all eight runners reused that exact same-run
payload without installing Rust or compiling it again. The final 168,881-byte
coverage ZIP has SHA-256
`dbd83c1b40c565c07776d48b2d464df49b487603a9fdc70b8e429058469e7dec`.
Its authenticated aggregate checked complete discovered inventory and unchanged
ignore/exclusion policies. Local artifact transfer currently returns HTTP 403;
the follow-up exposes exact per-harness counts and timings in readable job logs
as well as retaining the original archive.

Seven of eight additional triggered workflows succeeded. Cold job-span wall
times: D12 1:23, D13 1:51, D14 1:41, D17 1:50, visual 4:01, real Claude 16:27,
and Windows package/ConPTY integration 21:51. Recovery diagnostics failed because
its original exact selector still named the outer DurableReadmission test.
The corrected command explicitly selects both preserved 035 scenarios through
libtest and requires exactly two tests. No selector is removed.

The first 831-second tail was shard 3. Original raw libtest warning/finish
timestamps identify the retained-custody test at about 311 seconds in baseline;
reconstructing the complete 1,246-name baseline selection and applying the exact
scenario mapping placed it on shard 3 together with restore scenarios. The
initial placement heuristic matched top-level module names but undervalued the
real nested `version_history::windows::context::bundle_restore` and custody
namespace. The correction weights those actual names and separately weights
observed heavy intact cases. This changes assignment only; exact inventory
selection and final fail-closed reconciliation are unchanged. A behavioral
regression test failed on the old weighting and passes on the correction.

The follow-up also removes generated plans/results/executable bundles before
Rust cache post-save, clears any restored old bundle before new planning, and
prints harness start/end, durations, results and slow-case warnings. Raw
source/run/attempt evidence is recorded in `docs/ci-runtime-evidence.json`.
Ten minutes remained unmet on this first cold run. The completed controls and
final sixteen-shard measurement below record the observed improvement.


## Eight-shard warm control and final placement experiment

The second source was head `edcfd281877dde6c5012a09360ffc6cf117fde5d`, tested
merge `d51cba93b6876fedaf3c4e08d1df1bc1ba0e7d8c`, run
[37906391876](https://github.com/shawnwu2022/cc-desk/actions/runs/37906391876),
attempt 1. Complete ordinary CI and its exact eight-receipt coverage aggregate
succeeded in **18:57**, with **90.07 runner minutes**. The compiler/static-check
job took **4:02**, restoring a true cache hit (682,047,390 bytes) in 22 seconds;
complete no-run compilation was 98 seconds, Clippy 35 seconds and the actual
application loader build/check 40 seconds. The complete roundtrip policy took
7:44. The longest test shard remained **14:28**, now shard 0.

The actual aggregate has library **1258 passed, 0 failed, 32 original ignored,
18 filtered-out Job-free names** from a full 1308-name library inventory, and
bin/integration inventories 6/8/5. Across all four harnesses it has **1274
passed, 0 failed, 35 original ignored**; the same eighteen Job-free tests remain
unverified. All raw proof and same-source/run/attempt checks passed. This is a
successful complete measured control, not evidence for a ten-minute result.

The final experiment uses sixteen existing standard Windows runners, with a
single shared cardinality in the compiler planner and strict receipt validator.
All sixteen distinct bound receipts remain mandatory. It also extracts the
remaining retained-custody 002 outer loop's four independent cases, preserving
all statements/assertions and exact argument tuples. The original fixture roots
are independent and fault guards remain thread-local. The pinned-base body and
inventory conservation check now covers all 59 scenarios; the original 55 are
unchanged. The case split raises total passing tests by three, without changing
the 35 ignores or eighteen exclusions.

## Final measured configuration

Keep sixteen shards. Head `82dd9a9636895a8936d503df223d69e76e0b89a9`, tested
merge `cfe016174db2fe00b0ab01e1acd1372cca50a397`, run
[37908893484](https://github.com/shawnwu2022/cc-desk/actions/runs/37908893484),
attempt 1 completed all 22 jobs successfully. Workflow start 09:04:10 UTC to
last required job completion 09:16:32 UTC is **12:22**; API completion metadata
updated one second later (**12:23**). Summed job elapsed time is **87.52 runner
minutes** (5,251 seconds), not a rounded billing estimate. The true warm cache
hit restored 682,047,390 bytes. Compiler/static checks took **4:07**; the longest
shard took **7:16**. Shards 8 and 14 waited about 2:13 and 2:25 after the matrix
became runnable, and shard 8 determined the final tail.

| Ordinary CI configuration | Wall time | Runner minutes | Cache |
| --- | ---: | ---: | --- |
| Original complete suite, run 37894886145 | 50:31 | 68.00 | Warm, old namespace |
| Eight shards, run 37902592210 | 24:27 | 94.68 | Cold, new profile/namespaces |
| Eight shards, run 37906391876 | 18:57 | 90.07 | Warm |
| Final sixteen shards, run 37908893484 | 12:22 | 87.52 | Warm |

Final wall time is 75.5% shorter than the original measured complete suite and
34.7% shorter than the warm eight-shard control. Runner time is 2.8% lower than
the eight-shard control, but 28.7% higher than the original unsharded run.
Parallelism buys latency with existing standard runners; it is not a claim of
lower total compute than the original configuration.

The strict aggregate validated all sixteen bound receipts and their raw logs:
library inventory 1,311, selected 1,293, **1,261 passed / 0 failed / 32 original
ignored / 18 filtered-out Job-free names**; bin 6 passed, paste transport 8
passed, real-paste harness 2 passed and 3 original ignored. Total **1,277 passed,
0 failed, 35 original ignored, 18 unverified**. The 47 additional passing test
names are the independently extracted original scenarios. No assertion, fault
tuple, original ignore, or unavailable-specialist scope was removed. The
coverage artifact is `11605619583`, 168,627 bytes, SHA-256
`ac156bfbb565506087e9b840a5d4478f2544c10a9f74e81a4701e59968b87490`.
Its source/run/attempt and plan hash are recorded in `ci-runtime-evidence.json`.
Authenticated aggregate stdout provides exact counts; local artifact download
returned HTTP 403, so local ZIP inspection is not claimed.

All nine PR-triggered workflows passed. Job spans: D12 1:12, D13 0:52, D14 0:46,
D17 1:17, visual 2:43, focused recovery 6:33, Windows package/ConPTY 9:56 and
real Claude 11:17. The split roundtrip policy finished its required aggregate
8:48 after workflow start. All nine workflows together used **134.65 runner
minutes** (8,079 seconds). This measures the triggered PR set; official signed
Release, qualified native acceptance and manual diagnostics were not executed
on this branch and have no new timing claim.

The ten-minute target is still unmet. Compile plus the longest durability-heavy
shard already exceeds ten minutes before queueing, and real Claude acceptance
also exceeds ten minutes. Real FlushFileBuffers/readback, copy/hash, journal
verification, registry durability, ACL/TOCTOU guards, shared UI resource leases
and original time assertions remain intact. Do not increase shards or launch
more benchmark batches from this result. A later production-safe algorithmic
change or changed hosted runner capacity needs separate evidence.

The subsequent release-fix validation, head `4ca36892`, tested merge
`57acb8daa063402f640e9e8f10e9cdd38674ab23`, run
[37912313809](https://github.com/shawnwu2022/cc-desk/actions/runs/37912313809),
**failed**. All sixteen shards ran; shard 11 failed one selected original test,
`RestartLifetime_TerminalBeforeReopen_002` (85 passed, 1 failed, 4 ignored).
The required Rust aggregate correctly refused the failed receipt; all eight
other workflows passed. This is not an additional successful performance
result. Original failure artifact 11607626389 is 9,148 bytes, SHA-256
`a8964f38ade699fbb5097021b7d8d5c6dda8791226ae29ec76c2769500776e73`.

The verified raw failure showed inner worker exit 0 and owned Job count 1 at
the first query, followed by count 0 at the 0/5/10 ms diagnostic observations.
The test-only confinement helper previously rejected the first nonzero count
immediately and used subsequent queries only for diagnostics. It now waits on
the same retained owned Job handle for a successful empty observation, bounded
by five seconds and the remaining original 300-second worker window. A failed
worker, query error, persistent member, exhausted budget or late zero still
fails. There is no worker retry, named-Job reopening, skipped original test or
production confinement change. Five deterministic drain regressions plus all
six original diagnostic-policy tests execute through the real Rust helper in
Frontend checks; the actual Windows original case remains required in shards.

## Bounded shard diagnosis

PR41 run `38034381816` stalled in shards 7/10/11/12 after execution began; active job log download returned BlobNotFound. This is incomplete verification, not passing coverage or performance evidence. Startup lock-contention probes use `LOCKFILE_FAIL_IMMEDIATELY`; the controlled worker has a 60-second release deadline and terminal observations have finite waits. Source inspection alone has not established the stalled test or a production deadlock.

Shard execution now retains streamed stdout/stderr with advisory test-progress diagnostics, including unfinished assigned names, and enforces a 20-minute bound per harness. Exceeding the bound terminates only the disposable test harness process tree and produces an incomplete, nonzero receipt with partial raw logs; a printed success summary cannot override timeout. The shard job has a 30-minute outer bound. These bounds prevent the default six-hour wait and leave evidence for diagnosis; they do not demonstrate the ten-minute performance target, alter individual assertions, add exclusions, or certify native historical installation.

## Ordinary installer token admission and hosted coverage

The raw artifacts from `38036293902` confirm that all four new ordinary native success probes failed with `PermissionDenied: recovery requires an unelevated x64 user`. Their libtest summaries finished in 100.92–314.17 seconds; disposable suspended children then retained output pipes until the watchdog. This was a real admission rejection plus a fixture cleanup defect, not a 20-minute test body.

Ordinary preparation now spends its own one-shot attempt and validates the exact process token/image before resume intent and irreversible job disarm. Known admission denial leaves the original armed job and permits exact cancellation. Later unknown native outcomes retain existing no-replay/no-user-work-termination semantics. Test fixtures separately retain the exact child handle with bounded cleanup during panic; this authority never enters production recovery.

The existing host probe additionally observes actual token elevation. The original 18 external-Job tests retain their independent classification. Four ordinary success integrations (`020`, `022`, `024`, `025` in `process::cancel_tests`) are independently disclosed as unverified on an elevated host; they remain unchanged and execute naturally on an unelevated host. This additional scope is explicitly authorized for accurate hosted coverage, is never counted as passing, and remains a future native integration task without requiring user-run validation. Three new mandatory contracts (`027`–`029`) cover denied-admission state ordering, actual host token admission/refusal, and panic cleanup.

The compiler job reuses its exact test executable to run these three contracts first, plus the four success probes on an actual unelevated host, under a two-minute watchdog. It retains original panic/exit diagnostics and the raw log. Failure blocks the original full inventory; passing preflight never substitutes for that inventory or its strict same-source aggregation.

## Post-0.18.5 producer/static split and bounded measurement

This focused batch is based on released main
`ec857bd2ab54c0723c460774b95ac525ca8b2061`. Its successful baseline ordinary CI
was run `38042584078`: 11m29 wall time and 91.167 summed runner minutes. These
are baseline measurements, not measurements of this patch.

The `rust-compile` producer now finishes after the original four-harness test
compilation, full unfiltered doctests, immutable inventory/plan, original ordinary
admission/cleanup preflight and same-SHA/run/attempt bundle upload. An independent
Windows `rust-static` job retains the original executable PowerShell contracts,
formatting, strict all-target Clippy and real application loader/ConPTY probe.
The existing required `Rust checks` name requires producer, static and the entire
sixteen-shard matrix to succeed before any coverage aggregation. Failure, skipped,
cancelled or absent results are rejected by the executable strict gate. The
frontend and roundtrip required names and release admission policy are unchanged.

Both jobs use Rust 1.98.1, Node 22, Windows 2022, the existing debug0/test0
overrides, default features and dependency-only cache. Explicit
`shared-key: rust-compile` preserves the former producer namespace: the cache
action uses the shared key instead of the otherwise identical job ID. OS,
architecture, installed-toolchain, Cargo manifest/lock/config and environment
identity remain in the action's key. Static restores that compatible cache but
does not race the producer to save it. Source-bound bundles, preflight results,
compiler inventory, coverage and metrics are removed before producer cache save.
No workspace-crate, release-profile or application-code caching policy is added.

Sanitized `ci-build-metrics-<job>-<SHA>-<run>-<attempt>` archives record pinned
toolchain/runner/profile/config identity, exact cache-match observation, measured
cache-action surrounding wall interval, Cargo command exit/duration and actual
compiler-artifact fresh/rebuilt counts. Counts are retained per crate target and
feature set, including CC Desk, Tauri, Windows and SQLite when emitted. Cargo
test-compilation duration surrounds the existing Windows Compile entry, including
its read-only host/token and inventory checks; it is labelled with this scope.
build-script messages are separately counted; this is not a rebuilt-crate count.
Raw Cargo paths, rendered diagnostic bodies and build-script environment values
are excluded from the metrics archive. Non-exact cache output cannot distinguish
a fallback restore from a miss; byte size, fallback identity and save duration
must be read from the original cache log/final job step timestamps. Missing data
is explicitly null. An exact cache match never proves compilation reuse or speedup.

Shard execution starts one shared 25-minute budget in its first runner step,
before checkout, Node setup, host probe and artifact download. Each later harness
gets at most the smaller of the original 20-minute cap and the remaining shared
budget. An exhausted budget cannot begin another test body. This leaves a nominal
five-minute interval before the existing 30-minute job deadline for termination,
reporting and upload; runner time before the first step, infrastructure failure,
slow synchronous setup or a hard job kill can still consume that interval. It is
not a guaranteed upload deadline or a performance gain. Atomic incomplete receipts
exist before any body and are refreshed between harnesses; timeout, interruption,
missing receipts and partial outer output continue to fail strict aggregation.
Artifact preservation uses `always()` to attempt upload on cancellation too.

`timing-observations.jsonl` retains source/plan-bound harness timing and assigned
test-name progress/completion observation timestamps without assertion contents.
Parallel libtest output does not establish a trustworthy per-test start or
duration. These observations are explicitly labelled accordingly and never used
to change weights, selection or acceptance. Real per-name calibration through
the original exact libtest entry is the next bounded experiment, followed by
deterministic duration-weighted placement with unchanged complete inventory.

Historical three-run estimates put removed static/loader work at 51–80 seconds
of producer delay. Added static setup/recompilation and metrics upload may increase
runner consumption and affect this estimate. Hosted final-source warm controls
are required before claiming savings or p50/p95 progress toward ordinary CI
at most ten minutes. This patch does not change the Windows package/release
critical path or establish a complete-PR or signed-release ten-minute result.

The workflow, strict job gate and observational metrics source are included in
the immutable plan's source hashes. Original test source, scenario mapping,
four harness identities, default ignored reasons, 18 external-Job and four
elevated-host disclosures, original exact selectors, production/release profiles
and all released assets remain unchanged.

## Local frontend iteration groups (2026-10-11)

`vitest.config.ts` now splits the ordinary inventory into `fast-node`, `fast-dom`,
`process` and `stress`. Pure Node and real-process files use explicit lists;
other ordinary tests default to the DOM project. Visual fixtures retain their
independent config. `npm run test:fast`, `test:process` and `test:stress` select
the corresponding groups; `npm test`, `test:all` and `test:ci` still run all four.
Independent Node policy, Rust, browser visual and actual platform evidence are
not included in the name `test:all`.

This changes the iteration scope and avoids jsdom startup for Node-only files.
File isolation, original assertions, real subprocess/HTTP execution and the
5,000-session / 30-owner / 120-change stress workload remain intact. The discovery
guard uses one actual `vitest list --filesOnly --json` execution to check the
complete inventory, uniqueness and script selectors, rather than spawning a
separate list process for every selector.

Local Windows comparisons used the same test names. The 30-file, 219-test Node
subset took 9.01 seconds under the previous jsdom config and 5.35 seconds under
Node. The complete 161-file fast subset took 45.22 versus 45.96 seconds, so this
measurement does not show a total fast-group wall-time gain. The old full suite
took 58.79 seconds and the grouped suite 55.72 seconds in separate local runs;
these single samples are not a hosted CI improvement estimate.

The grouped full run retained all 2,426 original test names and statuses, plus
two new guard cases: 2,428 total, 2,422 passed, four existing observer reporter
failures and two pending cases. The reporter failures and separate Windows Node
policy portability failures were not removed or reclassified to obtain a pass.
No new hosted CI, Rust or real-CLI run was performed for this grouping change.
The hosted Rust shard and roundtrip build paths described above still need
measurements before claiming an ordinary CI critical-path reduction.

Agent execution rules and the command index are maintained in the canonical
[AGENTS.md](../AGENTS.md); tool entry files import or point to it as described in
[agent-guides.md](agent-guides.md). Detailed iteration and full-evidence guidance
is in the [test guide](测试编写原则.md). Changes to this grouping must update those
documents and the READMEs without adding separate tool-specific rule copies.

## Local runtime projection and portable fixtures (2026-10-11)

The next batch changes `useUnifiedWorkspaceRuntime` only at the catalog refresh
boundary. Each Legacy/Native owner has its own reactive watch. Changes enqueue
the normalized project path once per Vue tick and call the existing scoped
`catalog.refresh(projectPath)`. Moving an owner refreshes both paths; replacement,
removal and unmount stop the former watch. Attention updates and removals use the
exact Legacy PTY's project. History and archive changes still use a full refresh.
Existing per-project/full request versions, original adapter projection and all
launch/stop/admission checks remain responsible for correctness.

Four behavioral regressions cover unrelated row identity/open order, owner
move/replacement/removal/addition, attention acknowledgement and unmount. The
unrelated-row test failed against the original global refresh before the change.
Existing adapters and session operations are not replaced with mock projections.

An isolated Windows run before and after used the same `test:stress` command,
both original test names, 5,000 sessions, 30 owners, 120 changes and all original
assertions. Vitest case durations, including each case's initialization, were:

| Original case | Before | After |
| --- | --- | --- |
| `Stress_50Projects100Sessions_001` | 2.065 s | 1.673 s |
| `Stress_30Owners120Changes_002` | 19.780 s | 3.349 s |

These are one local before/after pair, not a per-event timing or hosted CI
critical-path estimate. JSON case reports were retained as local analysis
artifacts. No shard weights or required checks are adjusted using these numbers.

Portable fixture repairs retain the real execution checks: workflow text matching
normalizes CRLF only in the test, Node child scripts use `fileURLToPath`, and the
real rustc/libtest fixture has `.exe` on Windows. The executable Rust result gate
now proves an accepted success and then the precise rejection for every original
invalid result; a missing script no longer masquerades as a rejected job result.
The reporter fixture converts individual Windows backslashes and runs Bash with
`--noprofile --norc`, preserving the actual production script, pipe/body bounds,
strict silent output and no-secret assertions. Six Node policy failures and four
reporter failures were reproduced before repair; targeted runs are now 17/17 and
5/5 passed. The production reporter, CI workflow, Rust inventory and source-byte
hashing are unchanged.

The full follow-up run retained all 2,428 prior cases, including duplicate-name
occurrences, and added only the four catalog regressions: 176 files, 2,432 cases,
2,430 passed, zero failed and the original two pending. The original four
reporter failures are now passed. All 19 Node policy files ran: 212 cases,
206 passed, zero failed and six unchanged Windows skips for Unix executable
fixtures. Typecheck, production frontend build and `git diff --check` passed.
The full Vitest and Node runs overlapped, so their wall times are not an isolated
performance comparison. No new hosted CI or real-CLI validation is claimed.

## Local Rust calibration boundary (2026-10-11)

The follow-up rebuilt the current library harness with Rust 1.98.1, locked
dependencies and the CI's `DEV_DEBUG=0` / `TEST_DEBUG=0` profiles, after verifying
the pinned ConPTY. Cargo was run from `src-tauri` so its repository configuration
applied. Cargo JSON identified the actual executable; its hash, 266 tracked
Rust/manifest/lock source hashes and all 1,465 library names were recorded. This
is a local diagnostic subset, not the ordinary four-harness coverage report.

The ordinary PowerShell probe reported `inJob=true`, `elevated=false`. Four
non-Job-free exact cases first failed opening their default temporary fixture
root with `RelativeOpenFailure(3221225506)` / `STATUS_ACCESS_DENIED`, before the
test logic. Those short failures are not timing samples. Pointing only the
owned child processes' `TEMP`/`TMP` at a disposable workspace directory allowed
real execution without ACL/token changes, alternate implementations or removed
assertions. Raw initialization failures and the second run were both retained.

Each exact case was listed and count-checked first, then run serially from the
same executable with a local 300-second measurement bound:

| Original case | Outcome | Local wall time |
| --- | --- | --- |
| `HistoryContext_FreshAbsentUdf_032` | 1 passed; original assertions | 104.960 s |
| `HistoryContext_ReturnMissingImage_036` | 1 passed; original assertions | 118.789 s |
| `BundleReturn_RestorePresentImage_002` | Timeout; no completed summary | 300-second bound |
| `BundleReturn_FreshCopyMissing_003` | 1 passed; original assertions | 24.873 s |

The three completed runs each reported exactly one pass, zero failures/ignores
and 1,464 filtered names. The timeout retains partial raw output and is neither
a pass nor a completed duration sample. The local bound does not alter the
20-minute CI harness deadline. The generic 120 weight spans materially different
scenarios; these single local samples and one timeout do not establish hosted
weights, concurrent shard wall time or resource contention. Shard weights,
partitioning, required checks and original exclusions remain unchanged. A valid
update still needs completed same-source runner measurements and a full hosted
comparison, including scheduling and artifact restore costs.

During compilation another workspace operation advanced HEAD from `a8006a6` to
`8748317`, changing only the test guide; Rust/vendor inputs were unchanged. The
calibration metadata records `8748317` plus dirty files and actual source hashes.
Later fixture-cleanup changes are a separate source/test inventory and are not
mixed into this measurement record.

## Fixture cleanup and format follow-up (2026-10-11)

Two successful context probes still left their disposable trees. The context
`Fixture` declared `TempDir` before its held Windows directories, so automatic
field destruction attempted cleanup while those handles still pinned the tree.
`HistoryContext_FixtureCleanup_041` reproduced the retained directory before the
fix. Moving only that field and initializer to the end made the regression and
two original capture/copy cases pass. No `Drop` implementation is added, preserving
the existing tests that move individual fields out. Production sharing, ACLs,
directory identity and cleanup authority are unchanged.

Rust 1.98.1 `cargo fmt --check` also found existing layout differences in `pty.rs`,
`store.rs`, `tests/native_cli_registry.rs` and `tests/store.rs`. Only its exact
format hunks were applied. The final locked/offline compile produced all four
original harness identities. The library inventory retained all 1,465 names and
added only the cleanup regression (1,466 total). A count-checked exact subset of
21 cleanup/capture/copy, reader, shell validation, session-ID, registry and project
metadata tests passed; `cargo fmt --check` and diff checks passed. This subset is
not a full Rust execution or hosted coverage qualification.

The calibration and RED fixture residue was copied to the local diagnostic
artifact directory; all 448 file lengths and SHA-256 hashes were checked before
removing only the owned `src-tauri/target/ci-calibration-temp` root. Logs and byte
diagnostics remain available. These copies do not preserve native object identity
or prove recovery/CI coverage. No user installation, application state, Cargo
cache or unknown process was cleaned.

## Audit regression gate closure (2026-10-12)

The D17 production-source crate reproduced `E0432` after the input stager's
unreachable 16 MiB aggregate constant was removed. Its original 64 KiB chunk and
8 MiB upload assertions remain. Behavioral checks now reject both 8 MiB + 1 and
16 MiB uploads, and two added commit-fence regressions preserve partial receipts
and forbid conflicting sequence replacement. All nine standalone tests and
strict Clippy passed locally.

The D14 production-source crate reproduced `E0432`/`E0433` because two inline
staleness tests required Tauri callback types absent from the portable crate.
Those two tests now live in the existing native transport test file and retain
real `OutputRoutes`/`Channel` delivery; all eight native transport tests passed.
The portable crate keeps its original eleven tests and adds the same
stalled-ACK/healthy-flow behaviors against the actual transport source, including
exact generation/stream refusal. Its thirteen tests and strict Clippy passed
locally. These results do not substitute for the three-OS hosted matrices, the
full native inventory or real CLI/WebView acceptance.

## Completed restore probe and strict Rust checks (2026-10-11)

The next local batch completed the original
`BundleReturn_RestorePresentImage_002` with the CI's 20-minute harness bound:
379.408 seconds, exactly one pass, zero failures/ignores and 1,465 filtered
names. The earlier 300-second timeout remains an incomplete failed measurement.
Fixture journal generations and process CPU continued advancing during the
long run; this proves progress, not a particular function's CPU cost. The
existing installation workload, assertions, ACL/identity checks and Job policy
were retained. Only child `TEMP`/`TMP` pointed at owned workspace fixtures.

A same-source experiment used Cargo's package-specific test profile override,
without persisting a repository setting:

```text
cargo +1.98.1 test --locked --offline --no-run --config 'profile.test.package.cc-desk.opt-level=1' --message-format=json
```

Cargo JSON retained `debug_assertions=true`, `overflow_checks=true` and
`debuginfo=0` for all four test harnesses. The same exact restore case passed in
353.762 seconds, about 6.8% below the single default-profile sample. Rebuilding
the application package took 159.423 seconds with cached dependencies; the
1.53-second cached default compile is not a fresh-build comparator. This pair
does not establish a net CI gain, and the override was not adopted. Ordinary
profiles, shard weights, required checks and exclusions remain unchanged.
See Cargo's [profile overrides](https://doc.rust-lang.org/cargo/reference/profiles.html#overrides).

The strict all-target Clippy gate then reproduced 22 errors: sixteen test-name
warnings, four literal `format!` uses, one unused test import and one production
item block after a test module. Six test modules now have narrowly scoped
`non_snake_case` allowances to retain the required original test names. The
other changes remove the unused import, use `String::from` for literals and move
two unchanged validation helpers before the test module. `-D warnings` remains
enabled; runtime logic and assertions are unchanged.

On the final source, locked/offline compilation produced all four original
harnesses with the ordinary profile. The library's complete raw inventory was
byte-identical across default, experimental and final builds: 1,466 names.
Thirty-six exact document/environment/registry/input/transport/supervisor cases
passed, with executable and source hashes stable during execution. Strict
Clippy, formatting, typecheck and diff checks passed. The actual non-test
`cc-desk.exe` from that final Cargo invocation also passed `--check-conpty`
(`ok=true`, `backend=bundled`, `ptyLifecycle=true`). These are local checks,
not full four-harness execution, hosted qualification or real CLI certification.
Metadata records `b6e4001` plus the dirty Rust files and actual input hashes;
the timing pair precedes the lint cleanup and is kept separate from its tests.
After execution, all three uniquely owned diagnostic roots contained zero files
and were removed after path/ownership checks. Raw logs and metadata were retained;
Cargo caches, user state and installation recovery data were not cleaned.

Read-only metadata from historical successful run
[38062158367](https://github.com/shawnwu2022/cc-desk/actions/runs/38062158367)
reported a 223-second producer, a 189-second static job, sixteen shard jobs
spanning 100–379 seconds, and two roundtrip jobs taking 378/444 seconds. The
producer-start to Rust-aggregate-completion span was 10:24, including scheduling
and artifact transfer gaps. These are job intervals, not calibrated per-name
weights. The API reports head SHA `abcfd4d`, while artifact names declare tested
source `b0f5c3c`; downloading the build-metrics artifact returned HTTP 401, so its
actual bytes and source binding were not verified in this batch. No same-source
hosted run was dispatched for the current worktree. Completed runner evidence
and a full hosted comparison remain prerequisites for changing shard weights.

## Same-job debug frontend reuse and Node source loading (2026-10-11)

Read-only metadata from successful main run
[38062961552](https://github.com/shawnwu2022/cc-desk/actions/runs/38062961552)
declares source `2e74e854b088727388aff75d0351b608d74121b3`.
Frontend tests took 105 seconds, Node policy tests 25 seconds and the frontend
build 5 seconds. The ordinary roundtrip release build step took 457 seconds;
the complete debug job took 373 seconds, including 133 seconds for its two
scenario builds and strict Clippy. Rust shard jobs 5, 4 and 7 took 443, 367 and
348 seconds. These are historical job/step intervals, not current-worktree
measurements or per-name execution weights. The metrics artifact download
again returned HTTP 401; its actual bytes and source binding remain unverified.

The debug scenario loop previously invoked the same frontend/ConPTY preparation
hook twice. It now keeps the first ordinary Tauri build. Only after that build
succeeds and `dist/index.html` exists can the second scenario use a uniquely
owned temporary JSON config with `build.beforeBuildCommand=null`. Tauri's
[build config merge](https://v2.tauri.app/reference/cli/#build) applies that
single override. Both Rust applications are still built and hashed, strict
all-target Clippy still runs, the fixture must stay disabled, and its exact
original bytes are restored on success or failure. The override file is removed
in `finally`. No frontend crosses source, checkout or job boundaries; ordinary
release, feature denial, required checks and original coverage are unchanged.

An executable regression extracts the actual workflow PowerShell step and runs
it in an owned temporary workspace with spaces in its path and stale seeded
frontend files. The original step failed because it built the frontend twice.
The changed step passes success, first-build failure, missing-frontend,
second-build failure and Clippy failure cases. Native builders are controlled
substitutes in this regression; it does not certify an application build.
Separately, the actual installed Tauri CLI 2.10.1 was run against an owned toy
project with the locked Tauri 2.10.3 dependency and a deliberately missing
Cargo runner. Default / null-hook override / default calls observed hook counts
1 / 1 / 2 and all reached the expected runner refusal. This proves the installed
CLI's hook/config behavior, not successful Rust compilation or native acceptance.

The full Node policy inventory then exposed sixteen source-loader failures:
Vite's newly used `import.meta.env.DEV` remained in CommonJS output. The paste
generator and trace tests now share an AST compiler that applies only the known
production `DEV=false` constant, as the installed Vite production configuration
does. It preserves same-spelled string literals, payload bytes, actual clipboard
policy imports and refusal of unknown environment flags. Trace tests isolate
the host clipboard API with a throwing boundary; they still execute the real
paste and policy functions. The dedicated paste workflow path filters now also
include the shared compiler and clipboard policy source.

Local checks on `ae448e1` plus the dirty worktree with Node 24.18.0 passed all
nineteen CI Node policy files: 209 passed, zero failed and the six existing
Windows skips. Hosted CI remains on Node 22 and is independently unrun here.
The production-constant regression was first retained as a failing run. The
paste/policy Vitest subset passed all 63 cases. Selected workflow, test and
production input hashes were stable during the final Node run. Logs and actual
input hashes are retained with the batch-five artifacts; no tests, assertions,
workloads, exclusions, Rust profiles or shard weights were reduced.
Typecheck and Node syntax/diff checks also passed on the final local checks.
The current canonical agent guides were rechecked with the actual Codex prompt
assembler, Claude's loopback-only context capture and Gemini's installed loader;
the existing Claude ancestor-import trust limitation remains documented in
[agent-guides.md](agent-guides.md).

## Follow-up TODO (2026-10-11)

This is the canonical performance follow-up list. Checked items mean the local
implementation and scoped evidence recorded above are complete; they do not
declare the current dirty worktree fully tested or hosted savings proven.
The worktree has not been pushed and no hosted run was dispatched in these batches.

- [x] Split frontend iteration into fast, process and stress groups without
  dropping the complete inventory; scope runtime projection refresh by project
  and make affected fixtures portable. Preserve each batch's source and evidence.
- [x] Use `AGENTS.md` as the common standard with tool-specific references.
  Record actual Codex, Claude Code and Gemini loader checks and their scope/trust
  limitations in [agent-guides.md](agent-guides.md).
- [x] Reuse the frontend after the first successful same-job debug build and
  repair production Node source loading. Retain both native builds and original
  gates; the latest local Node policy, paste/policy and type checks are above.
- [ ] **P1 / CI-1 — Verify actual metrics artifacts.** Using an authorized
  authenticated read, download the historical metrics and raw logs, then verify
  their bytes, source SHA, run/attempt and plan/executable identities. Resolve
  the API/artifact source discrepancy recorded above before using that run.
  Completion requires a retained identity ledger and raw files; API step times
  or HTTP 401 responses cannot close this item or supply per-test weights. If
  historical bytes remain inaccessible, keep those runs metadata-only and use
  newly verified source-bound artifacts for subsequent decisions.
- [ ] **P1 / CI-2 — Measure the hosted control/change comparison.** After the
  reviewed source is integrated through the authorized Git/CI workflow, execute
  the original nineteen Node policy files on CI's Node 22 and the complete
  ordinary required gates, including all four Rust harnesses and sixteen shards.
  Compare controls with matching application/test inputs, inventory, toolchain
  and runner class; record each workflow variant's own exact SHA/run/attempt
  separately. Include cold/warm cache state, actual fresh/rebuilt compilation,
  scheduling, transfer, runner consumption and the complete critical path.
  Completion requires source-bound raw coverage and comparable measurements;
  local hook counts alone do not establish net wall-time savings. Current commit,
  push and remote dispatch remain outside these completed local batches.
- [ ] **P2 / CI-3 — Calibrate Rust names on the target runner.** Run the original
  exact entries serially from the producer-identified executable with unchanged
  workload/assertions. Retain full inventory, executable/source/toolchain hashes,
  host conditions, raw completion counts and completed wall times. Initialization
  failures, timeouts and parallel output timestamps stay excluded from duration
  samples. Keep the original eighteen Job-free disclosures and timeout policy.
  This collection can proceed independently of historical artifact recovery.
  Completion requires completed source-bound runner samples for the proposed
  placement; the 379.408-second local restore sample is supporting evidence only.
- [ ] **P2 / CI-4 — Decide on further shard/profile changes.** Only after CI-1
  through CI-3 close, evaluate deterministic placement or a test-profile override.
  Include added compilation and artifact costs, and compare the full hosted path
  with unchanged names, ignores, counts and required checks. Adopt a change only
  when comparable evidence proves a net gain; otherwise retain current settings
  and record the no-change decision. The single local profile pair is insufficient.
- [ ] **P2 / ACCEPTANCE — Close independent target gaps.** Track real Claude
  Code/Codex CLI D20, Windows rendering/installed-package checks and external
  Job-free execution in their original ledgers; see [roadmap.md](roadmap.md).
  Verify remaining agent UI integrations against [agent-guides.md](agent-guides.md)
  when their clients are available. Completion requires actual target evidence;
  these checks cannot be replaced by successful performance or loader tests.

## Production CSP startup gate (2026-10-12)

Required `Frontend checks` retain the complete Vitest inventory, the nineteen
Node policy files and the ordinary `npm run build`. They now also install the
locked Playwright Chromium and system dependencies with
`npx playwright install --with-deps chromium`, then run `npm run test:production`.
This independent browser gate performs a second normal production Vite build,
serves the actual `tauri.conf.json` production CSP and verifies both languages,
Projects and lazy Settings with zero CSP violations. It uses no visual fixture
entry or relaxed policy; its failing IPC boundary does not grant Native authority
or establish installed WebView2/real-CLI acceptance.

Chromium/dependency installation and the second build are added CI work, not a
claimed saving. Startup screenshots and failure traces retain the exact
source/run/attempt artifact name. The original Rust shards, required aggregate,
native exclusions and signed protected-main promotion gates remain unchanged.
`npm run typecheck` first builds Node composite declarations into ignored
`.cache/typecheck-node`, then checks the application and tests; cold checkouts
cannot substitute a preexisting local declaration cache for this step.

## Image-fence fixture preparation (2026-10-22)

PR #70 CI `39696755451/1` (head `cc9fc67`, tested merge `bd6f5f1`)
retains a failed shard-14 receipt: `HistoryWindows_FenceAliases_021` reached
`InitialExclusiveOpenBusy` before acquiring the third case's initial fence.
The first two cases completed their safety probes. The holder is unknown;
the error does not establish an antivirus or product-runtime defect.

Only that fixture preparation now uses the existing `wait_for_preparation`
with a 30-second deadline and the same frozen parent/name/identity/digest.
Only the production-typed, effect-free initial exclusive-open conflict can
wait; identity/content drift and all other errors fail immediately. Timeout
remains failure. No post-acquisition verification, execution, read/write,
alias, unlink or rename assertion is retried, skipped or relaxed. Production
sharing, ACLs, acquisition behavior and CI exclusions are unchanged.

Real NTFS regression `HistoryWindows_FencePreparationBusy_037` holds a reader,
then releases it and checks exact admission or refusal of digest/identity
drift. `_038` rejects an expired deadline before calling acquisition; `_039`
keeps the reader held and requires timeout without changing the file. Local
restricted-token NTAPI `ACCESS_DENIED` is a host limitation, not a pass; the
normal-permission targeted run and the next exact-source CI remain separate
evidence from the original failed receipt.
WSK-MSI's normal-permission Rust 1.95.0 run executed all six `HistoryWindows_Fence`
tests successfully (112.07 seconds, zero ignored), including all four original
alias cases. This local result does not substitute for pinned Rust 1.98.1 CI.

## Supervisor retirement observation (2026-10-22)

Main CI `39700647833/1`, source `f15df6e`, retains a shard-13 failure in
`D15_Supervisor_ProcessExitEofAndParsedAckAreIndependent_001`: immediately after
observing process `Exited`, the test expected the registry resource to be absent
but observed its still-owned `RoutedResource`. Production performs real reap,
lifecycle publication and registry convergence as separate steps; existing
`D15_StopDuringRegistry_AutoRetries_016` deliberately tests the intermediate
state. No failed process wait or fabricated completion was observed here.

The original test now deterministically pauses only its own registry steps,
observes real process exit and successful maintenance admission, then proves
that the resource is still owned in that window. A local RAII guard releases
the pause even on assertion failure. After release it requires exact
`RUN_NOT_READY` within 15 seconds, before sending any renderer ACK. Unexpected
errors and timeout fail; tail bytes, EOF, parsed-offset ACK and final-state
assertions remain unchanged. The controlled schedule reproduced the same
`RoutedResource` failure before this wait was added. This is test observation
coordination, with no production lifecycle, admission or ownership change.

The first RC4 local module run retained a second observation race in
`D15_Supervisor_AttachFailureAfterSpawnStillStopsAndReaps_008`: 26 passed,
one failed because the registry still reported `Running` immediately after
the process snapshot reported `Exited`. That path, pre-adoption shutdown,
and the explicit-stop/wait-retry/shutdown paths now wait at most
15 seconds for exact `RUN_NOT_READY` and then still require registry
`Exited`. Unexpected errors fail immediately; all original output, reap,
no-replay and final registry assertions are retained. Shutdown itself has a
five-second bound rather than an unconditional registry-completion promise;
its tests use the same exact convergence check. Tests deliberately observing
an incomplete registry step are unchanged.

After those observation changes, WSK-MSI ran all 27 supervisor-module tests
under the normal task permissions: 27 passed, zero failed/ignored, 7.96
seconds. This includes real Node/ConPTY owned children and the controlled
registry pause; it is not a logged-in model turn or native WebView result.

## OpenCode HTTP fixture accepted-socket mode (2026-10-22)

PR #72 CI `39715268295/1`, head `5814d8b` and tested merge `6e4b550`,
retains a shard-13 failure in
`Opencode_RenameHttpChecksIdentityAndNeverReplaysPatch_096`: the fixture
worker first received Windows `10035/WouldBlock` at its request read; the
worker join panic was secondary. That slice recorded 99 passed, one failed
and three original ignores. Its raw archive SHA-256 is
`ba5886ceeeece429a8d406d354b5234075f052bb94ff5c2b1ba752ee9eb44322`.
The RC4 supervisor exit/EOF/ACK regression passed in this same slice.

A real, isolated WSK-MSI TCP probe reproduced that a socket accepted from
a nonblocking listener remains nonblocking even after setting a read timeout.
Restoring blocking mode then waited for deliberately delayed bytes with the
same timeout. This is an OS/fixture diagnostic, not official CLI or WebView
acceptance. The updater HTTP fixture already uses this Winsock pattern.

Only the two test-server accepted sockets now restore blocking before their
existing bounded reads: the rename server and the creation redirect server,
whose second accept can inherit nonblocking mode. The latter is a confirmed
control-flow risk, not a second failure observed in this log. The listener
deadlines, read timeouts, extra-POST observation window, identity/title
checks, exact PATCH/POST counts and redirect controls remain unchanged.
Production HTTP retries, timeouts, authorization and path checks are unchanged.

The accepted-socket property inheritance is also specified by
[Microsoft accept documentation](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept).

The corrected WSK-MSI target module executed seven tests successfully
(2.21 seconds, zero failures or ignores), including the original HTTP rename
regression and the legacy/production redirect and creation-timeout controls.
These loopback HTTP and owned-shim results do not certify account-backed
official CLI turns or installed WebView acceptance.

Strict `cargo clippy --locked --lib --tests -- -D warnings` and formatting
also passed for the corrected local source. Fresh hosted CI remains required;
no result from the failed head is promoted.

### Delayed redirect control (next-version local work)

The current fixture had a separate observation defect: after its first 307 response
it slept 500ms and performed only one nonblocking accept. A legitimate control
following the redirect after that point could fail before its own request deadline.
`Opencode_CreationFixtureAcceptsDelayedRedirect_098` uses actual loopback sockets
and reqwest's redirect callback to delay that second POST by 650ms within a three-second
request budget. Before the change it failed with one accepted request instead of two.

Only the fixture now polls until its two-second redirect observation deadline.
The original no-replay case retains its exact one-POST and unknown-outcome assertions,
with a longer negative observation window; production redirects/timeouts/retries and
authorization are unchanged. All eight current OpenCode preparation tests passed
locally (2.22 seconds, no failures/ignores). This is isolated HTTP/shim evidence,
not a model-backed CLI or installed WebView result. The independent body-timeout
coverage candidate remains separate; it is not declared a product defect.
Formatting and `cargo clippy --locked --lib --tests -- -D warnings` also passed
for this local fixture revision; no hosted result is inferred.
