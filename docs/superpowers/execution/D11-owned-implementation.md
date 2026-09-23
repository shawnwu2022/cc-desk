# D11 owned-route and PTY implementation checkpoint

Base: `faa1eeb4c361d340a53ae6afa6b2015717ac16f9` on existing PR #19 / feat/native-cli-run-registry. This remote test scaffold already existed when work resumed; no duplicate branch or reimplementation of the earlier registry.

## Observed RED

CI #169 (`35816077205`), tested merge `29613cf10741597792f5f988036ecb396317b2ac`, Rust job `107037762221`: the complete log was read before implementation. Compilation and Clippy passed. Library: **472 passed / 9 failed / 15 ignored**. Four route-lifetime and five real-PTY behavior cases failed against explicit scaffolds. Three new guard cases already passed; they are not claimed as new RED evidence. The six included private-ConPTY checks are existing coverage, not six new ownership behaviors. Frontend job passed. The two new test files had separate formatting failures, corrected here without changing assertions.

## Implementation and rulings

- Reuse LaunchCoordinator::start and its non-cloneable reservation; no competing replay registry. A successful connect returns a lease kept outside the coordinator call until its outcome is published. Success also retains it with the process; cancellation, failure and unwinding release it outside registry locks. A partially failing connect must retain its own rollback guard while constructing its lease.
- D08 snapshot -> D09 invocation -> D10 process spec -> OwnedPty. Obtain reader/writer before spawn_command, then retain every successful child and control handle. Selected program, cwd, environment and argv resolution are not changed.
- Reader, writer, child wait, master resize and child termination have independent handle ownership/locks. A blocking write or wait does not hold the registry mutex or the child-killer lock. try_wait does not block behind another waiter; successful wait results are cached.
- The output reader retains only a route Arc, not the master-bearing resource. Root exit remains distinct from route/resource retirement; explicit master closure must not be pinned by a reader awaiting EOF.
- Ruling: OwnedPty is an internal ownership primitive, not an automatic close/kill policy. Its caller must wait/reap before retirement. Autonomous waiters, output credit/ACK and safe drain/stop remain D14/D15. No Drop-based implicit kill or process-name search is introduced. The actual lifecycle owner must be integrated before any new user-facing launch endpoint is enabled.
- No live Tauri authentication is claimed. Document epoch provenance, authenticated IPC admission, route adapter rollback, old controls/startup migration and native acceptance remain D11 integration work. These tests run a disposable Node probe, not Claude/Codex or a model.

## Verification status

Implementation pending exact-head full CI. Local network access to GitHub was checked and failed DNS; no local Rust toolchain is available. GitHub Actions is the executable verification environment. The approved v3 W2 D11 brief was read from the mounted plan ZIP after indexed retrieval returned no result. Existing feature branch remains authoritative.

No main writes, merge, publish, version/dependency/lockfile changes, native configuration mutation or current PTY-route switch. Author review only, not independent review. Final evidence belongs in PR metadata at the exact verified head; do not append an unverified documentation successor after recording GREEN.
