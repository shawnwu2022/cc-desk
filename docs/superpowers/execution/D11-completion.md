# D11 completion execution

Approved plan: 2026-09-22 native-cli v3 W2/D11. Starting verified source 866eb87f; continuation branch feat/native-cli-run-registry, never main.

## Scope rulings

- Ruling: W2/D11 and contracts C02 govern completion. D11 owns cli_start, cli_get_launch_status, atomic single-start orchestration and caller/run ownership. D14 owns RunEvent/ACK/backpressure; D15 owns terminal_resize/terminal_stop, drain/update/renderer-loss policy; D17 owns input transactions. Earlier checkpoint prose incorrectly called these all D11. This corrects task accounting; it does not claim those downstream capabilities are implemented.
- Ruling: production start must fail before process creation until a backend lifecycle/stream consumer is installed. A caller-supplied readiness boolean is forbidden. Native acceptance will install an isolated real-PTY consumer through the same backend composition seam used by later D14/D15. No unbounded/discarding output pump or premature UI migration is permitted.
- Ruling: retain existing coordinator/registry/OwnedPty/native admission; fill orchestration and resource lifetime, not reimplement launch algorithms. Active legacy PTY is not switched to an incomplete new transport.
- Ruling: revoke idle output owners without killing the process. Native Channel and its authorization closure both retain WebView/manager; a weak document guard alone does not break this cycle. UI revocation must not wait on a sending thread that may itself wait on the native UI.

## Workspace and baseline

Temporary workflow bdef61f exports tracked source (git archive, no .git or credentials), Linux node_modules and formatter for isolated local verification; original runtime cannot resolve github.com. It will be removed before final source verification. Downloaded archive digest verified. Local git initial snapshot is synthetic, never used as a remote parent.

Local baseline: npm run typecheck exit 0; npm run test:ci exit 0, 649 tests / 53 files. Windows verification uses the existing actual CI job. No independent reviewer agent is available; review is author self-review.

## Work ledger

- Native-owner cleanup: five behavior tests and no-op revoke scaffold written before implementation; RED pending.
- Production service / native command composition / API adapter: pending.
- Final full CI and original D11 completion checklist: pending.

## Observed RED / implemented before native command acceptance

- e3402d6, CI #188 / 35841916824 / Windows 107118746931: 521 passed, 4 failed, 17 ignored. Four new lifetime regressions failed after compilation: idle revoke retained native owners, send failure retained guard, concurrent revoke restored sender ownership, revoke during factory accepted the new route. Existing native reports and Clippy passed; RED-test formatting failed separately.
- 831976da, CI #189 / 35842904762 / Windows 107121998362: 527 passed, 11 failed, 17 ignored. The seven new service tests all reached LAUNCH_SERVICE_NOT_IMPLEMENTED; the four lifetime regressions remained. Six extra helper tests are reused runtime cases, not new behavior. Complete logs read before implementing the service. Frontend/Clippy and old native reports passed.
- Local frontend API: four cases failed on the API scaffold, then passed. Adding the attempt factory and public backend-instance metadata produced three further failing assertions (12 existing passed), then all 15 targeted cases passed. Full updated Vitest: 655 / 54 files; typecheck and three Node policy tests passed. A combined command timed out while building frontend after tests passed; build is rerun independently, not claimed green from partial output.
- The former ambient declaration for @tauri-apps/api/core hid the installed official Channel type. Removed only that replacement declaration, retaining other existing shims; no dependency or lockfile update.

Implementation closes a revoked route's native Channel AND admission captures outside locks. Table revocation never waits for a busy sender; that sender retires rather than restoring revoked state. Factory reservation/replay invariants remain. DocumentAuthority now owns/revokes the callback table so page lifecycle releases idle references. A guarded backend RunAccess rechecks caller and run identity at every operation and after waiting for writer/master locks.

LaunchService reuses the coordinator's original route-before-spawn sequence and frozen D08/D09/D10 lowering. Snapshot/PTY ownership is published before external supervisor adoption; handoff failure cannot discard the live process or authorize another spawn. Only the actual insertion/spawn winner calls adoption. Missing backend consumer rejects before profile IO; it is not caller-selectable.

Native application composition, original-main-config preservation and two formal command wrappers are compiled, with deliberate endpoint stubs for real WebView behavior RED. New native test uses the formal commands, one hundred same-request invokes, actual Node PTY, raw callback packets, profile deletion/replay, peer/stale rejection, native destruction, and explicit owned kill/reap. Its test-only consumer caps data at 32 KiB with 128-byte frames; it is not a product event/ACK/backpressure implementation. Closed mode tests default readiness failure before workspace creation. Child self-expires after 45 seconds for failed-parent safety. Temporary workspace-export workflow is removed.
