# CI runtime work

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
