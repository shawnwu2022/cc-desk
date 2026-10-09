# Desktop updater core tests

This optional local crate compiles the actual pure updater policy and HTTP transport without a GTK/WebKit desktop installation. It includes the same `desktop_updater_policy.rs` and `desktop_updater_http.rs` tests that the complete application runs in the ordinary Windows Rust inventory.

Run `cargo test --manifest-path tests/updater-core/Cargo.toml --locked`. Loopback proxy fixtures use synthetic metadata; the inherited-environment case changes only a child process. Public signature vectors are test-only and never authorize a production update. These tests do not execute an installer or establish native platform acceptance.
