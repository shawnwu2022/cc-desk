# D11 owned-resource integration

Base: ac1a2df0b23fbc01514a450aa52098c5f0efefdc, existing PR #19. The approved v3 W2/D11 remains the authority. Previous generic core/recovery is not being reimplemented.

Ruling: integrate route leases and actual PTY ownership before exposing live launch IPC. The current connect callback returns unit and relies on external rollback conventions; exposing it before resource ownership is established risks leaving routes/children unowned. Cost: document-lifetime authorization and the old-route migration remain subsequent D11 work, not silently considered finished.

Pre-flight: D08 snapshot feeds D09 build_invocation and D10 resolve_process unchanged. Routed launch wraps the existing coordinator and stores the route with its resource. The PTY constructor acquires reader/writer before the child spawn, removing fallible I/O setup after a child exists. The output reader must retain its route without retaining the master (which would prevent ConPTY closure). D14/D15 remain responsible for autonomous waiters, credit, EOF/ACK-driven retirement and user-approved stop policy.

Tasks: add six route-ownership tests and six PTY integration tests; observe behavioral RED against explicit scaffolds; implement minimum ownership layer; run full CI and author review. The native probe counts actual child-created files and never starts Claude/Codex, reads their data or calls a model. Windows tests initialize the pinned private ConPTY. Existing native acceptance/ignored tests keep their status.

This commit is the test/interface checkpoint, not a completed implementation. Real document identity is not derivable from a window label alone; no new IPC is registered and no caller-selected epoch is accepted. No main/merge/release/version/dependency/lockfile/native configuration or old PTY changes.

Execution environment: GitHub connected branch is the isolated authoritative workspace. Container git clone failed DNS resolution; local Rust is unavailable. Behavioral Rust verification uses the existing Windows CI, not a claimed local test run. Final exact-head CI evidence will be recorded in PR metadata without an unverified trailing docs commit.
