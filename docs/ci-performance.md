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

Eight Windows jobs restore the exact compiler artifact from their own SHA/run/
attempt. Source-file and payload hashes, original checkout path, observed Job
containment, exact assigned libtest listings, native exit codes and final outer
counts must agree. New unclassified tests are included automatically. Original
ignored workers remain ignored. Missing, repeated, cancelled or failed shard
receipts fail the existing `Rust checks` aggregate. The coverage artifact retains
its original report and seventeen raw-log filenames; execution logs include each
actual shard listing/output and a verified aggregate summary. Authenticated
release archive validation checks these raw proofs, all eight bound receipts,
and the exact eighteen unchanged Job-free disclosures.

Eleven test-only scenario loops are now fifty-five independently schedulable
cases (+44), with unchanged assertion sites and inner retry/recovery loops.
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
Windows host execution and the ten-minute target remain unverified until the
actual Draft PR CI run completes. Signed Release timing requires a separately
authorized future release; this optimization branch never dispatches one.
