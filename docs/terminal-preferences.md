# Shared terminal preferences (Task 18)

## User-facing settings

The normal Settings Terminal section replaces the Task 17 placeholder. It offers all 16 existing theme presets, a static preview, font family, font size, line height, cursor style, cursor blink and renderer preference. Shared AppButton/AppSelect/AppInput/InlineNotice controls preserve the existing shell and safe save feedback. Numeric drafts retain intermediate input, commit once on blur/Enter and cancel on Escape; font size is clamped to 10–24, line height to 1–2.

The preview is ordinary Vue markup and CSS, with fixed illustrative text and ANSI swatches. It imports no xterm runtime, PTY, native bridge or CLI. It does not claim that a process is running. Preview cursor animation respects reduced motion. Theme cards and controls wrap without a global minimum width; actual Windows scaling/visual acceptance remains unperformed.

Font selection uses existing installed fonts only: the system stack, Cascadia Code, Fira Code, JetBrains Mono, Consolas or Menlo. Platform CJK/emoji fallback stacks are preserved and shared with both runtimes. No font discovery or installation is added.

## Single preference and persistence owner

`app.terminalPreferences` is the same computed object for Legacy Claude and both Native CLI terminal consumers. Its fields are themeId/theme, fontFamily, fontSize, lineHeight, cursorStyle, cursorBlink and renderer. `config/terminalPreferences.ts` supplies normalization and explicit xterm option projection/update helpers; runtime/IO state is absent from that object.

All terminal setters use the existing serialized simple-settings lane introduced by Task 17. Saves stay optimistic and per-field; known failures roll back only the latest field intent to its confirmed baseline. Underlying read-origin sequence and per-field intent/commit ownership apply to all terminal fields. Unknown acknowledgements obtain a fresh read inside the same lane and are never replayed. A failed recovery read blocks subsequent GUI and terminal writes until a new explicit operation can read the saved state.

Compatibility storage keys `terminalTheme`, `fontSize`, `webglRenderer` are retained. New keys are `terminalFontFamily`, `terminalLineHeight`, `terminalCursorStyle`, `terminalCursorBlink`. Existing raw incremental config writing is unchanged and does not round-trip or erase unrelated/future keys. The Rust read DTO adds only optional fields, including the existing-but-previously-unread `webglRenderer` boolean; no bridge or command is added.

All existing theme IDs retain their exact palettes. Missing terminalTheme is inferred from the old stored GUI theme at initial hydration, then stays independent. Recovery reads with the terminal field still absent reuse the confirmed terminal baseline instead of re-inferring from a newer GUI choice. Startup migration checks original field intent, commit and read-publication sequence at actual serialized submission, so a late migration cannot overwrite a later terminal selection or recovery snapshot.

## Live terminal continuity

Both terminal constructors use the same appearance projection. Updates mutate each existing terminal's options:

- Theme/color change: only theme options; no fit, resize, restart, clear, selection change or input
- Cursor style/blink: only cursor options; no fit or resize
- Font family/size/line height: update metric options in place, request one coalesced visible fit; hidden instances retain pending metrics and fit when shown
- Renderer preference: only newly created terminals use the new selection; the UI states this explicitly

Legacy no longer has a separate font-size prop source. Its pending fits are tied to the exact terminal instance and recheck visible/minimized/disposed ownership when the animation frame runs. Native likewise defers metric fit while inactive and rechecks lifetime when a scheduled frame runs. Existing Native authenticated launch/input/output/ACK/parser paths and Legacy user-input versus parser-reply distinctions are untouched.

Optional Native WebGL loading is lifetime-guarded and falls back to the existing terminal on unavailable GPU/context loss. Legacy retains its existing renderer registry, context-loss handling and periodic reload behavior, using a frozen creation-time renderer choice. Both keep the same latest theme options through fallback. No renderer change recreates a live terminal or process.

## Verification and limits

Focused component tests exercise both actual terminal components with external xterm/host boundaries mocked, including all four GUI/terminal light/dark combinations for Legacy Claude, Native Claude and Native Codex; safe option-only changes; fit coalescing; hidden deferral; retained text/selection; renderer-on-new-terminal selection; and WebGL construction/context-loss color equality. Persistence tests cover complete hydration, normalized deltas, same-lane GUI/terminal ordering, known rollback, unknown recovery barriers, stale shared reads and theme migration.

The tests certify frontend behavior and host contracts only. Rust tools are unavailable here: the new optional-DTO Rust tests are authored but NOT RUN. Actual PTY/CLI behavior, GPU behavior, Windows 1024×640 at 100%/125%/150%, macOS/Linux rendering, CJK/emoji glyph geometry, screenshots, accessibility and installed-client acceptance remain pending their final authorized platform gate. No full suite, build, package, CI or publication was run for this task.
