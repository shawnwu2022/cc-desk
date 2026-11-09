# 010 explicit private protected source

Base head: `8a9cb8c485614c6dedcf7cc9b0eea6a83a06c883`. The architecture checkpoint
in stage17 was explicitly discussed and the coordinator authorized approach A.
The original hosted failures remain retained; arbitrary legacy ACL byte recovery
has not been solved or represented as passing.

Only 010 and new 013 use `Fixture::new_private_project`. The default constructor
still takes the original filesystem path for 009/012 and all other SDK experiments.
Before creating child directories, transcript originals or old DELETE handles,
the private mode uses existing production `PrivateDirectory::create_new` and
`CurrentUser::descriptor(true)` to establish a current-user-only protected OI/CI
DACL at FILE_CREATE. The production verifier checks its owner and private ACL.
Creation leases are explicitly released before continuing; no extra lease alters
the original writer/identity-guard sequence or TempDir cleanup ordering.

New test `ClaudeDelete_PrivateSourceHasVerifiedOwnerAndProtectedAcl_013` first
used the old default constructor and failed the actual production private-source
verifier (`PermissionDenied: recovery object owner or ACL differs`): **RED 0/1**.
After the creation implementation, the production open-existing/verify checks,
original/child existence and current-user creation check passed: **GREEN 1/1**.

Original 010 retains both classic and Ex escape assertions, source-name absence,
outside original presence, replacement refusal, identity lease, release ordering,
strict saved-DACL restoration and post-restoration creation. It additionally
checks creation before the deny. This proves the same old-handle escape from a
more restricted explicit private source. It does not model or fix every possible
host legacy ACL, and it does not enable Claude production deletion.

Actual local verification, Rust 1.98.1 and DEV/TEST debug=0:

```
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_PrivateSourceHasVerifiedOwnerAndProtectedAcl_013 -- --exact --nocapture
1 passed
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_OldDeleteHandleCanMoveOutsideProtectedSource_010 -- --exact --nocapture
1 passed; classic and Ex both moved the original and refused a replacement
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target claude_delete_guards::ClaudeDelete_Fixture -- --nocapture
3 passed: unchanged-default 009/012 and comparison-negative 011
cargo clippy --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target --all-targets -- -D warnings
cargo fmt --manifest-path tests/native-cli/scope-core/Cargo.toml -- --check
Both passed.
```

Independent read-only review accepted creation-time permissions, verified owner,
lease lifetime, constructor isolation and unchanged dangerous-operation assertions.
No production permission or release policy changed. All tests use task-created
synthetic sources and installed fixed SDK bytes; no real user ACL/config/session
or pending stage11 process is touched. New hosted 010 and full current-source
CI are required before main integration; the later exact-main/signature gates
still govern publication.
