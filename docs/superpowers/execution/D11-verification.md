# D11 core and recovery verification checkpoint

Source `bb96705e027303a3133265118550bee617e26fb9`, CI #167 (`35813683322`), tested PR merge `91a7d6206e802f7d6ba0e956134cda66fe5b9191`.

Both full job logs were read. Frontend `107030523645`: 51 files / 640 tests passed; typecheck, three Node release-policy tests and production build passed. Windows Rust `107030523966`: 463 library tests passed / zero failed / 15 ignored; main six passed, launch configuration two passed / three ignored, transport eight passed; Clippy passed. All 24 D11 Rust tests and 12 frontend launch-recovery tests passed. Four of the edge cases strengthen already implemented behavior, rather than claiming new RED evidence for those four.

The only failing step was rustfmt: the Indeterminate transition call at run_registry.rs:431 required a multiline layout. This checkpoint changes only that layout and records the observed result; no assertions or behavior are altered. The successor needs its own full CI confirmation, which will be recorded in PR #19 metadata against the exact tested head rather than appending an untested documentation commit.

## Author review and remaining integration

Author self-review, not an independent reviewer. Reviewed replay-before-profile-read and atomic second check; non-cloneable reservation permit; safe panic/failure tombstones; owner/epoch checks before spawn; immediate-exit monotonicity; resource retirement outside the registry lock; bounded streaming request fingerprints; and frontend immutable single-send attempts with instance/revision/schema checks. No claim of live authorization or complete application integration follows from generic-resource and transport-boundary tests.

D11 remains IN_PROGRESS. Continue on feat/native-cli-run-registry with real Tauri document-lifetime provenance, actual child/reader/writer ownership and guarded route rollback, then authenticated start/query IPC and old-route migration. The original request decode budget is still a separate live IPC concern. D14/D15 retain output-drain/stop semantics. No Codex user launch entry, native-agent, WebView, Unix or installer acceptance is delivered by this checkpoint. Do not advance D11's whole-task status merely because the core tests pass.

No main write, merge, publish, version/dependency/lockfile changes or native user configuration mutation. Existing runtime fixtures and ignored user-history/real-CLI cases keep their previous acceptance status.
