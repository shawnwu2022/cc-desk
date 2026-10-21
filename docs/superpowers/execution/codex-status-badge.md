# Codex status badge repair — 2026-10-21

## Follow-up: native state investigation and implementation plan

The user clarified that a permanently green process badge does not satisfy the
task. Keep `2e5dc50` in history; extend this same branch with native state
observation. `codex --version` on this host reports **0.163.0** (npm wrapper at
`D:\nodejs\node_global\codex.cmd`). The pre-existing `codex-full` temporary source
cache differs from the release tag, so the source evidence below uses official
[`rust-v0.163.0`](https://github.com/openai/codex/tree/rust-v0.163.0), not that cache.

### Verified source and coverage matrix (before implementation)

| State / fact | Official source and actual external outlet | Planned Desk treatment |
| --- | --- | --- |
| Process starting/running/exited/closed | Existing authenticated owned-run launch receipts | Preserve process lifecycle precedence; never infer turn work/completion |
| Native TUI working | [`terminal_program_status.rs::SetTerminalProgramStatus::write_ansi`](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/tui/src/terminal_program_status.rs#L205), OSC 7501 `state=working:app=codex` | Working glyph/motion only after the exact terminal stream emits it; includes task execution, MCP startup and background exec |
| Native TUI idle | Same protocol, `state=idle:app=codex`; [`status_surfaces.rs::desired_program_status`](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/tui/src/chatwidget/status_surfaces.rs#L299) | Static idle indication with an honest Idle label; no completion check |
| Needs user action | Same protocol, `state=blocked:app=codex`; native bottom-pane action requirement wins over working | Generic needs-user glyph; approval, input, elicitation and other action overlays are not distinguishable here |
| Waiting specifically for approval | App-server approval request/response has request/turn/thread identity; PermissionRequest hooks run before other hooks may allow/deny | Do not mislabel generic blocked as permission. No new app-server connection or approval ownership |
| Final turn completion / interrupt | `core/src/tasks/mod.rs` emits TurnComplete/TurnAborted, rollout persists them with turn IDs; hook Stop may be vetoed, notify may precede final compaction | No completion from OSC idle, Stop, notify, silence or process exit; not integrated by this minimal repair |
| Terminal handoff / clear | `terminal_program_status.rs::ClearTerminalProgramStatus`, OSC 7501 `state=clear`; `tui.rs` clears on external-editor handoff and exit | Invalidate activity to unknown; never convert clear to idle/closed |
| Unsupported/missing/malformed signal | No admitted native snapshot | Unknown activity, not an invented working/idle/permission/completion |

Official TUI [`chatwidget/tests/terminal_program_status.rs`](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/tui/src/chatwidget/tests/terminal_program_status.rs)
checks Idle→Working→Blocked→Working→Idle and background exec transitions.
[`iterm_session_status_tests.rs`](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/tui/src/iterm_session_status_tests.rs)
checks exact wire bytes and ordinary TTY/tmux/screen support. OSC 7501 is emitted
without enabling hooks or changing terminal-title/status-line settings. Internal
app-server/rollout events are not treated as externally subscribed TUI events.

### Minimal implementation and verification plan

1. Add a passive OSC 7501 parser consumer with a strict exact-payload allowlist;
   retain only working/blocked/idle/unknown. Do not parse arbitrary terminal text,
   titles, prompts, reasons or transcript bodies.
2. Bind publication and disposal to the existing exact request/run/generation and
   xterm parser/stream lifetime. Register after the existing generation parser
   drain/reset fence and before output admission. Starting receipts remain
   pending until running; unknown/failure/end/restart/close/disposal invalidate.
   Hidden sessions keep observing; focus never determines activity.
3. Project Codex terminal snapshots separately from authenticated Claude hooks,
   preserving existing icon/attention/closed priority and activity-driven catalog
   refresh. The stream is not cryptographically authenticated: child output can
   spoof it, so it remains a visual hint with no process/input/permission/history
   authority and no completion/unread/taskbar receipt side effect.
4. RED/GREEN through real xterm parser, store, adapter and row; exercise chunking,
   blocked/idle/clear, malformed/foreign-app signals, stale/closed owners,
   restart/parser drain, hidden sessions and Claude isolation. Verify actual
   installed CLI output with isolated home/project and a local synthetic provider
   where feasible; never use a real account or paid inference.

This is a bounded extension of the existing terminal/status flow, authorized by
the user's explicit instruction to record the plan then implement. No new CLI,
daemon/transport takeover, global configuration, launch flag or npm wrapper change.

## Follow-up implementation and evidence

The first green-only repair was insufficient. The missing source was Desk's
OSC 7501 consumer, not a lack of native Codex lifecycle support. The follow-up
adds a passive xterm handler, exact-attempt store field, Native adapter projection,
owner-change catalog refresh and localized Idle / Needs action labels. It changes
no Rust launch code, command validation, npm wrapper hook injection, process/input
ownership or global configuration. Claude and raw terminals do not subscribe.

### RED / GREEN and regression checkpoints

- RED: admitted native working bytes still projected a neutral process-running
  row; adding the parser/store/projection made the same real-xterm path green.
- RED: changing the new field did not refresh the catalog; adding it to the
  existing owner watcher fixed the stale row without history reads or other-project
  refresh. Repeating a state does not replace the catalog array.
- RED: an unknown Codex protocol state retained old working; strict decoding now
  invalidates unknown Codex payloads instead of preserving a stale hint.
- Real pinned xterm tests: 45 passed, including English/Chinese row labels,
  split OSC payload/terminator, foreign apps, unknown state/fields, clear, hidden
  terminals, multiple Codex owners, Claude/raw isolation, output gaps, disposal
  and queued old Codex OSC held across the existing restart parser fence.
- `npm run test:fast`: 182 files / 2926 tests passed. The first expanded run
  exposed 62 outdated Idle label assertions; the final run includes those updates.
- `npm run test:process`: 14 files / 124 passed / 2 existing skipped. This
  includes real subprocess and observer-reporter safety tests, not a live-provider
  certification claim.
- `npm run test:stress`: 2 passed, with the complete 5,000-session and 30-owner /
  120-change workloads retained.
- `npm run test:visual:unit`: 2 files / 66 passed. Its first run exposed one
  previous green-badge repair assertion still expecting no Codex running badge.
- `npm run typecheck`, `npm run build` and `git diff --check` passed. Existing
  npm-config/Vite CJS/chunk-size warnings remain. Production Rust sources and Tauri APIs are unchanged;
  no full Rust harness, signed build, installation or CI run was performed.
- Chrome visual checks use the existing installed executable, with no installation
  or snapshot update. The first follow-up browser run had two incorrect expectations
  treating the synthetic `waiting_input` sheet as native generic blocked. Those
  fixture-specific assertions were corrected; native blocked's Needs action label
  is separately verified by the real-xterm/store/adapter/row tests.
- Final `PLAYWRIGHT_CHROMIUM_EXECUTABLE=<installed Chrome> npx playwright test
  tests/visual/session-app-icon.spec.ts`: 5 passed, exit 0. Reviewed the retained
  light/dark Codex PNGs; synthetic status shapes, idle dot, closed treatment and
  fixed icon slot remain intact. Both owned fixture servers were stopped after
  their runs; no background test server is intentionally retained.
- Read-only review found no code blocker and confirmed the added old-OSC restart
  assertion. Its stale-ledger finding was resolved by explicitly labeling the
  first repair as history and recording this follow-up separately.

### Actual Windows CLI byte capture

An isolated local probe ran the installed **codex-cli 0.163.0** npm wrapper through
portable-pty and the actual production `conpty_runtime::initialize` implementation.
The already-prepared bundled runtime from the primary checkout was copied beside
the probe; production hash/size/architecture/held-file checks remained intact.
`prepare-conpty.mjs` could not download in this worktree, so no download success
is claimed. No repository Rust implementation was changed for this probe.

The probe cleared inherited environment, used temporary CODEX_HOME, HOME,
USERPROFILE, APPDATA, LOCALAPPDATA and project paths, and pointed a custom model
provider only at an ephemeral loopback HTTP server with no authentication.
Native `/plan` plus a synthetic `request_user_input` response opened a real TUI
question. Escape canceled it; `/quit` exited. Two loopback provider requests
produced this captured native OSC sequence:

```text
state=idle:app=codex
state=working:app=codex
state=blocked:app=codex
state=working:app=codex
state=idle:app=codex
state=clear
```

The final probe exited 0 with idle/working/blocked/cleared all true. Earlier
failed attempts are retained: missing Node on isolated PATH; removed `untrusted`
policy; a pasted Enter interpreted as a newline; and a full-access shell fixture
that did not request approval. None was counted as passing. The last shell fixture
executed only `Write-Output codex-status-fixture` inside the disposable project;
the passing question fixture performs no shell tool call.

Local ignored evidence (not committed binaries/raw terminal logs):

- `test-results/codex-status-smoke/src/main.rs`, Cargo manifest/lock and raw captures.
- `report-final.json` SHA256 `230342af5ec9f211c4dfc6cb3b01b8c74506aa0dd972fc1ffde76f94d607040f`.
- `report-final.json.raw` SHA256 `7a45cf251ae8aaec07a173ee6f2ed52fece493190b306b6b1cda12dd1271bc5c`.
- Probe source SHA256 `2ddb617efcf8c408646c305b554e5cfc42ce4c221a100947deb9d83a6611d92b`.
- Installed native `codex.exe` SHA256 `2692c8c7bb2cf8b0aefe638741fc90236f4b824c59578ce84994f9de58200993`.

This verifies real CLI-to-Windows-ConPTY bytes with a synthetic provider. The
separate real-xterm component tests verify transport-to-row behavior with synthetic
IPC. They do not constitute one installed Desk WebView2 end-to-end run. No real
account, paid inference, OS taskbar, user approval dialog, macOS/Linux, older Codex
version or affected-user machine acceptance was run. OpenCode exists on another
unintegrated branch: it was untouched and cannot be runtime-certified here.
0.163.0 does not expose precise permission/completion through this protocol;
unknown future states/fields degrade to unknown rather than claiming parity with
Claude. Terminal bytes remain spoofable visual hints, never authorization facts.

## Initial green-badge repair history (superseded by the follow-up above)

### Scope and repository checkpoint

- User-directed worktree: `E:\source\github\cc-box\.claude\worktrees\active-session-filter`.
- Branch: `feat/active-session-filter`, clean baseline `61ec2806c360730f0fc393941b67bde776e4fe3b`, based on `614d1e86cbae26c1844eb37f36cfe0e66b0a1554` (`origin/main` cache, package version 0.23.5).
- The primary checkout remains on `fix/opencode-backend` with pre-existing uncommitted work; no changes were migrated or overwritten. No new worktree or repair branch was successfully created.
- Remote main could not be refreshed: SSH known_hosts read failed; HTTPS Schannel reported `SEC_E_NO_CREDENTIALS`. No remote state is inferred from the cache.
- Library screenshot `libfile_4bb47d1b4cb08191a87c58103110a2bd` returned image metadata (169×66) and OCR `Codex CLI; 运行中`, consistent with the neutral running projection. Pixel materialization failed in this environment. The reported installed version and affected machine remain unconfirmed.

### Initial root cause and minimal repair

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

### Initial verification checkpoints

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

### Initial evidence boundary

Launch receipts and hook identities in these tests are synthetic host inputs.
No live account, real CLI turn, installed Windows WebView, OS taskbar or affected
machine screenshot validation was implied by that first repair. Its assumption
that Codex had no available working/idle producer was corrected by the official
tag investigation and native OSC byte capture documented above. Stop is still
not completion; precise permission and completion remain unsupported here.
