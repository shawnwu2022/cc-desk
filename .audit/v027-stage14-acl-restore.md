# Synthetic ACL restoration: preserve grants, not only remove denial

Base head: `f8a51257407af481ab4c58dffd2205668947aa0d`.

That source's [full Linux visual run](https://github.com/shawnwu2022/cc-desk/actions/runs/42460137790)
and main CI Frontend checks passed. However,
[D12 run 42460137978](https://github.com/shawnwu2022/cc-desk/actions/runs/42460137978)
failed on Windows-2022 job `128667458967`: **260 passed / 1 failed**.
The macOS and Linux D12 jobs passed. The failing original 010 again reached
`FixtureCreationDeny::restore` after the dangerous native move. The AI-only
comparison correction was insufficient. This actual failure remains failed;
no exclusion, tolerance change or arbitrary descriptor normalization is added.

Microsoft explicitly specifies that [`icacls /deny`](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/icacls)
also removes the same rights from explicit grants. Therefore `/remove:d` is not
its inverse: it can remove the added deny while leaving the original grant
damaged. This is a concrete restoration-algorithm defect even though the prior
hosted logs do not expose their exact original descriptor difference.

New test `ClaudeDelete_FixtureRestorationPreservesExistingExplicitGrants_012`
creates a fresh isolated fixture, adds an explicit Everyone WD/AD grant, applies
the existing fixture denial, proves creation is blocked and then demands exact
original restoration. It reproduced the same restoration assertion locally on
the previous implementation (**RED: 0 passed, 1 failed**, exit 101). No real
directory, session, configuration or permission is involved.

The fixture now parses the saved bounded valid SDDL and uses
[`SetNamedSecurityInfoW`](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow)
to restore its complete DACL and original protected/unprotected state. It does
not set owner, group or SACL, enable privileges or change production permission
logic. Existing current-token DACL authority on the newly created fixture is
used. Inheritable ACEs may propagate to its synthetic child objects, as the API
documents; this is not a claim of zero child-permission side effects.

An owning RAII descriptor keeps the DACL pointer valid until the native call
finishes and frees the allocation once on all subsequent paths. Missing/NULL
DACLs are refused; a valid empty ACL is distinct and denies access. After actual
restoration, the unchanged narrow binary comparison still checks every byte
except the already documented AUTO_INHERITED clear-to-set transition. The 010
dangerous-move, source absence, new-writer refusal, guard ordering and successful
post-restoration creation assertions remain intact. Claude production deletion
stays disabled.

Actual GREEN on Windows using Rust 1.98.1 and DEV/TEST debug=0:

```
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_Fixture -- --nocapture
3 passed: 009, 011, 012

cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_OldDeleteHandleCanMoveOutsideProtectedSource_010 -- --exact --nocapture
1 passed; both classic and Ex moves reproduced; both ACL restoration checks passed

cargo clippy --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target --all-targets -- -D warnings
cargo fmt --manifest-path tests/native-cli/scope-core/Cargo.toml -- --check
Both passed.
```

Independent read-only review accepted RAII lifetime, DACL-only flags/current
authority, retained comparisons and unchanged dangerous-operation assertions.
No frontend suite or complete Rust shard suite was redundantly rerun locally.
The new source still needs fresh required hosted CI, including the Windows D12
fixture and original main suite. This record is not hosted success, main merge,
release authorization beyond the existing scope, or real installed-UI acceptance.
The pending stage11 instance, leases and denied interfaces remain untouched.
