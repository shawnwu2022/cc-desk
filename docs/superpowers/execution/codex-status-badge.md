# Codex status badge repair — 2026-10-21

## Scope and repository checkpoint

- User-directed worktree: `E:\source\github\cc-box\.claude\worktrees\active-session-filter`.
- Branch: `feat/active-session-filter`, clean baseline `61ec2806c360730f0fc393941b67bde776e4fe3b`, based on `614d1e86cbae26c1844eb37f36cfe0e66b0a1554` (`origin/main` cache, package version 0.23.5).
- The primary checkout remains on `fix/opencode-backend` with pre-existing uncommitted work; no changes were migrated or overwritten. No new worktree or repair branch was successfully created.
- Remote main could not be refreshed: SSH known_hosts read failed; HTTPS Schannel reported `SEC_E_NO_CREDENTIALS`. No remote state is inferred from the cache.
- Library screenshot `libfile_4bb47d1b4cb08191a87c58103110a2bd` returned image metadata (169×66) and OCR `Codex CLI; 运行中`, consistent with the neutral running projection. Pixel materialization failed in this environment. The reported installed version and affected machine remain unconfirmed.

## Root cause and minimal repair

The backend Codex reporter allowlists only main-thread UUID and SessionStart,
UserPromptSubmit and Stop metadata. The authenticated hook bus binds those facts
to the exact run/generation; `NativeCliTerminal` applies Codex identity only.
`nativeTabs` validates the exact request/run/generation and rejects ended/stale
attempts. The Native adapter deliberately excludes Claude activity/notice overlays
for Codex, preserving unknown turn activity and a neutral process-running visual.
`SessionAppIcon` then suppressed every running badge. Together these rules left
healthy Codex sessions without a status corner for their entire running lifetime.

Keep the existing static running glyph for Codex. No observer, launch, npm wrapper,
command validation, hook arguments, user configuration, process control or status
authority changes. Claude's running presentation and closed rows remain unchanged.
This branch has no OpenCode implementation; the new condition matches Codex only.

## Verification checkpoints

- Baseline: `npm test -- tests/components/sessionAppIcon.test.ts tests/utils/unifiedStatusSemantics.test.ts tests/stores/nativeCliAdapter.test.ts` — 3 files / 102 tests passed.
- RED: new/resume tests through real Pinia tab store, Native adapter and mounted SessionItem — both failed at the missing `running` badge (2 failed / 6 passed).
- GREEN: initial affected set — 3 files / 104 tests passed. Expanded new/resume coverage also verifies English/Chinese names, startup, identity receipts, stale generation rejection, no completion claim on Stop, stopped and closed rendering.
- Targeted regression: 12 files / 252 tests passed (app/status icons, active-session filter, observer reducer/status/reporter, hook store, Native tabs/adapter, unified status, design tokens).
- First `npm run test:fast`: 181 files passed / 1 failed, 2916 passed / 1 failed. `tests/components/sessionItem.test.ts` → `Row_CompactTimeAndIcons_002` still asserted the old Codex no-running-badge contract. Updated that expectation and checked its badge remains decorative; both affected component files then passed (76 tests).
- Final `npm run test:fast`: 182 files / 2917 tests passed. `npm run typecheck` and `git diff --check` passed. Npm config, Vite CJS and expected negative-path test diagnostics remain existing warnings.
- Default browser entry failed all five cases before launch because Playwright's Chromium headless binary is absent. A second attempt on the default port was blocked by the first run's still-owned fixture server.
- Browser fallback: existing installed Chrome, temporary config with fixture port 4175, `npx playwright test --config playwright.codex-status.local.ts tests/visual/session-app-icon.spec.ts --grep 'merged app status'` — 4 passed, exit 0. Both owned fixture servers were stopped and the temporary config removed. No browser/application installation or baseline update.
- Inspected light/dark Codex captures under `test-results/visual/session-app-icon-merged-app-status-{light,dark}-codex/merged-status-{light,dark}-codex.png`: visible green running badge, static image/shape, fixed 20px slot, closed row without corner badge. Claude light/dark cases also passed. The touch-only unread case was not repeated under Chrome.
- Independent read-only review: no blocking findings; confirmed Codex-only running rendering, unchanged closed/Claude/notice priority, real store→adapter→row regression coverage and no launch/security/npm-wrapper edits. The reviewer made no writes or additional test claims.
- This checkpoint is ready for the user-authorized local commit on `feat/active-session-filter`; no push, main merge, release or application install is part of this task.

## Evidence boundary

Launch receipts and hook identities in these tests are synthetic host inputs.
No live account, real CLI turn, installed Windows WebView, OS taskbar or affected
machine screenshot validation is implied. Codex currently has no supported
working/idle/permission/completion producer; Stop is not completion. Those
semantics are preserved, not fabricated to make a badge change.
