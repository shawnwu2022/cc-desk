# Codex compatibility repairs

## Authorized scope

Continue on `feat/active-session-filter`, base `996a8fa0427f23337c697dab43b7447797e5ec5c`.
Preserve the OSC7501 status repair and active-session filter. Repair the five
findings from the source-backed 52-interface audit. No unrelated OpenCode work,
global CLI configuration, paid model calls, application installation, push,
merge or release. The user authorized a final local commit after verification
and independent review.

Audit evidence: `C:/Users/wusha/AppData/Local/Temp/cc-desk-codex-audit-20261021-100927/REPORT.md`.
Current repair logs: `C:/Users/wusha/AppData/Local/Temp/cc-desk-codex-fixes-20261021/`.

## Implementation boundaries

- F1: optional Codex hook flags must not break the selected original program.
  Admit enhancement only with positive target capability evidence; preserve
  original program/runner/argv/environment and single-spawn semantics when
  enhancement is unavailable. No relaxation of cmd/path quoting.
- F2: the last valid append in `session_index.jsonl` wins, including equal or
  rolled-back timestamps. Malformed trailing records remain ignorable.
- F3: replace the Codex observer's lifetime event ceiling with bounded replay
  handling that continues for new events and cannot revive evicted old identity
  events. Keep Claude causality and run/document authority separate.
- F4: user prompt text cannot prove automatic origin. Structured automatic
  source markers still exclude automatic histories; unknown/manual sources
  stay discoverable even when their text resembles an automation scaffold.
- F5: program-path or profile-revision lifetime caches cannot assert the
  capability of a replacement binary. Scope capability evidence to the actual
  launch attempt; unknown evidence uses existing safe Desk paste.

## Progress

- Initial check: requested branch, exact base, clean worktree. No other branch
  or user change was moved.
- Existing audit reproductions retained. New production-history regressions
  `CodexCompatibility_NameIndexUsesAppendOrder_070` and
  `CodexCompatibility_ManualPromptsNeverProveAutomaticOrigin_071` added before
  implementation; RED execution is pending dependency initialization.
- `prepare-conpty.mjs` downloaded but its PowerShell extraction could not load
  Expand-Archive. Reused the already generated probe runtime, staged only the
  manifest-listed files in the ignored worktree runtime, then ran the normal
  prepare command: `Verified cached ConPTY 1.24.2607.10001`.
- Cargo's normal source failed with Schannel `SEC_E_NO_CREDENTIALS`. The first
  loopback attempt also exposed denied writes in the host Cargo cache. A
  task-local Cargo home and a loopback public crates mirror are being used;
  Node still verifies upstream HTTPS, Cargo still verifies package checksums,
  and no user Cargo configuration or ACL was changed. These setup failures do
  not count as test results.

Final RED/GREEN, regression inventories, live CLI evidence, independent review,
remaining platform gaps and commit will be recorded here before completion.

### Implementation checkpoint (2026-10-21 12:34 UTC)

- Still branch feat/active-session-filter, base HEAD 996a8fa, production changes uncommitted.
- All five scoped production fixes implemented. F1 adds isolated metadata capability admission with original-snapshot fallback; unknown hook trust/schema disables only optional observation. F2 append-order and F4 manual-origin regression both first failed against production parser (history-red-ready.log).
- F3 frontend RED received only 1024/2050 identities; full hook file then GREEN 22. Observer production harness GREEN 47 + 1 existing ignore. Ordered IDs are Desk-owned boot-monotonic replay tokens, not Codex turn sequence. Legacy opaque/Claude replay remains unchanged.
- F5 frontend cache regression first failed; affected frontend files then 43 pass. Attempt freeze restart regressions added; initial fixture forgot the required exited receipt, corrected fixture rather than production guard.
- Scope-core full run: 197 pass / 3 native Claude rename tests return NATIVE_RENAME_UNSUPPORTED. Baseline reproduction pending; do not count full suite as passing. No rename policy was loosened.
- Full Rust Cargo metadata initialization stalled on network index, canceled only this task-owned Cargo PID10420. Seeding task-local public crate/index cache from existing read-only host public cache; no global writes, no ACL/token changes. Other work continues.
- Independent read-only reviewer compatibility_review started. It also checks shared identity fallback concerns from the separate Claude/OpenCode report; no OpenCode edits authorized here.

### Verification checkpoint (2026-10-21 04:47 UTC)

- Earlier checkpoint used host local clock 12:34, not UTC; that timestamp means 04:34 UTC. Tool-clock UTC is authoritative.
- F5 independent review caught cross-attempt pending-Promise reuse; regression078 first RED, then API changed to a fresh invoke per attempt. API2 + terminal22 now pass; unchanged running capability/new attempt refresh both directions verified with IPC fixture.
- Final full Vitest: 198 files, 3057 pass, 2 existing skip (vitest-final.log). Typecheck/build pass; visual unit66 pass. No installed WebView2 or actual clipboard acceptance is implied.
- Actual installed official Node -> codex.js through production freeze/invocation exits0 with hook admission disabled (live-node-fallback.log).
- Native probe was tightened after review: only audited native bytes can be executed for metadata. Unknown/custom/old/future bytes and non-Windows native targets disable enhancement without changing original base launch. No arbitrary-wrapper subprocess probe or reader thread remains. This conservative enhancement limitation is explicit in native-cli-v3.md.
- Initial new probe accidentally capped native hashing at128MiB; the installed official exe is341270320 bytes. The isolated real-CLI regression failed and diagnostic copy showed failure before spawn; fixed by exact audited size followed by full held-file SHA256. Preserve live-capability-final/repeat/probe-diagnostic logs as failures. Corrected actual production module probe passes (live-capability-size-corrected.log), no model turns. Debug hash+metadata took20.26s; optimized release latency has not been measured.
- Full root Rust library suite is running. Multiple unchanged Windows maintenance/Job/security tests fail and real WebView workers are timing out; no ACL/token/Job policy or tests were relaxed. Three Claude rename failures reproduced in exact996a8fa baseline isolated source (baseline-native-rename.log14pass3fail). Other full-suite failure classification remains pending.

### Final gate checkpoint

- Five fixes implemented and targeted regression green. Final production root tests: `Codex` filter57 pass (includes real cmd forwarding/new-resume fallback), invocation21 pass; observer-core48 pass + 1 existing ignore; scope-core HistoryCodex32 pass. Fresh final frontend inventory198files3057pass2skip. Typecheck/build/visual-unit66 pass. Strict Clippy running, no commit yet.
- Independent reviewer accepted final conservative native-probe boundary after finding/fixing pending-Promise reuse and unbounded reader/wrapper risk. API now has no cross-attempt cache. Probe never executes unknown hash; held canonical target/parent protected throughout, no reader thread, wait must succeed. 079 directly checks unknown admission returns None.
- Full Rust run completed:1292pass305fail32ignore,361.58s. Inventory1629 at compilation (before final size gate and reporter-ID test081). Final targeted production tests ran AFTER those last changes and total inventory1630. Full result is not final-source all-green evidence. Parsed305 failures in rust-failures.json:4 Claude native rename,297 Windows maintenance (includes1 real WebView),4 Native WebView. Common maintenance failures are access denied/HRESULT0x80070005/RelativeOpenFailure0xC0000022 or Job prerequisites. Do not claim all305 baseline-proved: only3 rename failures were rerun on exact996a8fa snapshot. No policies/ACL/token/Job guards/assertions were changed to hide them.
- Existing independent Claude candidate remains out of scope: A SessionStart -> B SessionStart -> late A Stop can restore A because eventType is omitted on Claude dispatch. Codex replay changes do not touch that branch; fixture/static evidence is not real-CLI disorder certification. No OpenCode production files changed.

### Local commit preparation

All five targeted fixes and final regression gates are complete. Strict Clippy (`--lib -- -D warnings`) and Rustfmt pass; final Codex57 pass after last changes; typecheck reconfirmed exit0. Independent review accepted the final boundary. Full Rust failures remain disclosed above.

The sandbox rejected creation of the shared Git worktree index.lock when restoring two generated schema files (their normalized Git diff is empty). No Git index mutation or commit occurred. Source edits and all reports are complete; local Git write permission is the remaining operation. Final Git receipt will be recorded in the external REPORT.md rather than rewriting this committed pre-commit snapshot.

### Failure diagnosis follow-up — current progress

Both test executables have compiled: immutable996a8fa and the final staged production source, with separate copied binaries and SHA256 receipts (`diagnostic-artifacts.json`). All301 selected non-Native-WebView failures were rerun by exact test name against both binaries using one frozen environment/argument set. Both301 fail, and all301 normalized reason signatures match (`diagnostic-fast-comparison.json`). All39 panic-source files and11 lower-level production files are byte-identical to baseline. No new patch-caused failure has been found.

The other4 Native WebView failures all reproduced in the exact baseline, with matching bootstrap stages and90-second worker timeouts (`diagnostic-baseline-webviews.log`, four failed in360.25s). All305 therefore have baseline reproduction. Same-scenario actual PTY controls pass3/3 on each binary, and ConPTY integrity/runtime controls pass66/66 each. An ad hoc baseline-module diagnostic locates NTSTATUS0xC0000022 at absolute capability path walking, after ordinary std file write/read and volume-root opening succeeded, before private ACL creation. No security guard was changed. Own-host query reports restrictedToken=true/inJob=true; exact enforcement policy and WebView rendering cause remain unproven.

A final current Native-launch WebView representative is running to check the final binary's pre-page-load failure against the captured baseline. Other five-fix final Codex regressions were rerun on the copied final binary:57 pass. Source snapshot comparison is unchanged. Commit remains explicitly forbidden during this diagnosis; no commit retry occurred. The original27-file staged patch is preserved; this progress appendix is documentation only.

Diagnosis complete: all305 original failures reproduced on exact996a8fa. Strictly paired301 final/baseline failures have identical actual error codes and reasons. All4 baseline Native WebView timeouts match original bootstrap stages; an extra final Native-launch representative paired in one frozen environment/args also times out identically (90.06s each). No new patch-caused regression found; final Codex57, actual PTY3/3 and ConPTY66/66 controls pass. No production changes or commit retry in this diagnosis. Full positive Windows/WebView/private-capability/Claude rename acceptance remains blocked. Detailed receipt: `C:/Users/wusha/AppData/Local/Temp/cc-desk-codex-fixes-20261021/FAILURE-DIAGNOSIS.md`. Original27-file staged patch stays unchanged; this appendix is intentionally not staged during the no-commit diagnosis phase.

### Local commit authorization and final scope check

The user subsequently explicitly authorized committing these27 reviewed files and the verification record locally, without push/publication. Before this retry, branch and HEAD remained feat/active-session-filter /996a8fa; the original staged patch SHA256 still matched2358f4e4d4407f16b1eb7545888dbf910427b901df6d08638f3721ac9648a8d3. The only working-file difference from the reviewed snapshot was this diagnostic ledger appendix. No production/test source changed after verification, no other contributor change was found, and cached/worktree diff checks passed. The completed diagnosis and its full-suite failure limitations above are retained. Final commit hash and clean-status receipt will be recorded in the external REPORT.md.
