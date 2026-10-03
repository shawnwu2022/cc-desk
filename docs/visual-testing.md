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
fixed error. Only inert window-state/listener reads are provided for the real
TitleBar. There is no host bridge, PTY, configuration reader, profile mutation or
payload logger.

Normal development rejects the fixture route and source modules. Production
builds reject fixture imports even when the flag and visual mode are supplied.
Normal `App.vue` and `main.ts` contain no fixture routing. The visual server refuses
normal App/main entry requests, so the fake host graph cannot stand in for the
product. No release or updater policy changes are involved.

Fixture component tests have a dedicated `vitest.visual.config.ts`; the ordinary
unit suite excludes `tests/visual/` and keeps its real API/module boundaries.
Playwright discovers only `*.spec.ts` there.

Browser setup actions are shared with real-fixture DOM regressions in
`tests/visual/fixtureActions.ts`. Project expansion keeps stable toggle membership
while each click changes `aria-expanded`; menu setup focuses the real session row
before clicking its hover/focus-enabled overflow button. These regressions verify
setup ordering and actual component state, not browser hit testing or pixels.

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
`visual-evidence/verification.log` and `test-results/visual-verification/`. When all
baselines are absent and this run fails, a separate snapshot-only run writes candidate
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
