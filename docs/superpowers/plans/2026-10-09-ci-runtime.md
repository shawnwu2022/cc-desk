# CI Runtime Implementation Plan

**Goal:** Reduce daily and complete verification latency toward ten minutes with standard public runners while retaining exact test and release provenance.

**Architecture:** Compile the four Windows test harnesses once. Partition the discovered full-name inventory deterministically across runners, execute exact assigned names, and aggregate only after raw inventories and outer results reconcile. Split test-only scenario loops to remove indivisible critical paths. Cache independent validation crates and overlap release candidate builds with full same-source CI preflight.

**Constraints:** Base db517757eefd323b3eaa4c69224bdd8f74b7feb6; isolated perf/ci-runtime only; draft PR; no main/tag/publication changes, paid runners, protection changes, added exclusions, weakened assertions, or production durability changes. Existing eighteen Job-free tests stay explicitly unverified on contained hosted runners. Frontend, roundtrip policy, doctests, loader, formatting, lint and all original harnesses remain required.

- [x] Add failing inventory partition/aggregate tests: new names included, duplicate/missing shard/name rejected, source/run/attempt/compiler binding rejected, native failure preserved, original ignores retained.
- [x] Implement scripts/windows-rust-shards.mjs and runner, single compile artifact and full-name plan; update Windows orchestration and raw coverage validation without weakening release archive checks.
- [x] Split independent long Rust scenario loops into explicit schedulable tests with exact scenario mapping.
- [x] Reshape CI into compile, parallel shards and existing Rust checks aggregate; no required workflow-level path filters; bounded artifacts and phase timing summaries.
- [x] Optimize remaining workflows with caches, scoped concurrency, compression settings, source-gated parallel release candidate build. Preserve final release preflight and same-platform signing.
- [x] Run local Node/PowerShell contract tests, complete frontend suite/typecheck/build, Rust formatting and standalone core validation where supported.
- [ ] Independent full-branch review, normal branch push and Draft PR; collect real GitHub wall clock, runner minutes, cache state and complete inventory evidence. Tune only from measured bottlenecks.

Review focus: exact selected-name union and duplicate execution; ignored workers and eighteen exclusions; cache/artifact contamination across commits or attempts; missing/failed/cancelled shards; release candidate build admission and final provenance revalidation.
