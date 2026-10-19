# Native terminal last-row regression

## Cause and fix

`NativeCliTerminal` previously placed 6px padding on its absolutely positioned
border-box host. Locked xterm 5.5.0 / FitAddon 0.11.0 reads that host's computed
height but subtracts only the `.xterm` element's padding. It therefore measured
12px more height than the host's content area. The unified terminal host clips
overflow, so a row admitted across the next cell boundary could be partially
hidden. The [upstream fit algorithm](https://github.com/xtermjs/xterm.js/blob/6.0.0/addons/addon-fit/src/FitAddon.ts#L74-L96)
and the installed package source were checked independently.

The 6px inset now belongs to `.xterm`, where FitAddon accounts for it. The host
has no padding. Rendering and Native PTY resize continue using the exact fitted
rows and columns. No manual row decrement or runtime/observer/history change is
involved.

## Reproduction and regression

```sh
npm run test:terminal-fit
```

Install the project's Playwright Chromium beforehand, or set
`PLAYWRIGHT_CHROMIUM_EXECUTABLE` to an existing Chromium/Chrome/Edge executable.
The independent test configuration starts a loopback Vite server in the same
Node process and closes it after the run. It refuses a busy port. This avoids
Windows sandbox process-tree enumeration during cleanup.

The fixture directly mounts production `NativeCliTerminal.vue` with production
global CSS and the locked real xterm/FitAddon. Only external launch, terminal
transport and Tauri host boundaries are synthetic; no actual process, user
configuration or installed application is accessed. It uses no theme-preview
substitute and never invokes `fitVisible` from the test to hide lifecycle faults.
The fixture is not imported by the normal app or production build.

Six browser tests cover DPR 1/1.25/1.5, eleven viewport heights crossing cell
boundaries for three font-size/line-height configurations, window width/height
changes, and metric changes while hidden followed by showing the same terminal.
They place a visible marker on the last row, check its lower edge and the screen's
right edge against the clipped host, compare actual launch/resize arguments with
the displayed xterm rows/columns, and require exactly one launch. The last DOM
row must contain the marker, and another complete row must not fit; a fixed
one-row decrement cannot satisfy that capacity assertion. The fixture explicitly
scrolls to the bottom after writing, since repeated shrinking/growing can retain
an older scrollback viewport. Screenshots and
geometry attachments are retained under `test-results/terminal-fit/`.

## WSK-MSI evidence, 2026-10-19

Base: `316279818c57671e4f82eb41e7d051dc946f97ae`, branch `fix/v0.23.3`.
Browser: installed Windows Chrome 155.0.8059.84, controlled by Playwright 1.63.0.
Before the CSS fix the real browser regression failed at every required DPR:

| DPR | Host bottom | Last-row bottom | Rows sent/displayed |
| --- | ---: | ---: | ---: |
| 1 | 469px | 474px | 28 |
| 1.25 | 460px | 462.71875px | 26 |
| 1.5 | 525px | 525.6875px | 30 |

These values were collected after the production ResizeObserver debounce had
sent new dimensions. RED error contexts are retained locally in
`.cache/v0233-verification/red/`. After moving the padding, the six-test browser
matrix passed. The targeted appearance/paste/parser/inventory group passed all
98 tests, and typecheck passed.
The separate visual-fixture/workflow/product-boundary group passed all 60 tests.
An independent read-only review found no blocking issue, inspected the three
enhanced DPR screenshots and independently repeated the installer PE comparison.

A final combined-worktree debug NSIS test bundle was also built with updater artifacts disabled;
it retains version 0.23.2 because this work does not authorize a version bump.
The installer was not run. Its outer PE contains one icon group whose seven
frames exactly match the approved ICO. Test installer SHA-256:
`8d6f626ea757f558636a4a7d923a1ed47d9fdbd56cd44948facc68328096db88`.

Browser results certify the real component layout and frontend resize boundary,
not actual ConPTY delivery, live Claude/Codex TUI output, Windows WebView2,
installed-app rendering or actual Windows display scaling. Those independent
platform/CLI checks remain unverified. No running user sessions were terminated,
and no application was installed or replaced.
