# D11 frontend lost-response policy

Twelve frontend tests precede implementation of createLaunchAttempt. The interface takes a validated request, the expected backend instance and an explicitly supplied transport. This is not a live Tauri command or an authentication token; no nonexistent IPC name is invoked and no production UI launch route is switched.

The attempt owns immutable request values, sends start once even under re-entry/concurrency, and uses status-only recovery after a lost response. Missing status or a restarted backend must not create a new ID or respawn. Strict receipts require exact run/request/generation/instance identity, canonical u64 revision, valid phase/failure combinations and no unknown fields. A delayed lower revision cannot replace a newer state; conflicting equal revisions and impossible phase reversals are rejected. Returned state is frozen.

The initial scaffold rejects calls with LAUNCH_ATTEMPT_NOT_IMPLEMENTED. Observe RED before implementation; failures are not ignored or replaced by mock success. These tests exercise the production frontend state machine through its transport boundary, not real WebView authorization or a real agent.
