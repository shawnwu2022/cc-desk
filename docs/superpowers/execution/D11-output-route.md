# D11 output-channel ownership

Approved plan: 2026-09-22 native CLI v3, W2/D11; baseline 74d7e05f018544f86ad880ea06ab86755ae416f3, PR #19. D11 remains IN_PROGRESS.

## Scope and rulings

- Continue the existing remote feature branch, not main. GitHub write actions are available. Container git access fails DNS and has no Rust toolchain; Rust/complete frontend validation use existing CI. Supplied v3 archive was read after indexed Files retrieval returned no results.
- Ruling: preserve LaunchRequest raw bytes; carry the serialized JS Channel descriptor in a separate x-cc-desk-output-channel header. This is transport metadata, never caller identity and never part of replay content. Construct the actual Channel only in the reservation winner's connect closure. A replay must not close or replace the retained route.
- Ruling: use a generic typed OutputRoute<T> until D13 defines the event envelope. This avoids inventing a second RunEvent schema. D11 owns callback leases, authority checks and cleanup, not reader credit, ACK, drain, input sequencing or process-stop policy.
- Ruling: Channel send success is host dispatch acceptance, not proof that a renderer callback or xterm parser consumed the bytes. Native callback evidence is tested separately. No zero-copy or bounded-RSS claim.
- Callback registration has a finite per-document capacity; duplicates cannot alias active routes. Constructor work and destruction occur outside the callback-table lock. Revocation and failed sends prevent future sends, without retrying or releasing process ownership.
- Original native document tests and the loader smoke remain enabled. This increment does not enable an incomplete product startup/terminal path.

## RED stage

Ten Rust route/descriptor tests and four JS channel-header tests call the production scaffold/bridge. The route scaffold returns explicit NOT_IMPLEMENTED errors. Observe actual behavioral failures before implementation; compile/format failures are separate. Native channel factory/coordinator integration and real WebView checks follow.
