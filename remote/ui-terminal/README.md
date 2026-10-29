# Remote terminal semantic consumer

Internal E05 trial package, independently testable without an npm installation.
`npm test` uses Node24.18+ built-in runner and TypeScript type stripping. It does
not typecheck; run an installed TypeScript compiler with `--noEmit --strict
--target ES2022 --module NodeNext --moduleResolution NodeNext
--allowImportingTsExtensions --lib ES2023,DOM src/reducer.ts src/renderer.ts`.
The shared wire source and fixture are in `protocol/remote/inner/v1/PROJECTION.md`.

Only the authenticated native record adapter may call `begin` with a verified
Host barrier and pass decoded records. The reducer also checks finite fields,
exact target/binding/stamp/base, cell roles, epochs and ordered chunk coverage.
It never parses VT or authorizes input. `disconnect` clears synchronization and
pending install; old frozen display remains historical. A fresh replacement
requires a new increasing sync epoch. `ack()` is just the exact installed stamp;
Host validation and policy remain mandatory. No target auto-selection or retry.

`render(document, installedFrame, palette?)` creates a detached grid with fixed
numeric positions and textContent, never HTML/URI/clipboard/IPC actions. Import
`src/terminal.css` when mounting the result atomically. Wide cells use Host roles,
not client wcwidth. The optional trusted269-entry palette accepts only #rrggbb.
The trial fallback theme is explicit; exact desktop-theme matching, browser
shaping and accessibility/selection behavior still need real shell acceptance.

`npm run test:native` requires explicit TEST_ONLY artifact path in
`CC_DESK_TEST_ONLY_PROJECTION_JSONL`. Generate it once with the Rust terminal
encoding test of the same tree and that variable; it uses create_new and never
overwrites prior evidence. This verifies actual Rust Core encoder bytes into
the TS reducer, not a phone, network, renderer pixels or an AI CLI session.

No native bridge, enrollment, secret storage or remote control entry is wired.
Per-Host/peer client memory accounting, Host-issued barriers/ACKs and E09 control
authority remain required before the package can enable a product input UI.
