# D11 output-channel ownership

Approved plan: 2026-09-22 native CLI v3, W2/D11; baseline 74d7e05f018544f86ad880ea06ab86755ae416f3, PR #19. D11 remains IN_PROGRESS.

## Scope and rulings

- Continue the existing remote feature branch, not main. GitHub write actions are available. Container git access fails DNS and has no Rust toolchain; Rust/complete frontend validation use existing CI. Supplied v3 archive was read after indexed Files retrieval returned no results.
- Ruling: preserve LaunchRequest raw bytes; carry the serialized JS Channel descriptor in a separate x-cc-desk-output-channel header. This is transport metadata, never caller identity and never part of replay content. Construct the actual Channel only in the reservation winner's connect closure. A replay must not close or replace the retained route.
- Ruling: use a generic typed OutputRoute<T> until D13 defines the event envelope. This avoids inventing a second RunEvent schema. D11 owns callback leases, authority checks and cleanup, not reader credit, ACK, drain, input sequencing or process-stop policy.
- Ruling: Channel send success is host dispatch acceptance, not proof that a renderer callback or xterm parser consumed the bytes. Native callback evidence is tested separately. No zero-copy or bounded-RSS claim; authorization cannot retract messages already queued before revocation.
- Callback registration has a finite per-document capacity; duplicates cannot alias active routes. Constructor work and destruction occur outside the callback-table lock. Revocation and failed sends prevent future sends, without retrying or releasing process ownership.
- Original native document tests and the loader smoke remain enabled. This increment does not enable an incomplete product startup/terminal path.

## Observed RED

91d5ed72, CI #183 (35836552849): complete logs read. Frontend job 107101288190: typecheck passed; 645 existing tests passed, all four new header/descriptor cases failed for missing behavior. Policy/build skipped after failure. Windows job 107101287983 compiled successfully, then 506 existing library cases passed, all ten route/descriptor cases failed, 16 existing ignored entries. Existing real WebView reports passed. Clippy passed; new test formatting failed separately. This is behavioral RED, not a compile-only failure.

## Next tested implementation

OutputRoutes now reserves a callback lease before invoking a Channel constructor. Duplicates/capacity reject without constructing or dropping a conflicting native callback. RAII returns the slot on constructor error/panic; authorization is checked before and after construction. OutputRoute serializes each route's sends, rechecks authority, redacts transport errors and makes errors terminal. It never exposes a raw Channel handle.

The bridge's optional third argument serializes only a canonical u32 Channel descriptor into the dedicated header; the payload bytes and old two-argument calls are unchanged. Local Node VM checks after frontend RED passed valid zero/max descriptors, exact Unicode bytes, invalid/throwing descriptors without dispatch and query compatibility. This is not a local Vitest or native WebView run.

A new Windows native Channel test and a deliberately unimplemented channel_native interface are added before implementing that native factory. The isolated application uses the production bridge and document admission, actual transformCallback/channel_on transport, four ordered byte payloads, duplicate binding rejection without closing the original callback, a second native WebView carrying the main proof, and a send after real native destruction. Both worker and parent require every ordered observation and no sticky failure. No Claude/Codex or product startup runs in this test. Observe its real RED before filling the factory.

## Outstanding boundaries

Production endpoint registration/lifecycle ownership and event pump remain unenabled. Tauri 2.10.3 uses an application-wide fetch queue for larger Channel messages; this primitive's small-frame test does not certify large-message queue ownership, cancellation, pending-data cleanup or backpressure. Those must be resolved/tested with D13/D14 before any product stream is enabled. A native Channel retains its WebView/manager; lifecycle retirement must break manager/resource ownership cycles. No broad authentication, complete D11 or installed-product claim is justified here.
