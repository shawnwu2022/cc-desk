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
