# Hosted restoration difference: avoid requesting new parent inheritance

Base diagnostic source: `3840ed2366012515372dee239c5e4d2d1438543a`.
[D12 run 42461820886](https://github.com/shawnwu2022/cc-desk/actions/runs/42461820886),
Windows job `128672541972`: **261 passed / 1 failed**. Original 010 still failed;
the prior failed attempts are not reclassified as passing.

The bounded native receipts established the remaining boundary: restoration
returned success, but the subsequent descriptor comparison correctly refused
additional ACEs. At 08:13:40Z the original and restored metadata were:

| Field | Original | Restored |
| --- | --- | --- |
| Control | 32772 (`0x8004`) | 33796 (`0x8404`) |
| Descriptor length | 108 | 188 |
| ACE count | 3 | 6 |
| ACE flags | 3, 3, 3 | 3, 3, 3, 19, 19, 19 |
| Access masks | all 2032127 | all 2032127 |

The first three complete ACE hashes remained identical. The last three had the
same SID hashes and masks but included the INHERITED_ACE flag. No native call
failure or metadata-preflight failure was reported. Thus the actual difference
is not merely the AUTO_INHERITED control bit: the previous request explicitly
re-enabled inheritance and introduced three parent ACEs into a legacy DACL.

Microsoft specifies that
[`UNPROTECTED_DACL_SECURITY_INFORMATION`](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-information)
makes a DACL inherit ACEs from the parent. The fixture's `/deny` never changed its
protected state, so restoration now requests only `DACL_SECURITY_INFORMATION`.
It restores the original ACL contents without requesting a new protection or
inheritance transition. The postcondition still checks original control bits,
ACE order/count, masks and SID bytes under the unchanged narrow matcher. Added
ACEs are not accepted as a successful restoration.

Test 012 now explicitly prepares fresh protected and unprotected synthetic
directories (`icacls /inheritance:d` and `/inheritance:e`), and verifies complete
grant restoration in both. Original 010's initialization, native move, denied
replacement, source identity lease, release ordering and restoration assertions
remain unchanged. No production permission logic, owner/group/SACL setting,
privilege change, test exclusion or CI policy change is introduced.

Actual local checks, fixed Rust 1.98.1 and DEV/TEST debug=0:

```
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_Fixture -- --nocapture
3 passed: 009, 011, 012
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_OldDeleteHandleCanMoveOutsideProtectedSource_010 -- --exact --nocapture
1 passed; classic and Ex both reproduced the dangerous move
cargo clippy --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target --all-targets -- -D warnings
cargo fmt --manifest-path tests/native-cli/scope-core/Cargo.toml -- --check
Passed.
```

The final explicit two-state 012 setup was separately checked again: **1 passed**.
Independent read-only review accepted the evidence-to-flag relationship and
unchanged safety boundary. New hosted 010 and complete current-source CI are
still required. This is not a claim of complete 0.27 acceptance or a release.
Claude production deletion remains disabled; the pending stage11 instance and
all denied interfaces remain untouched.
