# Claude Workspace Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refocus CC Desk on reliable multi-project, multi-session Claude Code operation by removing downstream-duplicate configuration runtimes and making monitoring optional.

**Architecture:** Keep the Vue/Tauri/portable-pty execution path as the stable core. cc-switch remains the upstream configuration owner; Claude Code remains the native capability owner. Provider management, mirrored CLI installation, mutating native-resource controls, and the independent MCP client are removed. PTY routing is established before spawn, natural exits are reaped centrally, and hook failure degrades status only.

**Tech Stack:** Vue 3, TypeScript, Pinia, Vitest, Tauri 2, Rust 2021, portable-pty 0.8.

**Spec:** `docs/superpowers/specs/2026-09-10-claude-workspace-boundary-design.md`

## Global Constraints

- Work only on `codex/focus-claude-workspace`; do not modify `main` directly.
- Preserve existing files in users' `~/.cc-box/` directories; migration never deletes legacy Provider data.
- CC Desk must not write Provider state or mutate native Claude resource configuration.
- Hook monitoring failure must not kill or restart a live Claude PTY.
- Unknown Claude CLI arguments must continue through the launch path.
- Existing protected checks `Frontend checks` and `Rust checks` must pass before the PR is ready.
- Changes use test-first red/green cycles where executable behavior changes.

---

### Task 1: Add product-boundary regression gate

**Files:**
- Create: `tests/productBoundary.test.ts`

**Interfaces:**
- Consumes: repository source tree through Node `fs`.
- Produces: a regression gate that rejects Provider, bundled installer, mutating resource, and independent MCP-runtime entry points.

- [ ] **Step 1: Write the failing test**

```ts
import { existsSync, readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

const read = (path: string) => readFileSync(path, 'utf8')

describe('CC Desk product boundary', () => {
  test('does not ship downstream Provider management', () => {
    expect(existsSync('src-tauri/src/providers.rs')).toBe(false)
    expect(existsSync('src/api/provider.ts')).toBe(false)
    expect(existsSync('src/stores/providers.ts')).toBe(false)
    expect(existsSync('src/types/provider.ts')).toBe(false)
    expect(existsSync('src/config/providerPresets.ts')).toBe(false)
    expect(read('src-tauri/src/lib.rs')).not.toContain('commands::activate_provider')
  })

  test('does not distribute or overwrite Claude and Git binaries', () => {
    expect(existsSync('src-tauri/src/installer.rs')).toBe(false)
    const api = read('src/api/tauri.ts')
    expect(api).not.toContain('downloadAndInstallClaude')
    expect(api).not.toContain('killClaudeProcesses')
    expect(api).not.toContain('listClaudeVersions')
  })

  test('native capability panels are projection-only', () => {
    const commands = read('src-tauri/src/lib.rs')
    expect(commands).not.toContain('commands::set_skill_enabled')
    expect(commands).not.toContain('commands::set_agent_enabled')
    expect(commands).not.toContain('commands::set_mcp_server_enabled')
    expect(commands).not.toContain('commands::set_plugin_enabled')
    expect(commands).not.toContain('commands::get_mcp_server_detail')
    expect(existsSync('src-tauri/src/mcp.rs')).toBe(false)
  })
})
```

- [ ] **Step 2: Push the test-only commit and verify RED in the draft PR**

Expected: `Frontend checks` fails because the fork-era files and commands still exist.

- [ ] **Step 3: Keep this gate in the final suite**

The test is policy-level by design: it prevents later AI-assisted changes from silently reintroducing responsibilities assigned to cc-switch or Claude Code.

---

### Task 2: Remove Provider management end to end

**Files:**
- Modify: `src/components/settings/SettingsView.vue`
- Modify: `src/stores/sidebar.ts`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`
- Delete: `src-tauri/src/providers.rs`
- Delete: `src/api/provider.ts`
- Delete: `src/stores/providers.ts`
- Delete: `src/types/provider.ts`
- Delete: `src/config/providerPresets.ts`
- Delete: `src/components/settings/sections/ProvidersSection.vue`
- Delete: `src/components/settings/providers/CommonConfigPanel.vue`
- Delete: `src/components/settings/providers/ProviderCard.vue`
- Delete: `src/components/settings/providers/ProviderEditPanel.vue`
- Delete: `src/components/settings/providers/ProviderList.vue`
- Delete: `src/components/settings/providers/ProviderPresetPanel.vue`
- Delete: `tests/stores/providers.test.ts`
- Delete: `tests/config/providerPresets.test.ts`
- Delete: `docs/provider-management.md`
- Delete: `docs/provider-test-cases.md`

**Interfaces:**
- Consumes: existing settings navigation and Tauri command registry.
- Produces: settings UI with no Provider section and backend with no Provider IPC surface.

- [ ] **Step 1: Remove the Provider navigation item and component import**

`SettingsView.vue` must render only appearance, startup, shortcuts, update, and about sections.

- [ ] **Step 2: Normalize stale settings navigation**

In `openSettings(section?)`, accept only:

```ts
export type SettingsSection = 'appearance' | 'startup' | 'shortcuts' | 'update' | 'about'
```

If a stored or caller-provided section is not in that set, assign `appearance`.

- [ ] **Step 3: Remove Rust module/imports/commands and frontend Provider files**

Do not migrate or delete user `providers.json` files. Removing code support is the complete migration action.

- [ ] **Step 4: Run the product-boundary test**

Expected: Provider assertions pass; other Task 1 assertions remain red until later tasks.

- [ ] **Step 5: Commit**

```bash
git commit -m "refactor: remove downstream provider management"
```

---

### Task 3: Remove bundled Claude/Git distribution and keep diagnostics

**Files:**
- Modify: `src/App.vue`
- Modify: `src/components/settings/sections/UpdateSection.vue`
- Modify: `src/stores/update.ts`
- Modify: `src/stores/sidebar.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/types/app.ts`
- Modify: `src-tauri/src/lib.rs`
- Delete: `src-tauri/src/installer.rs`
- Delete: `tests/stores/update.test.ts` and recreate it for the reduced store

**Interfaces:**
- Consumes: existing `checks.rs`, Tauri updater plugin, `checkForUpdates`.
- Produces: retryable environment diagnostics and CC Desk self-update only.

- [ ] **Step 1: Write reduced update-store tests**

```ts
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, test } from 'vitest'
import { useUpdateStore } from '@/stores/update'

describe('update store', () => {
  beforeEach(() => setActivePinia(createPinia()))

  test('tracks only CC Desk update download state', () => {
    const store = useUpdateStore()
    store.setDownloadState('downloading')
    store.setDownloadProgress({ downloaded: 25, total: 100, percent: 25 })
    expect(store.downloadState).toBe('downloading')
    expect(store.downloadProgress.percent).toBe(25)
    expect('claudeVersionList' in store).toBe(false)
  })
})
```

- [ ] **Step 2: Verify RED**

Expected: the test fails because the legacy Claude version state remains.

- [ ] **Step 3: Remove auto-install UI and backend commands**

The environment failure overlay keeps `Retry` and presents each check's detected path/action. It no longer downloads binaries or kills all Claude processes.

- [ ] **Step 4: Rewrite `UpdateSection.vue` for CC Desk only**

Retain update checking, signed download/install, active-PTY confirmation, progress, manual release link, and errors. Remove all Claude CLI version catalogue/install controls.

- [ ] **Step 5: Reduce API/types/store**

Remove `DownloadProgress` only if no longer used by the self-updater; retain the self-updater's `downloaded/total/percent` type. Remove `ClaudeCliUpdateInfo`, `ClaudeVersionEntry`, `ClaudeVersions`, installer events, and related state.

- [ ] **Step 6: Run focused tests and typecheck**

Expected: reduced update test passes; product-boundary installer assertions pass.

- [ ] **Step 7: Commit**

```bash
git commit -m "refactor: remove bundled Claude distribution"
```

---

### Task 4: Make native capability panels read-only

**Files:**
- Modify: `src/stores/sidebar.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/types/config.ts`
- Modify: `src/components/skills/SkillItem.vue`
- Modify: `src/components/agents/AgentItem.vue`
- Modify: `src/components/mcp/McpItem.vue`
- Modify: `src/components/mcp/McpPanel.vue`
- Modify: `src/components/plugins/PluginItem.vue`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/store.rs`
- Delete: `src-tauri/src/mcp.rs`
- Delete: `src/components/mcp/McpSubItem.vue`
- Delete: `tests/api/setEnabled.test.ts`

**Interfaces:**
- Consumes: read-only `get_all_skills`, `get_all_agents`, `get_all_mcp_servers`, and `get_all_plugins`.
- Produces: projection-only panels; skill/agent invocation still writes text into the active terminal because Claude Code interprets that text.

- [ ] **Step 1: Remove all mutation imports and store actions**

`sidebar.ts` exposes load and panel functions only. It does not expose `toggleSkillEnabled`, `toggleAgentEnabled`, `toggleMcpServerEnabled`, or `togglePluginEnabled`.

- [ ] **Step 2: Remove toggle controls from item components**

Keep disabled visual state when reported by native data, but do not offer a control that changes it. A disabled skill/agent/plugin cannot be invoked through the convenience button.

- [ ] **Step 3: Replace MCP live probing with static configuration detail**

`McpItem.vue` expands locally to show source, transport type, URL or command, and arguments. Never render headers or environment values because they may contain credentials.

- [ ] **Step 4: Remove independent MCP Rust runtime and mutation commands**

Delete `mcp.rs`, `get_mcp_server_detail`, and the four enable/disable commands. Remove now-unused helper functions from `store.rs` only when they are no longer referenced by tests or other code.

- [ ] **Step 5: Run the product-boundary test**

Expected: all three Task 1 test cases pass.

- [ ] **Step 6: Commit**

```bash
git commit -m "refactor: make Claude capability panels read only"
```

---

### Task 5: Decouple process startup from hook monitoring

**Files:**
- Modify: `src/composables/useSessionStartWaiter.ts`
- Modify: `src/components/TerminalView.vue`
- Modify: `tests/composables/sessionStartWaiter.test.ts`
- Modify: `src/i18n/locales/en.ts`
- Modify: `src/i18n/locales/zh.ts`

**Interfaces:**
- Consumes: PTY spawn result and optional `SessionStart` hook.
- Produces: `Promise<'monitored' | 'unavailable'>` for successful process startup monitoring.

- [ ] **Step 1: Write failing state-machine tests**

```ts
expect(reduceWaiter('waiting', { type: 'timeout' })).toBe('unavailable')
expect(isStartupFailure('unavailable')).toBe(false)
expect(isStartupFailure('exited')).toBe(true)
```

- [ ] **Step 2: Verify RED**

Expected: `unavailable` and `isStartupFailure` do not exist.

- [ ] **Step 3: Implement the new state model**

```ts
export type WaiterStatus =
  | 'waiting'
  | 'started'
  | 'unavailable'
  | 'exited'
  | 'failed'
  | 'cancelled'

export function isStartupFailure(status: WaiterStatus): boolean {
  return status === 'exited' || status === 'failed' || status === 'cancelled'
}
```

Timeout transitions to `unavailable`. `settleWaiter` resolves both `started` and `unavailable`; it rejects only actual process/spawn/unmount failures.

- [ ] **Step 4: Remove timeout kill/retry behavior**

`TerminalView.startProjectSession` must not call `ptyKill` or dispose the terminal after a hook timeout. Persist `lastOpened` after either monitored or unavailable startup. Show a non-blocking monitoring-unavailable hint for the active startup.

- [ ] **Step 5: Run focused tests**

Expected: state-machine tests pass and no test expects `STARTUP_TIMEOUT_CODE`.

- [ ] **Step 6: Commit**

```bash
git commit -m "fix: treat hook monitoring as optional"
```

---

### Task 6: Route PTY events before spawn returns

**Files:**
- Modify: `src/api/tauri.ts`
- Modify: `src/components/XTermTerminal.vue`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/pty.rs`
- Modify: `src-tauri/src/tests/pty.rs`

**Interfaces:**
- Consumes: frontend-generated UUID `PtySpawnOptions.id`.
- Produces: backend events using the pre-registered ID.

- [ ] **Step 1: Write failing Rust UUID tests**

```rust
#[test]
fn pty_id_accepts_uuid() {
    let id = "550e8400-e29b-41d4-a716-446655440000";
    assert_eq!(validate_pty_id(id).unwrap(), id);
}

#[test]
fn pty_id_rejects_arbitrary_text() {
    assert!(validate_pty_id("../../session").is_err());
}
```

- [ ] **Step 2: Verify RED**

Expected: `validate_pty_id` is undefined.

- [ ] **Step 3: Add required spawn ID to IPC**

```ts
interface PtySpawnOptions {
  id: string
  cwd: string
  cols: number
  rows: number
  type: 'claude' | 'shell'
  args?: string[]
}
```

The backend validates the UUID and passes it to `spawn_claude`/`spawn_shell`; those methods no longer generate their own IDs.

- [ ] **Step 4: Pre-link frontend routing**

Before `await ptySpawn(...)`, generate `const ptyId = crypto.randomUUID()`, assign it to the terminal instance, call `ptyToTab.link(ptyId, tabId)`, and mark the tab starting. On spawn failure, unlink it during `discardUnstartedTab`.

- [ ] **Step 5: Run Rust tests and frontend typecheck**

Expected: preallocated ID is used end to end.

- [ ] **Step 6: Commit**

```bash
git commit -m "fix: register PTY routing before spawn"
```

---

### Task 7: Reap natural PTY exits and isolate writers

**Files:**
- Modify: `src-tauri/src/pty.rs`
- Modify: `src-tauri/src/tests/pty.rs`

**Interfaces:**
- Consumes: `portable_pty::MasterPty`, child handle, reader, and writer.
- Produces: backend maps that contain only live PTYs and actual natural exit codes.

- [ ] **Step 1: Add a writer-map isolation test around the new entry type**

Create `PtyWriterEntry` with `writer: Mutex<Box<dyn Write + Send>>`. Test that map lookup releases the outer map lock before `write_pty_data` is invoked through a test writer.

- [ ] **Step 2: Replace stored `PtyPair` with master-only state**

```rust
struct PtyInstanceData {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send>,
    status: PtyStatus,
}
```

Destructure `PtyPair`, spawn using `slave`, then immediately `drop(slave)`.

- [ ] **Step 3: Register state before starting the reader**

Insert the instance and per-PTY writer first. Start the reader thread only after both maps contain the ID. Remove the artificial 50 ms sleep.

- [ ] **Step 4: Reap on EOF**

On EOF, flush decoder output, remove the instance and writer from the manager, call `child.wait()`, and emit actual `status.exit_code()` and `status.signal()`. If explicit kill already removed the instance, emit nothing from the EOF path.

- [ ] **Step 5: Avoid blocking I/O under global map locks**

`write()` clones an `Arc<PtyWriterEntry>` while holding the map lock, releases the map lock, then holds only that PTY's writer lock during write/flush/chunk delay. `kill_all()` drains instance and writer maps before waiting on child processes.

- [ ] **Step 6: Run Rust tests**

Expected: PTY unit and transport tests pass; no duplicate exit events in the fake-CLI smoke harness.

- [ ] **Step 7: Commit**

```bash
git commit -m "fix: make PTY lifecycle deterministic"
```

---

### Task 8: Preserve custom argument boundaries

**Files:**
- Create: `src/utils/commandArgs.ts`
- Create: `tests/utils/commandArgs.test.ts`
- Modify: `src/stores/app.ts`
- Modify: `src/components/XTermTerminal.vue`
- Modify: `src-tauri/src/platform.rs`
- Modify: `src-tauri/src/pty.rs`
- Modify: `src-tauri/src/tests/platform.rs`

**Interfaces:**
- Produces: `parseCommandArgs(input: string): string[]` and `quote_shell_arg(arg: &str, shell: ClaudeShellKind) -> String`.

- [ ] **Step 1: Write failing TypeScript parser tests**

```ts
expect(parseCommandArgs('--model sonnet --name "hello world"')).toEqual([
  '--model', 'sonnet', '--name', 'hello world'
])
expect(parseCommandArgs("--label 'a b'")).toEqual(['--label', 'a b'])
expect(() => parseCommandArgs('--name "unterminated')).toThrow(/unterminated/i)
```

- [ ] **Step 2: Verify RED**

Expected: module does not exist.

- [ ] **Step 3: Implement the minimal tokenizer and use it at both call sites**

Replace every `trim().split(/\s+/)` custom-argument path with `parseCommandArgs`.

- [ ] **Step 4: Write failing Rust quoting tests**

Cover empty strings, spaces, apostrophes for bash, and apostrophes for PowerShell. The launcher must quote the executable path, every argument, and the plugin directory independently.

- [ ] **Step 5: Build the shell command from structured arguments**

Do not call `extra_args.join(" ")`. `get_claude_shell` receives a command string assembled only by the platform quoting helper.

- [ ] **Step 6: Run focused tests**

Expected: TypeScript parser and Rust platform tests pass.

- [ ] **Step 7: Commit**

```bash
git commit -m "fix: preserve Claude CLI argument boundaries"
```

---

### Task 9: Align documentation and clean references

**Files:**
- Modify: `README.md`
- Modify: `README_CN.md`
- Modify: `PRODUCT.md`
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md`
- Modify: `docs/capabilities.md`
- Modify: `docs/components.md`
- Modify: `docs/data-persistence.md`
- Modify: `docs/hook-monitor.md`
- Modify: `docs/terminal-integration.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: one authoritative product direction for humans and coding agents.

- [ ] **Step 1: Remove Provider and mirrored-installer claims**

State that cc-switch is the upstream Provider/configuration owner and that CC Desk neither requires nor reads cc-switch internals.

- [ ] **Step 2: Document read-only panels and optional monitoring**

Explicitly state that panel or hook failures do not block terminal operation.

- [ ] **Step 3: Update architecture/file maps**

Remove deleted files and commands. Describe frontend-preallocated PTY IDs and deterministic exit reaping.

- [ ] **Step 4: Add migration notes**

Legacy user files are retained but ignored; users manage Provider configuration through cc-switch or Claude Code native mechanisms.

- [ ] **Step 5: Commit**

```bash
git commit -m "docs: align project with Claude workspace scope"
```

---

### Task 10: Full verification and pull request completion

**Files:**
- Modify as required by verified failures only.

- [ ] **Step 1: Run frontend verification**

```bash
npm ci
npm run typecheck
npm run test:ci
npm run build
```

Expected: all commands exit 0.

- [ ] **Step 2: Run Rust verification**

```bash
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: all commands exit 0.

- [ ] **Step 3: Inspect the PR diff**

Confirm no legacy Provider or installer entry point remains; confirm no unrelated generated or user data is included.

- [ ] **Step 4: Review ignored tests**

Record any platform/manual tests that cannot run in GitHub Actions. Do not report them as passing.

- [ ] **Step 5: Mark the draft PR ready only after protected checks pass**

The PR remains unmerged for owner review.