# 0.27 RC gate corrections: retained failures and bounded verification

Base head: `07001406a2d0364a0d2a902ed16ce4dd630d0284`.
Tested PR merge: `31c4b4b3b1f4564e056a5a58b81a1c7dc3a3852f`.
The first-hour release target was not achieved; the observed failures remain
blocking until corrected-source CI passes. No main merge, tag or release occurred.

## Original Rust failure

[CI 42458148017](https://github.com/shawnwu2022/cc-desk/actions/runs/42458148017),
job `128662768058`, shard 15: **112 passed, 1 failed, 2 original ignored**.
[Artifact 13609260813](https://github.com/shawnwu2022/cc-desk/actions/runs/42458148017/artifacts/13609260813).
The original `logs/cc_desk-execution.log` is retained byte-for-byte under
`v027-stage13-shard15/` locally and in the committed lossless
`v027-stage13-original-execution.log.gz`; decompressed SHA256
`0b9791380cdf421b5a3713a8484dd0ea3bf9f2391f0531054d5a1c09a2f62413`.
The original has an extra blank EOF line, rejected by `git diff --cached --check`.
The first commit attempt stopped before creating a commit or pushing. The exact
raw bytes are therefore stored in gzip, with roundtrip digest verified; neither
the raw log nor the whitespace policy was altered to manufacture a passing check.

`ClaudeDelete_OldDeleteHandleCanMoveOutsideProtectedSource_010` already proved
the classic old DELETE handle moved the original outside the source, the source
name disappeared, and creating its replacement was refused. It then failed at
`FixtureCreationDeny::restore`, line 277. No ACL-command-failure output appeared;
the failed comparison was the final saved SDDL equality. The artifact does not
contain before/after SDDL, so an AI-only difference was not directly established.
The failed run did not reach the Ex branch. Claude production deletion stays off.

The test-only matcher now parses both bounded valid SDDL values into descriptors
and reuses the existing production restoration comparison: only
`AUTO_INHERITED` clear-to-set is permitted; every other byte remains exact.
No ACE sorting, SID removal, rights widening, policy change, skip or ignore is
introduced. Invalid, overlong and NUL-containing text is rejected. The API-owned
allocation is copied and freed. See Microsoft's
[automatic inheritance contract](https://learn.microsoft.com/en-us/windows/win32/secauthz/automatic-propagation-of-inheritable-aces)
and [SDDL conversion ownership](https://learn.microsoft.com/en-us/windows/win32/api/sddl/nf-sddl-convertstringsecuritydescriptortosecuritydescriptorw).

The actual same fixture is also compiled by existing Windows `scope-core`, using
the same pinned SDK bytes/hash via a source-relative include. Only that independent
test executable runs; the pending `cc_desk` host is never linked, queried or reused.
This permits verification without bypassing its retained lifecycle boundary.

`ClaudeDelete_FixtureRestorationAllowsOnlyDocumentedInheritanceFlag_011` first
failed with the original exact-string comparator, then passed with descriptor
comparison. Negative cases preserve reverse-transition, rights, SID, ACE flags,
ACE count, protection and malformed-input rejection. The unmodified dangerous
move assertions in 010 passed locally for both classic and Ex, including the
post-guard ACL restoration and new-creation check. This local token is not the
hosted elevated runner: corrected hosted verification is still required.

All commands set Rust `1.98.1-x86_64-pc-windows-msvc`, DEV/TEST debug=0:

```
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_FixtureRestorationAllowsOnlyDocumentedInheritanceFlag_011 -- --exact --nocapture
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_OldDeleteHandleCanMoveOutsideProtectedSource_010 -- --exact --nocapture
cargo clippy --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target --all-targets -- -D warnings
cargo fmt --manifest-path tests/native-cli/scope-core/Cargo.toml -- --check
```

Result: 1/1 + 1/1, strict Clippy/fmt passed. No full suite was rerun locally.

## Original visual failure and pixel review

[Visual run 42458148299](https://github.com/shawnwu2022/cc-desk/actions/runs/42458148299),
job `128661548879`: **3 failures**. One was the archived screenshot (367
Playwright-different pixels); two looked for the removed native-archive button.
[Artifact 13608292920](https://github.com/shawnwu2022/cc-desk/actions/runs/42458148299/artifacts/13608292920)
records Ubuntu 24.04, Node 22.23.4, Playwright 1.63.0, Chrome 153.0.8010.12.
The uploader-reported ZIP digest (not independently rehashed archive bytes) is
`c917fe3d28ae522cdbad2dc8b3625075c39dc152cd204286374513a480057505`.

Root and `cleanup_safety_design_review` each actually opened expected, actual and
diff. The only visible change is the new permanent-delete availability separator
and collapsed heading beneath the three archive rows. Existing rows/icons/main
content remain unchanged. Only this exact Linux actual PNG is adopted:

- Previous tracked HEAD PNG: `f198a94537b494d1bdff2e03a2d6841412005746e8b05d0585ffdcb3513eada8`.
- Adopted actual/candidate: `b44fc17fa12de582abbcdf2dc67cd77d69990e1870f1eefaffc91a5eace2dda7`, 51,904 bytes.
- All other 12 PNG bytes match their reviewed CI candidate copies; no update.

The old manifest top-level entry still named an earlier PNG hash. The adoption
precondition rejected that mismatch without writing; HEAD and working PNG were
then independently confirmed identical. Both old metadata history and the actual
pre-adoption tracked hash are retained. The adoption script records each check;
all 12 other manifest entries and the full prior entry/history compare unchanged.

The visual test now explicitly selects the Claude fixture, checks the real
transcript/subagent directory scope and independent second-confirmation warning,
keeps permanent deletion disabled, and requires the removed archive control to
be absent. A first local attempt incorrectly expected Codex scope for the first
Claude row and failed twice; that evidence remains in
`test-results/v027-stage13-targeted`. The explicit-source corrected run passed:

```
npx playwright test tests/visual/unified-workspace.spec.ts --grep 'diagnostics and conservative cleanup stay reachable' --update-snapshots=none --output=test-results/v027-stage13-corrected
```

Result: **2/2**. Full zero-tolerance Linux CI is still required. No snapshot mask,
pixel tolerance, geometry contract, production CSS or 13-image inventory changed.
Python image-metric probing was unavailable via the system alias; a separate
one-line Node probe had a syntax error. Neither ran a verification nor changed
images. The guarded adoption script subsequently completed successfully.

Independent read-only review accepted the descriptor boundary, unchanged 010
assertions and exact single-image adoption. Superpowers systematic debugging was
used to distinguish the real unsafe-native counterexample from fixture cleanup
and obsolete UI assertions. Stage11 IPC/visible UI and original pending instance
remain unverified/untouched; this browser fixture is not their acceptance.
