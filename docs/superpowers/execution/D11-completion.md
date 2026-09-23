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
