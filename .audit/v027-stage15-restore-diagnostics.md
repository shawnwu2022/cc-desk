# Remaining hosted 010 failure: diagnostic-only checkpoint

Base head: `ce65ead838d01bee7d2d779dc39e8838d16b0824`.
[D12 run 42461126466](https://github.com/shawnwu2022/cc-desk/actions/runs/42461126466),
Windows job `128670443575`: **261 passed, 1 failed**.

The new explicit-grant restoration test 012 passed on the hosted Windows runner,
as did 011 and the other fixture tests. Original 010 still proved the classic
dangerous move and replacement refusal, then failed in `restore_fixture_deny` at
line 274. The original logs do not distinguish the native restoration call from
the later exact comparison. Its Ex branch was not reached. Both prior source
failures remain blocking; the full-DACL correction is not claimed to resolve 010.

This change only adds bounded test-fixture failure receipts:

- Native restoration failure: fixed stage, numeric Win32 result and requested
  information bits; descriptor preflight failure has a separate fixed stage.
- Strict-comparison failure: descriptor lengths/control bits and at most 16 ACEs
  with types/flags/access masks. SID and complete ACE bytes are hashed; raw SID,
  SDDL, path, environment and user data are not logged.

Every slice is checked against a validated bounded descriptor; output is finite.
The restoration API, its arguments, return values and all strict assertions are
unchanged. This is evidence gathering before another semantic change, not a third
unproven fix, permission expansion or test exclusion.

Using fixed Rust 1.98.1 and DEV/TEST debug=0:

```
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_Fixture -- --nocapture
cargo clippy --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target --all-targets -- -D warnings
```

Result: 009/011/012 **3 passed**, strict Clippy passed; formatting was applied.
011's existing negative cases exercise the diagnostic summaries without changing
their required failure outcomes. Independent read-only review accepted bounds,
privacy and unchanged decision logic. Original 010's local pass belongs to its
earlier recorded source; it is not substituted for the still-failing hosted case.

New hosted evidence is required to decide whether the remaining boundary is the
native call or the resulting descriptor. No retry of the denied stage11 process
observations, real ACL/session changes, main merge or release is performed.
