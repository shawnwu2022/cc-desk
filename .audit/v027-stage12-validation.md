# 0.27 stage 12: frozen audit checkout integrity

Base: `1a1a777fd1b1b4167f23acbcdf806a55dacf0116`.

The two frozen audit indexes were tracked without an end-of-line attribute.
Real `git -c core.autocrlf=true cat-file --filters HEAD:<path>` reproduced a
checkout-byte mismatch before the fix (two regression tests failed):

| Index | Frozen SHA256 | Converted SHA256 |
| --- | --- | --- |
| stage10 | `57f0eef28dd18fe6c5fefead518da1a745cb96b751c2cfc56c85d75466afa835` | `6ae075e90a75c2f6effd387bafc093dfffd130b844c22e0f694231c7328b318d` |
| stage11 | `07842b7170eef162b2259bc36918d48eca9d302a8c5b93620b95d1783fa9d0d1` | `60b26701c37ed5e46d0f5f2e2d692d884d927bd0592b6b092d7c7e8c653cfa73` |

Only these two exact paths now use `-text`. Neither frozen index nor its
referenced evidence has changed. The regressions are in the existing CI Node
policy entry point and compare actual Git filtered bytes with the fixed hashes
for `core.autocrlf=true`, `false` and `input`; external filters are rejected.

Validation:

```
node --test tests/scripts/historySignatureFixtures.node.mjs tests/scripts/historyPayloadFixture.node.mjs
```

Result: 14 passed, 0 failed. Independent read-only review found no blocking issue.
This static check neither observes nor releases the pending stage11 instance.
Stage11 document IPC / visible WebView acceptance remains blocked and is not
reclassified by this fix. No real CLI/session access, installation, version
bump, main merge, push, tag or release was performed for this validation.

## Codex compressed-history rename compatibility

Official upstream: Codex `rust-v0.167.0`, annotated tag
`cb41599ccbe371272892f156286a236aa097bada`, peeled commit
`086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a`.

- [Stored metadata reports the plain rollout path, lines 425-445](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/thread-store/src/local/read_thread.rs#L425-L445).
- [Upstream plain-before-compressed resolution](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/rollout/src/compression/path_metadata.rs).
- [Explicit gateway login opt-in, lines 155-166](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server/src/request_processors/initialize_processor.rs#L155-L166).

Desk's production `check_thread` returned `SESSION_NOT_FOUND` for a synthetic
compressed original when `thread/read` named its missing logical `.jsonl`.
The new regression reproduced this before implementation (1 failed, 0 passed).
An initial invocation used an incomplete exact test name and selected zero tests;
that invocation is not acceptance evidence. The corrected exact name was:
`registry::tests::NativeRename_CodexCompressedLogicalPathRetainsAuthority_046`.

The fix attempts only the exact `.jsonl.zst` sibling after `Ok(None)`, with the
original held Root and the same Budget. All other errors and a present plain file
retain their existing behavior. Header ID/cwd, source identity, revocation and
bounded decompression remain required. Permanent-delete admission explicitly
keeps the strict path policy and does not enable this rename fallback.

The synthetic negative cases cover foreign root, wrong suffix, present invalid
plain file, plain directory, mismatched compressed ID/cwd, invalid compression,
oversized decompressed metadata, compressed directory, both files missing and
revoked scope. They use only task-created temporary files.

The existing exchange test separately reproduced a missing
`explicitGatewayOauth` (`Null != true`). Initialization now opts into explicit
login, consistent with the already verified stage10 delete exchange. It adds no
login, account, model or turn request. This is a compatibility/privacy correction,
not evidence of the user's actual failure cause.

Commands use `RUSTUP_TOOLCHAIN=1.98.1-x86_64-pc-windows-msvc`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`:

```
cargo test --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target NativeRename_ -- --nocapture
cargo clippy --locked --offline --manifest-path tests/native-cli/scope-core/Cargo.toml --target-dir src-tauri/target --all-targets -- -D warnings
cargo fmt --manifest-path tests/native-cli/scope-core/Cargo.toml -- --check
```

Only the independent `d12_scope_core-98d6c87465292b80.exe` is linked/run; no
`cc_desk` host executable, WebView, real CLI or pending-instance observation is
part of this check. The initial compressed-path-only run passed 18 tests and
strict Clippy. The final initialization-inclusive run also passed 18 tests, strict
Clippy, formatting and diff checks. Independent review accepted both changes. Full original
16-shard application validation and real CLI rename were not rerun.
