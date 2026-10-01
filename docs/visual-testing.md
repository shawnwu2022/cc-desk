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
before accepting that candidate. The comparison permits zero differing pixels.
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

## Current evidence: BLOCKED_VISUAL

Infrastructure and component/isolation tests are implemented. No screenshot
baseline has been captured or approved, and the screenshot gate has not passed.

The Task 23 cloud environment installed `@playwright/test` 1.63.0 successfully.
Its official Chromium and headless-shell 153.0.8010.12 downloads returned a 195-byte
HTML response instead of a ZIP and failed extraction. Existing system Chromium
151.0.7922.173 reported `socket() failed: Operation not permitted` at process
singleton startup, including the authorized escalated retry. The managed cloud
browser separately rejected the local fixture URL with `ERR_BLOCKED_BY_CLIENT`.
No restriction was bypassed and no unofficial binary was fetched.

Consequently the first genuine missing-baseline run, all 10 required PNGs, three
supplemental PNGs, pixel review, the rendered tooltip result and the final no-diff
PASS remain pending in an authorized browser environment. The generated browser
launch failures are not counted as screenshot RED evidence. The parent owns the
remaining rendered gate and final CI sequencing.


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

Every baseline above is currently absent. This table is an expected artifact inventory,
not rendered evidence. Windows WebView2/OS scaling and real CLI remain separate gates.

### Final review: pipeline failure propagation

Both Playwright producer pipelines explicitly select `shell: bash`, including the optional candidate capture. GitHub therefore invokes Bash with pipefail. `tests/config/visualWorkflow.test.ts` executes each actual step script with only the Playwright producer replaced by `node -e "process.exit(7)"`; both must exit 7 through `tee`. This complements the final outcome guard tests and does not launch a browser or create/approve baselines. The original verification result remains mandatory even if candidate capture succeeds.
