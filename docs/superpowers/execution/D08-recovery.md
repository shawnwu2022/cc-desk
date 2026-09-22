# D08 interruption recovery

## Verified checkpoint

Remote PR #17 was re-read after the interrupted response. Head was already `0e70cc4d19af39c2744b548857bbf2fb0c2f973c`; do not repeat the ref update or recreate the branch.
CI #152 (`35723268022`), Rust job `106730827888`, tested merge `5b72b74e7c5802d66b55cb99a8961fa8297b9925`:

- All ten environment behavior tests passed.
- Eight snapshot/discovery tests failed against the deliberate scaffold; 373 passed / 8 failed / 14 ignored in the library. Each failure was read before implementing the snapshot.
- Formatting failed; Clippy identified one unnecessary clone in the discovery test. These are tracked separately from behavioral RED.
- Frontend job succeeded. No complete-task claim was made.

## Implementation boundary

This recovery implements immutable launch input values and filesystem-only discovery. The selected executable path is preserved (not silently replaced by a canonical target); neither a path snapshot nor availability certifies executable bytes, a working login, a validated shell dialect, or PTY compatibility. Availability checks the same selected program/runner and reports host health separately.

Legacy defaultCustomArgs is preserved as opaque backend-only text when inherited. D09/D10 must migrate it explicitly rather than silently splitting or dropping it. Raw argv suppresses Desk default arguments, skip-permission injection, and extra observer fields, while preserving explicit profile environment preferences.

CallerIdentity is an internal value, not an IPC payload; D11 must construct it from the backend's current window/epoch registry. D08 alone does not establish lifecycle authorization. Neither global legacy startup checks nor the existing PTY launch path is switched by this commit.

## Recovery implementation verification

CI #153 (`35743904013`), head `45b716c28610bc10ff627fa1e6018f4c47849f85`, tested merge `edf5bed810d905711658d414e0b614f67ac42ac8`:

- Rust library: 381 passed / 0 failed / 14 ignored, including all ten environment and eight snapshot/discovery cases. Main: 6 passed; launch-config: 2 passed / 3 ignored; transport: 8 passed. Zero doc-tests add no coverage.
- Frontend job succeeded (typecheck, Vitest, Node policy and production build).
- Clippy strict checking passed. Only rustfmt failed, for one method-chain layout in snapshot.rs. The follow-up applies that exact whitespace-only change, without altering tests or behavior.
- Final review for this bounded recovery: author self-review, no independent subagent. Selected-path preservation, raw/default separation and secret-safe snapshot formatting were checked. Remaining D08 service, backend ownership integration, Unix execution and adversarial coverage are not certified by this recovery.

Status: D08 IN_PROGRESS. Follow-up CI must be checked against its exact head before claiming full green. Per-profile availability service/IPC integration, complete launch routing, additional adversarial cases and user-machine/native-CLI acceptance remain pending. Preserve this checkpoint rather than recreating the implementation.
