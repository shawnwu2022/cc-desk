# Unified workspace visual harness

The fixture renders the production shell, project/session rows, resource views,
Settings sections and shared menu/dialog/drawer primitives with fabricated in-memory
data. The terminal area uses the production non-PTY `TerminalThemePreview`; it does
not mount either real terminal host. Runtime IO certification belongs to the
terminal and real-CLI gates.

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

## Current evidence: first CI render reviewed, corrected rerender pending

The first genuine Linux Chromium run was
[Actions 36839148817](https://github.com/shawnwu2022/cc-desk/actions/runs/36839148817),
source head `397b3bfc7e9d954973aa664f83d9f30bf04de937`, tested PR merge
`b8aee85adac4f50b0e59e907428c74e8209a7e04`. It ran all 137 cases: 123 passed,
13 failed for absent baselines, and the transformed-tooltip geometry case failed
because its right edge was 1713px instead of at most 1012px. The separate candidate
capture produced all 13 PNGs; artifact `11151230321` records Ubuntu 24.04, Node
22.23.3, Playwright 1.63.0 and Chrome for Testing 153.0.8010.12.

All 13 actual images were inspected and their dimensions/SHA256 checked against
the artifact manifest. No baseline is approved or copied into the repository.
The tooltip image is rejected because its description is clipped out. Mixed,
hover and dark workspace images retain an unrelated project-toggle tooltip;
1024px empty/menu scenes accidentally hover the window Close control. The
projects sample used CSS screenshot scale and is only 1280×720, so it is rejected
as the promised DPR 1.5 raster. The new-session dialog hides its Create action
below the initially visible scrolling body. The five other candidates have no
additional demonstrated pixel defect but remain pending the corrected rerender.

Corrections under review portal passive tooltips to the document body above
modal backdrops, preserving the trigger's focus and description relationship.
Snapshot setup now moves focus to the fixture main area and the pointer to its
lower-right gutter, preserving intentional menu/dialog/tooltip focus and row
hover. Screenshot scale is explicitly `device`, with zero allowed differing
pixels unchanged. New-session Create uses the shared fixed footer with native
form ownership; repeated synchronous submissions are rejected after closure.

Component regressions cover clipping escape, modal focus/Escape ownership,
setup-tooltip cleanup, footer form association, invalid-argument retry and
single submission. The rendered suite still has 13 screenshots and 120 geometry
cases, now with five interaction cases (138 total); the added case checks the
new-session action at 1024×640 after scrolling and real Enter submission. The
corrected browser run and all resulting PNGs must be inspected before baseline
acceptance, followed by a full no-update run. These component checks do not prove
corrected pixels, native Windows scaling, screen-reader behavior or real CLI IO.

Local browser attempts remain blocked: official browser downloads returned HTML,
system Chromium failed its process-singleton socket with `Operation not permitted`
including the authorized escalated retry, and the managed cloud browser rejected
the local URL with `ERR_BLOCKED_BY_CLIENT`. No retry or bypass was attempted for
these corrections. The parent owns CI rerendering and baseline acceptance.


## Prepared final workflow and evidence review

`.github/workflows/unified-visual.yml` runs on the eventual final PR into
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
independent review. The parent owns those external actions. No PR or workflow has
been created/run by Task25 local preparation, and pixels remain unaccepted.

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

Every repository baseline above is still absent. The expected dimensions describe
the corrected device-scale capture; the first projects candidate was only 1280×720.
The first CI artifacts are unapproved evidence, not committed baselines. Windows WebView2/OS scaling and real CLI remain separate gates.

### Final review: pipeline failure propagation

Both Playwright producer pipelines explicitly select `shell: bash`, including the optional candidate capture. GitHub therefore invokes Bash with pipefail. `tests/config/visualWorkflow.test.ts` executes each actual step script with only the Playwright producer replaced by `node -e "process.exit(7)"`; both must exit 7 through `tee`. This complements the final outcome guard tests and does not launch a browser or create/approve baselines. The original verification result remains mandatory even if candidate capture succeeds.
