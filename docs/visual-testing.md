# Unified workspace visual harness

The fixture renders the production shell, project/session rows, resource views,
Settings sections and shared menu/dialog/drawer primitives with fabricated in-memory
data. Populated terminal areas use the production non-PTY `TerminalThemePreview`.
The `empty` and `empty-project` scenarios mount the actual `UnifiedTerminalHost`
with an empty session list, matching the production empty-slot geometry without
mounting `TerminalView` or `NativeCliTerminal`. The empty slot fills both host axes
so the guidance group centers within the available main region. Runtime IO
certification belongs to the terminal and real-CLI gates.

## Isolation

Run the visual server only with both `--mode visual` and
`CC_DESK_VISUAL_FIXTURE=1`. `build/visualFixture.ts` serves `/__visual__/` only in this
explicit **serve** mode. All `@tauri-apps/*` imports resolve to a fail-closed local
stub in that graph; host calls increment a reactive violation counter and throw a
fixed error. Only synthetic window-state/listener reads are provided for the real
TitleBar and unified focus owner. A synthetic `requestUserAttention(null)`
cancellation is inert; every positive attention request remains blocked. There is no host bridge, PTY, configuration reader, profile mutation or
payload logger.

Normal development rejects the fixture route and source modules. Production
builds reject fixture imports even when the flag and visual mode are supplied.
Normal `App.vue` and `main.ts` contain no fixture routing. The visual server refuses
normal App/main entry requests, so the fake host graph cannot stand in for the
product. No release or updater policy changes are involved.

Fixture component tests have a dedicated `vitest.visual.config.ts`; the ordinary
unit suite excludes `tests/visual/` and keeps its real API/module boundaries.
Playwright discovers only `*.spec.ts` there.

The isolated visual plugin fixes the displayed application version to `0.18.1`,
the synthetic value in the reviewed Settings baselines. Version text is fixture
data alongside the clock and project names; release metadata changes should not
replace otherwise identical UI screenshots. The same production Settings
components still render the footer normally. Ordinary development and production
keep the real package version, including when a visual flag is passed to a build.
Actual Vite resolver tests cover every enablement boundary, and the ordinary
package identity test checks the compiled display version against package metadata.
Baseline changes require inspection of the actual pixels and exact source/hash
provenance. The thirteen-image inventory, zero-pixel comparisons, geometry and
interaction assertions remain required.

The 0.18.12 session icon merge replaces five Linux Chromium screenshots with
independently inspected actual PNGs from PR49 visual run 38222119387 attempt 2.
The two pre-acceptance attempts produced identical bytes for all five images.
Three dark-terminal captions also show the same grayscale antialias change in
their first line; the text, geometry and all other main-content pixels remain
unchanged. `approval-manifest.json` records exact hashes and accepted regions.
The failed pre-acceptance runs stay failed; the updated images require a fresh
zero-tolerance, no-update PR run before merge.

Browser setup actions are shared with real-fixture DOM regressions in
`tests/visual/fixtureActions.ts`. Project expansion keeps stable toggle membership
while each click changes `aria-expanded`; menu setup focuses the real session row
before clicking its hover/focus-enabled overflow button. These regressions verify
setup ordering and actual component state, not browser hit testing or pixels.

## PR57 session workflow baseline acceptance

The v0.21.0 close/archive separation reserves independent trailing action slots.
Two agents opened and inspected all five changed actual/expected/diff groups from
[PR57 visual run 38884099695 attempt 1](https://github.com/shawnwu2022/cc-desk/actions/runs/38884099695).
The remaining eight baseline images were also opened and independently SHA-256
checked against the artifact copies; their bytes are unchanged. No error page,
hidden content, tolerance change or screenshot mask was accepted.

Capture source head: `327a6b60870547b1c0a11f327e28204bb98e4c25`; tested PR merge:
`ff91ee7b72add5fad6a7356932004b1e0f8ffa85`. Artifact **12038981730** was independently
downloaded through the authenticated API: 34,436,254 bytes, SHA-256
`915f49f52c462b29bd0a4d160d251565f77479dc3dd901422331e3c02ecd6625`.
Environment: Ubuntu 24.04, Node 22.23.3, Playwright 1.63.0, Chrome for Testing
153.0.8010.12. Only the exact five actual PNG byte streams from that archive are
adopted; the approval manifest preserves their former bytes' provenance.

| Image | Reviewed change | Raw changed pixels / exclusive bounds |
| --- | --- | --- |
| workspace-mixed-1366-zh | Earlier session-title truncation; time/retry move left for separate archive slot | 2647 / [175,183,289,555] |
| workspace-hover-action-1366-en | Close moves to independent primary slot; full title Tooltip follows its narrower anchor and stays in viewport | 4734 / [67,183,290,555] |
| archived-sessions | Archive-row time/restore move left; drawer boundary and Close stay unchanged | 503 / [1246,74,1307,125] |
| workspace-menu-1024-en | Underlying rows change; menu geometry, text and focus outline remain identical | 1679 / [175,183,290,555] |
| workspace-dark-gui-light-terminal | Same row slot changes; main and terminal pixels unchanged | 2452 / [175,183,290,555] |

The original run remains **RED: 5 screenshot failures, 231 other cases passed**.
Adoption is pixel review, not a passing verification. A fresh complete original
236-case CI run with `updateSnapshots: none` and `maxDiffPixels: 0` is required
before merge. This evidence does not certify Windows WebView2 or real CLI IO.

## CLI group integration baseline acceptance

On 2026-10-17, grouping, creation ordering and runtime optimizations were reviewed
as one integrated working tree: `b8924d7` (including `3be33b1`) plus `12a7af1`,
based on `351f539`. The four actual PNG streams below were independently opened
alongside their expected/diff images by `root` and `integrated_review`. Nine prior
baseline byte streams remain unchanged. No mask, tolerance or production CSS was changed.

| Image | Raw changed pixels | Exclusive difference bounds |
| --- | ---: | --- |
| workspace-mixed-1366-zh | 27732 | [60,163,515,619) |
| workspace-hover-action-1366-en | 33941 | [60,163,534,619) |
| workspace-menu-1024-en | 45026 | [60,163,534,619) |
| workspace-dark-gui-light-terminal | 28033 | [60,163,534,619) |

The two 28px CLI headers each have a 2px list gap: the selected row and its
hover tooltip move down 30px, and subsequent projects move down 60px. The shared
menu follows its row from y325 to y355 (bottom y493 to y523), retaining its size,
entries and focus outline without clipping. Column widths, title truncation,
terminal geometry and terminal text positions remain intact. Each image also has
exactly 13 changed pixels inside the terminal sample leaf emoji: every changed
channel differs by only one and alpha stays 255. Their bounds are
[503,163,515,171) in Chinese and [522,163,534,171) in English. These raster details
were explicitly inspected and accepted; the screenshots are unedited actual bytes.

Capture environment: Ubuntu 22.04 under WSL2, Node 22.19.0, Playwright 1.63.0,
Chrome for Testing 153.0.8010.12, Noto core `20201225-1build1`, CJK
`1:20220127+repack1-1` and emoji `2.047-0ubuntu0.22.04.1`. The local fontconfig
default omitted the RGB subpixel setting. Its first full run remains RED
(10 screenshot failures, 227 passes). A process-only `FONTCONFIG_FILE` restored
the original font rendering: all nine unrelated comparisons passed, and only the
four reviewed group-header screenshots failed. That 4-failure/9-pass comparison
also remains RED; image adoption does not rewrite a failed verification result.
Passing Playwright comparisons do not assert byte equality of separately captured PNGs.

For this local environment the exact fontconfig file is:

```xml
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <include>/etc/fonts/fonts.conf</include>
  <match target="font">
    <edit name="rgba" mode="assign"><const>rgb</const></edit>
  </match>
</fontconfig>
```

Set `FONTCONFIG_FILE` to that file only for the test process. Its SHA-256 is
`e86daa6a2a4f1283605029630bf393957b784dd0a582d9c5ed3fea7c78305e3f`.
The reviewed frontend-source patch against `351f539` (excluding baseline images)
has SHA-256 `3ac6696677b68b94e582022cd6f22067e18b482bb0525142c5efe002e8877173`;
the exact diff command, per-image hashes, prior provenance and both reviewers are
recorded in `tests/visual/__screenshots__/approval-manifest.json`. Local logs,
actual/expected/diff files and that source patch are retained under
`test-results/release-integration-review/`. After adoption, a fresh complete
237-case no-update run passed in 2.6 minutes, including every original snapshot,
geometry/interaction case and the new real-browser bidirectional collapsed CLI
navigation regression. `updateSnapshots: none` and `maxDiffPixels: 0` remain unchanged.
Current-source CI and Windows WebView2/real CLI acceptance remain separate requirements.

## PR58 CLI group header baseline acceptance

Two agents (`root` and `backend_review`) opened all four actual/expected/diff
groups from [PR58 visual run 39052536216 attempt 1](https://github.com/shawnwu2022/cc-desk/actions/runs/39052536216)
and all nine retained baseline images. The artifact's thirteen previous baseline
copies match the repository SHA-256 values. Only the four exact actual PNG byte
streams are adopted; the other nine files and prior review provenance remain intact.

Capture source head: `f822aa03661875ba2529f59969cb84a39df3f989`; tested PR merge:
`1c8a5cd2fe3c1b10fa4ddcf9215e6e699b16f70b`. Authenticated artifact **12108256058**
was independently downloaded and hashed: 34,100,120 bytes, SHA-256
`b5908107d32c117d82b7cefcc4517ba1cc61dfe0ffd5079f8b9311fd1c081a33`.
Environment: Ubuntu 24.04, Node 22.23.3, Playwright 1.63.0, Chrome for Testing
153.0.8010.12; Noto core `20201225-2`, CJK `1:20230817+repack1-3`, emoji
`2.047-0ubuntu0.24.04.1`.

| Image | Raw changed pixels | Exclusive difference bounds |
| --- | ---: | --- |
| workspace-mixed-1366-zh | 13129 | [60,163,515,622) |
| workspace-hover-action-1366-en | 15359 | [60,163,534,622) |
| workspace-menu-1024-en | 19266 | [60,163,534,622) |
| workspace-dark-gui-light-terminal | 13360 | [60,163,534,622) |

The CLI headers now use centered rule-labels, omit redundant icons and show
active/total counts. The list gaps move the selected row and its tooltip/menu
down 2px and subsequent projects down 4px. The menu, focus outline and tooltip
remain inside the viewport. Each image also contains exactly thirteen leaf-emoji
pixels differing by one channel value, at [503,163,515,171) for Chinese or
[522,163,534,171) for English. No terminal text or geometry changes were found.
No error page, hidden content, mask or tolerance change was accepted.

The original run remains **RED: four screenshot failures, 233 cases passed**.
The subsequent backend-only rustfmt repair does not alter the rendered source.
A fresh full 237-case no-update CI run with `maxDiffPixels: 0` remains required
before merge. Pixel acceptance does not certify Windows WebView2 or real CLI IO.

## Commands

Install the lockfile's exact development dependency and its official browser:

```sh
npm ci
npx playwright install chromium
npm run test:visual:unit
npm test -- tests/config/visualFixtureBoundary.test.ts tests/productBoundary.test.ts
npm run typecheck
npx playwright test tests/visual/unified-workspace.spec.ts
```

For an already authorized installed Chromium, set
`PLAYWRIGHT_CHROMIUM_EXECUTABLE=/absolute/path/to/chromium` before the Playwright
command. This is explicit; there is no silent executable fallback. Record that
browser's version with the evidence. `PLAYWRIGHT_BROWSERS_PATH` may select a writable
local installation cache without changing the committed configuration.

Missing screenshots fail: `updateSnapshots` defaults to `none`. Once the actual
rendered results have been inspected and corrected, capture candidate baselines:

```sh
npx playwright test tests/visual/unified-workspace.spec.ts --grep snapshot --update-snapshots
npx playwright test tests/visual/unified-workspace.spec.ts
```

Review every PNG under `tests/visual/__screenshots__/` at its actual dimensions
before accepting that candidate. Screenshots explicitly use device pixels (`scale: device`), including the DPR 1.5
sample. The comparison permits zero differing pixels.
Failures/traces are under `test-results/visual/` (ignored). Never create a passing
baseline from an error page, hidden content or an uninspected screenshot.

## Matrix and determinism

- 10 required named screenshots, plus menu, transformed-tooltip and dark-GUI/light-terminal states
- 120 geometry cases: five CSS viewport sizes × DPR 1/1.25/1.5 × zh/en × light/dark GUI × compact/standard density
- Eight empty-workspace geometry cases: 1024/1468 CSS viewport widths × zh/en ×
  no project/selected project, checking both center axes after sidebar collapse and
  re-expansion, with no terminal children or host calls
- Both light and dark terminal palettes under each GUI theme
- 200-character session title and 80-character project name
- Fixed UTC time through Playwright's clock, fresh browser contexts, local fonts,
  reduced-motion preference, disabled screenshot animations and hidden caret
- Production geometry, clipping, overflow and focus CSS remains active; no screenshot masks
- Named menu/dialog keyboard and focus cases, overlay/inline resource threshold,
  and a transformed/clipping-ancestor tooltip geometry case

The configured Windows user-agent selects production Windows-style title-bar
controls. This is still **Linux Chromium rendering**, not Windows WebView2. DPR
changes raster density without multiplying CSS viewport width twice. The
`projects-150-percent` sample uses 1280×720 CSS pixels at DPR 1.5, producing a
1920×1080 raster; it is not proof of native Windows 150% desktop scaling.

The fixture requests installed `Noto Sans` / `Noto Sans CJK SC` and the terminal's
normal fallback stack. Keep browser/OS/font versions consistent for bit-exact
comparison. Windows WebView2, macOS/WebKit and Linux native WebView acceptance
remain separate. No screen-reader, real GPU or real Claude/Codex certification is
implied by this harness.

## Initial 0.17.7 baseline acceptance

All 13 Linux Chromium baseline PNGs in `tests/visual/__screenshots__/` were copied
byte-for-byte from artifact **11153226557**, after two independent reviews opened
and inspected every actual image. Their dimensions and SHA256 values match the
[approval manifest](../tests/visual/__screenshots__/approval-manifest.json).

Accepted capture provenance:

- [Actions run 36845204924](https://github.com/shawnwu2022/cc-desk/actions/runs/36845204924)
- Source head `d90ec90db9f2a27f1e77f747fecd62c6f716d233`
- Tested PR merge `2ce9e19545318a35a79094c7aea179ddb434fcd0`
- Artifact ZIP SHA256 `8559c2be34ebc3fcd8f1b33a2381ceca331873faadc20cb626ba753dbbb0ddd9`
- Ubuntu 24.04 Linux Chromium, Node 22.23.3, Playwright 1.63.0,
  Chrome for Testing 153.0.8010.12

That full browser run executed **138 cases: 120 geometry and five interaction
cases passed; 13 screenshot cases failed solely because baselines were absent**.
The separate capture then passed 13/13. The original missing-baseline failures
remain genuine RED evidence; capture success and pixel approval do not turn that
verification into PASS. The **full no-update screenshot/geometry/interaction gate
against the committed accepted baselines subsequently passed all 138 cases in [run 36847655055](https://github.com/shawnwu2022/cc-desk/actions/runs/36847655055)**.

### Rendered review and corrections

The first run,
[36839148817](https://github.com/shawnwu2022/cc-desk/actions/runs/36839148817),
used source `397b3bfc7e9d954973aa664f83d9f30bf04de937` and merge
`b8aee85adac4f50b0e59e907428c74e8209a7e04`. Its 137 cases had 123 passes,
13 missing-baseline failures and one real transformed-tooltip geometry failure:
right edge 1713px instead of at most 1012px. Actual pixel review also rejected
stale project-toggle tooltips, accidental Close-button hover, the undersized
projects raster and a Create action below the initially visible dialog body.

Production fixes portal passive tooltips to the document body above modal
backdrops while retaining trigger focus, hover/Escape and description semantics.
New-session Create uses the shared non-shrinking footer and native form ownership;
repeated synchronous submits are rejected after closure. The added rendered case
checks the action at 1024×640 after scrolling and real Enter submission.

The next capture,
[36842796908](https://github.com/shawnwu2022/cc-desk/actions/runs/36842796908),
passed all 125 geometry/interaction cases but exposed an unintended whole-main
focus outline in four PNGs. Fixture setup now clears the prior control through
real focus/blur events and restores the main element's previous tabindex. It
preserves intentional menu/dialog/tooltip focus and row hover, with no CSS outline
suppression or masks. Pointer setup uses the empty main gutter. Screenshot scale
is explicitly `device`; the projects sample is now 1920×1080 for CSS 1280×720 at
DPR 1.5, while zero differing pixels and all viewport/DPR cases remain unchanged.

In the accepted final capture, all nine previously acceptable images were
byte-identical and the four rejected outline images were corrected. Every final
PNG was reopened and approved; no remaining demonstrated pixel defect was found.
The original artifact's candidate manifest remains unmodified with `approved:
false`; the repository's separate approval manifest records the subsequent review.

Linux Chromium baselines do not certify Windows WebView2 or native 150% desktop
scaling. Windows manual acceptance, screen-reader behavior and D20 real CLI IO
certification remain pending/separate. No merge, public release or updater
promotion is authorized by this evidence.

Local browser attempts remain blocked: official browser downloads returned HTML,
system Chromium failed its process-singleton socket with `Operation not permitted`
including the authorized escalated retry, and the managed cloud browser rejected
the local URL with `ERR_BLOCKED_BY_CLIENT`. No retry or bypass was attempted for
these corrections; actual rendered evidence was obtained through the CI runs above.


## Prepared final workflow and evidence review

`.github/workflows/unified-visual.yml` runs on the final PR into
`feat/native-cli-finalization` (or `main`), with no development push trigger. It uses
Ubuntu 24.04, Node 22, `npm ci`, the lockfile's Playwright 1.63.0 and official Chromium
installation with system dependencies, plus Noto core/CJK/emoji fonts. Browser and
font package versions are recorded with source head and actual tested PR merge commit.

The first full run uses `--update-snapshots=none`. Its logs/traces remain under
`visual-evidence/verification.log` and `test-results/visual-verification/`. When
verification fails, including stale or absent baselines, a separate snapshot-only run writes candidate
PNGs with `--update-snapshots=all`. Copies in
`visual-evidence/candidate-baselines-unapproved/` have dimensions/SHA256 recorded in
`candidate-manifest.json` with `approved: false`. Capture failures also retain artifacts.
The final workflow guard returns failure unless the **original** full verification
passed; successful capture never changes missing-baseline or geometry failures into PASS.

Artifact name: `unified-visual-<tested-commit>-<run-attempt>`, retained 14 days. A reviewer
must inspect every actual image at its recorded dimensions, evaluate all other failures,
and only then accept corrected baselines in a reviewed commit. The subsequent full
no-update run is the screenshot/geometry/interaction gate. A browser-launch error in
CI would still be a blocker, not the required missing-baseline failure.

GitHub [requires a workflow_dispatch file on the default branch](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onworkflow_dispatch).
This new file on the feature branch cannot promise an immediate manual dispatch.
A [pull_request event tests the merge ref](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request)
and supplies the head SHA separately. Opening the eventual draft also starts the
existing ordinary CI and Windows package gate; the latter now includes unified UX
source/configuration/test paths in its PR filter, with its push filter/jobs unchanged. The original plan required pixel freeze first, so with rendering
blocked locally the owner approved the final draft/CI sequence on 2026-10-01, after
independent review. The parent owns those external actions. Task25 local preparation itself did not
create a PR or run a workflow. The later CI captures and baseline acceptance are
recorded above; the final no-update gate remains pending.

| Snapshot | CSS viewport | DPR | Expected raster |
|---|---|---|---|
| workspace-empty-1024-zh | 1024×640 | 1 | 1024×640 |
| workspace-mixed-1366-zh | 1366×768 | 1 | 1366×768 |
| workspace-hover-action-1366-en | 1366×768 | 1 | 1366×768 |
| workspace-resources-overlay-1024 | 1024×640 | 1 | 1024×640 |
| projects-150-percent | 1280×720 | 1.5 | 1920×1080 |
| new-session-dialog | 1024×640 | 1 | 1024×640 |
| archived-sessions | 1366×768 | 1 | 1366×768 |
| settings-terminal-light-gui-dark-terminal | 1440×900 | 1 | 1440×900 |
| settings-launch-configurations | 1280×720 | 1 | 1280×720 |
| confirm-stop-and-archive | 1024×640 | 1 | 1024×640 |
| workspace-menu-1024-en | 1024×640 | 1 | 1024×640 |
| workspace-dark-gui-light-terminal | 1280×720 | 1 | 1280×720 |
| tooltip-transformed-1024 | 1024×640 | 1 | 1024×640 |

All 13 repository baselines now match these dimensions and the accepted artifact
hashes. The first projects candidate was only 1280×720 and was not accepted.
A future full no-update run must validate the complete baseline inventory.
Windows WebView2/OS scaling and real CLI remain separate gates.

### Final review: pipeline failure propagation

Both Playwright producer pipelines explicitly select `shell: bash`, including the optional candidate capture. GitHub therefore invokes Bash with pipefail. `tests/config/visualWorkflow.test.ts` executes each actual step script with only the Playwright producer replaced by `node -e "process.exit(7)"`; both must exit 7 through `tee`. This complements the final outcome guard tests and does not launch a browser or create/approve baselines. The original verification result remains mandatory even if candidate capture succeeds.

## 0.18.0 feedback baseline update

[Run 36861864182](https://github.com/shawnwu2022/cc-desk/actions/runs/36861864182) tested source `444c2dfed9ed11305ea77fc14063caf17155d1a8`, merge `e7eebee57566b4f13de8b2c69a606ec1f48fc3b8`. All 133 geometry/interaction cases and five unchanged screenshots passed; eight screenshots differed only in the requested marks/status/chevrons, centered guidance and version text. The run remains a failed comparison, not a full PASS.

All eight actual PNGs were independently inspected by two reviewers at original dimensions and accepted. They were copied byte-for-byte from artifact `11162500482` (ZIP SHA256 `2a3007d155f4fbc4f3ead6e5bf1d3d950367d0cb5ee37bee889368849b84b2c7`); five existing PNGs remain unchanged. The approval manifest records both generations of provenance. No image editing, masks or tolerance changes were used. A fresh full 146-case no-update run is required after committing this update.

## Single Close control baseline review

Run [37318672270](https://github.com/shawnwu2022/cc-desk/actions/runs/37318672270)
verified source `553bd3fd58ad88c1d0cb9c501d565c056319ae64`, tested merge
`eb3a4c1f1ee28d20e055308e040092aae7881fa3`: 202 cases passed and two screenshot
comparisons failed. Two independent pixel reviews accepted only the intended
Stop-to-× replacement (44 pixels) and removal of Stop, Close and Stop and archive
from the running-session menu (1590 pixels). The actual images are copied
byte-for-byte from artifact `11349467522`; the approval manifest records hashes
and provenance. The other eleven baseline images are unchanged. No tolerance,
mask, production CSS or snapshot inventory changes are used.

The original failed comparison remains RED evidence. A subsequent complete
no-update run is required. New bilingual Close confirmation and opened-session
state/menu captures remain separate unapproved evidence, not additional baseline
approvals or proof of real process lifecycle, Windows WebView2 or screen-reader
acceptance.

The opened-state evidence run at source `c4b9cabe362895945ccb69a9f56bd3470b6d5581`
passed all thirteen approved baseline comparisons. Its eighteen new keyboard cases
failed before the focus/menu captures: freezing `Date.now()` made Vue's parent
capture timestamp equal its child listener attachment time, so native Escape did
not reach the button listener. Native-event fixture regressions reproduce that
failure; the browser setup now sets the same initial system date while allowing
time to progress. The Escape/tooltip/focus assertions, production UI and zero-pixel
comparison remain unchanged. This repair still requires a fresh full browser run.

## 0.18.1 version-label baseline review

Run [37351948916](https://github.com/shawnwu2022/cc-desk/actions/runs/37351948916)
tested source `6aa9b2ca99ddcd479587063da75c4da171cf922b`, merge
`d879db0105691e2bdc4779142555c949f209bd38`: 224 of 226 cases passed. All 213
non-snapshot cases and eleven unchanged screenshot comparisons passed; only the
two Settings screenshots differed. The original full comparison remains RED.

Two independent reviewers opened all six expected/diff/actual PNGs at original
dimensions. Both changes are confined to the last digit of the Settings footer,
`CC Desk v0.18.0` → `CC Desk v0.18.1`, supplied by the production
`SettingsView.vue` compile-time version from `package.json` through Vite. Exact
RGBA comparison found 52 changed pixels in each image, within inclusive bounds
`x=131–137, y=876–883` for the 1440×900 terminal screenshot and
`x=131–137, y=696–703` for the 1280×720 launch-configurations screenshot.
Playwright's comparison highlighted 18 and 14 pixels respectively. Every pixel
outside those bounds is identical; no clock, terminal, layout or other UI
difference was found.

Only these two reviewed actual PNGs were copied byte-for-byte from artifact
`11363191338` (ZIP SHA256
`d8d767d10b9aedae6aca609c35616cffceed5e63dfb526450447383488f6944f`).
The approval manifest records the exact new hashes and retains earlier approval
provenance. The other eleven baseline images are byte-identical, the inventory
remains thirteen, and no masks, tolerance, production CSS or clock changes were
made. The original artifact remains untouched. Other captured evidence remains
unapproved.

A fresh full no-update run of all 226 current cases is required after committing
this reviewed baseline update. Accepting these version-label pixels does not
make the original failed run pass, certify Windows WebView2 or real CLI behavior,
or authorize release/updater promotion.

## 0.18.3 candidate visual changes

The project-name sorting, restored status shapes and external Archive control
intentionally change workspace pixels. Baselines change only after actual
CI captures are inspected and independently reviewed. Original thirteen-image
inventory, zero-pixel thresholds and all geometry cases stay required. Opened
state captures now check the external Archive button's real keyboard focus and
viewport hit target while requiring no Archive entry in any menu. The test fixture
explicitly supplies known idle activity for its synthetic running examples; it
cannot be used as evidence that an actual Native source supplies that state.

### Reviewed baseline bytes and pending current-source verification

The nine changed actual PNGs from
[run 37965393533](https://github.com/shawnwu2022/cc-desk/actions/runs/37965393533)
were inspected on the authorized Windows evidence host and then reopened in the
cloud workspace at their original dimensions. The accepted differences are
displayed-name project sorting, restored status icons/counts, and menu/hover
positions following their moved session rows. Terminal pixels, dialog contents,
drawer geometry, tooltip anchors and theme combinations have no unexplained
change. Four existing baseline PNGs remain byte-identical.

The replacement bytes come from source
`198c3f892de4b12d9f484c736222a4bf188b9ca6`, tested merge
`79fec77d9e7a1eeeb3978f0ed86f6be9fb73d09c`, artifact `11633985429`, ZIP SHA256
`0718e755776fca863f27fdbaa5862fcff37d9bfda9903e1c58398ab14fb715d1`.
Explicitly authorized repository sharing preserved those bytes in evidence
commit `728ae1fedd2b07040c58eb2c1e1484c1d2ad228b`, tree
`c44e02a1374736bef7e3b5e47a13e61e56669d33`. Its source map SHA256 is
`ff3bff2bfeacc406db9443f58eeae398970af205f920769d86c8c40d31b81a96`.
The approval manifest records every ZIP entry, image hash and review scope.
Only the nine inspected files are copied byte-for-byte; there is no image editing,
masking, threshold change or screenshot inventory change.

These are **old-source captures**, not screenshots of `f36fd14` or the updated
PR head. The Native receipt/title flex changes after `198c3f8` still require
current-source verification; matching old/new failure counts cannot prove pixel
identity. Both previous failed runs remain failed. The new exact PR source must
pass the complete 230-case no-update screenshot/geometry/interaction gate with
`maxDiffPixels: 0`. The four additional Native receipt captures remain separate
unapproved evidence. Native current working/completed, live CLI and native
WebView/taskbar acceptance remain unverified/separate.

## Unreleased session-list glyph evidence

The `session-status` scenario uses seventeen explicit synthetic display inputs and
production session rows. Four additional `session glyph and row restore evidence`
Playwright cases capture light/dark English/Chinese glyph sheets separately as
unapproved PNG attachments. They check the absence of an independent Resume control,
live age, row keyboard focus and immediate status-hover dismissal. Row admission is
covered separately by actual-App regressions with synthetic IPC. No CLI or observer activity
is generated. The thirteen historical baselines and zero-pixel threshold are
unchanged; an intentional new UI mismatch remains a failed baseline until reviewed
and explicitly authorized. Do not call fixture or Linux evidence Windows acceptance.

## 0.18.7 row-restoration baseline review

[Run 38054103755](https://github.com/shawnwu2022/cc-desk/actions/runs/38054103755)
tested source `9bad3cc832589dfe792589e79acf189d39d2894b`, merge
`14094d5793aad269111802f5585bfc9c426bf0b1`: **230 of 234 browser cases passed**,
with four historical snapshot comparisons failing. Artifact `11670383443`, ZIP
SHA256 `9a9463e23086323bc0e0cd69382d3bfc9d8a72e95e6473cda685c06f077b3968`,
was verified before opening every affected actual, expected and diff image.
Two independent pixel reviews found only the requested closed Settings-row change:
the separate Resume/play control disappears, the title gains space and the age
moves right. All 354–358 changed RGB pixels lie within exclusive bounds
`[180,182,263,192]`; every pixel outside those bounds is identical.

Only `workspace-mixed-1366-zh`, `workspace-hover-action-1366-en`,
`workspace-menu-1024-en` and `workspace-dark-gui-light-terminal` are copied
byte-for-byte from those inspected actual PNGs. The other nine baselines retain
their bytes and provenance, with the same thirteen-image inventory and zero-pixel
threshold. The approval manifest preserves prior approval records and records the
new per-image provenance. The original run remains failed; a fresh complete
234-case no-update run is required on the updated PR head.

Both reviewers also opened the four light/dark English/Chinese status sheets.
The history row has no separate Resume control, and status/title/age and selected
row layout remain intact. These sheets remain synthetic evidence, not historical
baselines or Windows WebView2/live CLI acceptance. Source-drawer behavior has
actual App interaction coverage but no dedicated browser pixel acceptance.

## PrimaryNav settings-anchor sink invalidates all baselines

A broken style was fixed in `PrimaryNav.vue`: `margin-top: auto` was declared on
the settings button, but the flex item of `nav.primary-nav` is the wrapping
`.ui-tooltip-anchor`, so the rule never applied and all three destinations sat at
the top. The auto margin now targets the anchor via
`:deep(.ui-tooltip-anchor:has([data-primary-section="settings"]))`, and the
settings button renders at the bottom of the nav column.

Every one of the thirteen baselines shows the primary nav, so all of them
necessarily differ (the settings glyph moves from the third top position to the
bottom-left corner). Until the standard cycle completes — full RED run, artifact
capture with `--update-snapshots=all`, independent pixel review, manifest update,
fresh no-update gate — the committed baselines do not match the intended UI. A
local Chromium (1280×800, non-WebView2) measurement confirmed
`settings.bottom = 792/800`, `projects = 78–110`, anchor is the flex child.

## Session-list regrouping also invalidates all baselines

The same batch also changes the session list itself, so the baseline refresh above
must cover both at once. `ProjectNode` now clusters a project's non-archived rows by
CLI type (Claude first, then Codex) with no rendered heading, and `SessionsPanel`
orders rows by a frozen first-seen creation key (descending) instead of live
activity. Any baseline that renders a populated project tree changes in row order and
vertical position (the `mixed`, `hover`, `resources`, `projects` and `archived`
scenarios at minimum); settings-, dialog- and terminal-only baselines show no session
rows and are expected to differ only from the nav move. The exact per-image split must
be read from the actual capture, not assumed. A local Chromium check on the visual
fixture confirmed the `cc-desk` project splits into `claude` (2 rows) then `codex`
(2 rows) groups with no heading element.

Both changes ride one approved capture: do not accept a partial baseline set that
reflects only the nav move or only the row regrouping.

## Local acceptance run and the environment floor

The hoisted drag-drop listener in `UnifiedTerminalHost` subscribes through
`getCurrentWebview().onDragDropEvent` on mount, which the visual stub previously
counted as a blocked host call, failing all eight empty-workspace geometry cases
with `data-blocked-host-calls="1"`. The stub now provides the same class of inert
read-only subscription already granted to window-state listeners: a synthetic
`onDragDropEvent` that registers nothing and never delivers events. Positive host
calls stay blocked and counted, and the drag events themselves cannot cross the
fixture boundary.

With that repair the local no-update gate reproduces the documented RED state
exactly: 175 cases pass (120 geometry, 8 empty-workspace centering, interaction,
glyph/receipt/close-state evidence) and only the thirteen baseline comparisons
fail. A DOM probe on the served fixture confirms the intended structure directly:
the settings anchor occupies `top=728, bottom=760` inside a nav column ending at
768, and the `cc-desk` project renders two `claude` rows followed by two `codex`
rows with no heading element.

Local pixel comparison against the committed baselines has a floor that is
independent of this batch: an identical capture of unmodified `main` on this
Windows Chromium does not reproduce the Linux-captured baseline bytes either.
Within one local environment the branch-vs-`main` delta on the empty scenario is
378 pixels confined to `x∈[13,30]` at exactly the two settings-icon slots (the
third top slot vacated, the bottom slot filled); populated scenarios add the row
regrouping. That isolates the branch's visual change from the font/rasterizer
noise, but it is not baseline acceptance: freezing the thirteen baselines still
requires the standard Linux Chromium CI cycle (RED run, `--update-snapshots=all`
capture, independent pixel review, manifest update, fresh no-update gate), and
Windows-rendered PNGs must not replace the Linux provenance.


## PR51 session UX batch baseline adoption

All thirteen Linux Chromium baselines are copied byte-for-byte from the actual
PNGs accepted by one independent review of actual, expected and diff images for
[run 38550064925](https://github.com/shawnwu2022/cc-desk/actions/runs/38550064925),
artifact `11885664893`. Source head is
`4c74618853f01dc5a65a848a7a63aed16490eaf2`; tested synthetic merge
`8ac79430edd9170d85c5f3785b1f8340c2f11483` has the same tree
`e289558672317438ae56fee7b46b997168f01aa2`.

The review accepts the bottom Settings anchor, CLI group spacing and closed
Codex artwork/badge changes, with no rejected image. The approval manifest
records each exact actual hash, size, mapping and previous baseline provenance.
The Library JSON and decoded ZIP were verified against their supplied SHA-256
values before all PNG hashes, dimensions and previous baseline hashes were
verified and any baseline was replaced. No other baseline was regenerated and
no comparison tolerance or test configuration changed.

The original run remains RED: thirteen screenshot failures and 223 other cases
passed. Fresh full verification of the updated source remains pending, using
`--update-snapshots=none` and the existing `maxDiffPixels: 0` configuration.
The local preparation environment lacks Playwright 1.63.0 and Chromium; this
adoption is not a successful browser test or hosted CI result. These Linux
synthetic screenshots do not certify Windows WebView2, native drag-and-drop,
terminal lifecycle, rename delivery, real CLI IO or authenticated receipts.

## PR62 compact-density baseline provenance repair

Main commit `b642a8ab93a7b333e1a781f9fa70b175736bdb50` changed
`projects-150-percent.png` without updating its approval manifest. The committed
PNG has SHA-256 `e06a7febb70895aa51a6aaedbe469ba0dac9722b4c77ad7a7f4726a24469a6e1`
and 40,576 bytes; its capture environment is unconfirmed. Standard Linux CI
[run 39191092606](https://github.com/shawnwu2022/cc-desk/actions/runs/39191092606)
passed 240 of 241 cases, with only this snapshot failing (13,420 differing
pixels). Direct inspection of actual, expected and diff at 1920 by 1080 pixels
confirmed the compact geometry, with differences in text and icon rasterization.
All thirteen baseline images were inspected; the twelve passing PNGs stay unchanged.

The replacement is the exact inspected actual PNG from artifact `12159579806`,
source head `cee1c14b5f950e22bd213e4b961ab22b1eeb0e67`, tested synthetic merge
`efa08c903a8c3a16d5dac11b9cac8e596edf2b88`. Its SHA-256 is
`d2dbcd7320cd993f7dfe27480cef1bb2c38932e322b0108924d49042cb1ca153`
and size is 59,931 bytes. The downloaded artifact ZIP matched GitHub's digest
`f79841dab041af3d38acdad6982ab55d7be1504dde1b83f60b3c5b9b2449deac`
and 31,878,696-byte size. The environment remains Ubuntu 24.04, Node 22.23.3,
Playwright 1.63.0 and Chromium 153.0.8010.12, with the recorded Noto fonts.

One reviewer accepted this image; earlier review records are retained separately.
No rendered source, tolerance or test configuration changes. The original failed
run remains RED. A fresh complete 241-case no-update run with `maxDiffPixels: 0`
is required before merging. This Linux fixture evidence does not certify Windows
WebView2, native scaling, real CLI lifecycle or the final title-sync fix.

## PR64 refined product icon baseline acceptance

The approved refined CC Desk icon replaces the former titlebar product mark.
Two reviewers (`root` and `icon_final_review`) inspected all thirteen
expected/actual/diff groups from [run 39224444406, attempt 1](https://github.com/shawnwu2022/cc-desk/actions/runs/39224444406).
The independent reviewer opened all 39 full-size images, including the complete
150% icon lower edge. Twelve images differ by exactly 175 pixels within
`[13,9,27,23)`; `projects-150-percent.png` differs by 406 pixels within
`[19,13,41,35)`. Every pixel outside those bounds is identical. Session CLI
marks, status badges, layout and text are unchanged; dark-theme brand colors
match the approved icon.

The accepted PNGs are the exact actual bytes from source head
`a8d12f9ab902ba93491f9d9d345c01ee04c22057`, tested merge
`d77b22b8a95b9a0a271bd83e4a55865b9134832e`, artifact `12173306619`.
The downloaded ZIP is 37,661,053 bytes with SHA-256
`02803bfb31e8beac8a517f3b179157c4b37fa8258d15366781c2c45913d3a409`.
The approval manifest retains each previous image hash and provenance, and
records the new hash, exact ZIP entry and pixel review for every accepted image.
The environment is Ubuntu 24.04, Node 22.23.3, Playwright 1.63.0 and Chromium
153.0.8010.12, with the recorded Noto core, CJK and emoji versions.

The original run remains RED: thirteen snapshot failures and 228 other cases
passed. This acceptance changes no rendered source, fixture, mask, tolerance or
test configuration, and performs no image repainting or regeneration. A fresh
complete 241-case current-head no-update CI with `maxDiffPixels: 0` is required
before merging. These Linux fixture pixels do not certify installed WebView2,
native taskbar/Dock presentation, native scaling or real CLI behavior.
