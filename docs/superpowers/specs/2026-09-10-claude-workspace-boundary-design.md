# CC Desk Claude Workspace Boundary Design

## 1. Goal

CC Desk is a lightweight desktop workspace for Claude Code power users. It hosts the real Claude Code CLI and adds the capabilities that a single terminal does not provide well: multi-project organization, multi-session operation, fast switching, status overview, and attention notifications.

CC Desk is downstream of cc-switch. cc-switch owns provider, authentication, endpoint, and routing configuration. Claude Code owns conversation, permissions, tools, MCP execution, skills, agents, plugins, and native session semantics. CC Desk must not duplicate either product's configuration authority or runtime.

## 2. Product Contract

A user who can complete an operation in a native Claude Code terminal should be able to complete the same operation in a CC Desk terminal under the same executable, working directory, environment, and arguments.

CC Desk enhancements are optional. If history indexing, a sidebar projection, or hook monitoring fails, the underlying Claude process and terminal session continue running.

### 2.1 Responsibilities

| Component | Owns |
|---|---|
| cc-switch | Providers, API credentials, endpoints, routing, optional proxy lifecycle |
| Claude Code | Interactive CLI, permissions, model behavior, MCP runtime, skills, agents, plugins, native sessions |
| CC Desk | Project metadata, workspace tabs, PTY transport, cross-project navigation, status presentation, local notifications |

### 2.2 Data ownership

CC Desk may write only its own workspace metadata under `~/.cc-box/` and explicit local UI preferences. It must not write provider state, move Claude resources to private disable folders, edit `~/.claude/settings.json`, edit `~/.claude.json`, or delete native Claude transcripts as an incidental workspace action.

Legacy CC Desk files such as `providers.json` are left untouched during migration. Removing support must not delete or rewrite user data.

### 2.3 Integration with cc-switch

The integration is deliberately loose:

```text
cc-switch -> Claude native configuration or local proxy
CC Desk -> PTY -> installed Claude Code CLI
```

CC Desk does not read the cc-switch SQLite schema, call its private APIs, copy providers, or proxy model traffic. Each new Claude process inherits the effective environment and native configuration available at launch.

## 3. In Scope

- Native PTY-backed Claude Code terminals.
- Multiple projects and multiple terminal tabs in one window.
- Resume and launch workflows that delegate semantics to the installed CLI.
- Project pinning, aliases, hiding, and workspace-only session organization.
- Read-only projection of native resources where the projection does not start or mutate the resource.
- Optional hook-based working/waiting/attention signals.
- CC Desk's own signed application updater.
- Environment diagnostics for the installed Claude executable and required platform dependencies.

## 4. Out of Scope

The following fork-era features are removed:

- Provider presets, CRUD, activation, common configuration, connection testing, failover metadata, and cc-switch database import.
- Claude CLI or Git binary mirroring, version catalogues, downloading, overwrite installation, and process-wide Claude termination.
- GUI enable/disable operations that move skills or agents, edit MCP configuration, or call plugin enable/disable.
- A second MCP client that starts or connects to MCP servers to inspect tools, prompts, or resources.
- Deleting Claude native session files from a workspace organization action.

Read-only lists may still show that a native resource is enabled or disabled according to the data reported by Claude Code. CC Desk does not own or change that state.

## 5. Core Architecture

### 5.1 Stable core

```text
Vue workspace UI <-> typed Tauri IPC <-> PtyManager <-> portable-pty <-> Claude CLI
```

The stable core does not parse Claude screen output and does not depend on private Claude APIs. Arguments remain structured until the last platform-specific launch boundary.

### 5.2 Optional monitoring

```text
Claude hook -> local loopback receiver -> hook event bus -> status presentation
```

Monitoring is advisory. PTY spawn success establishes that the terminal process exists. Absence of `SessionStart` changes monitoring state to unavailable; it never kills a live PTY and never triggers an automatic duplicate launch.

The UI models process state separately from activity state:

- Process: starting, running, stopped.
- Activity: working, waiting, unknown.
- Monitoring: connecting, active, unavailable.

### 5.3 Read-only native projection

Skills, agents, MCP configurations, and plugins are read from native files or public CLI output. The projection must not start an MCP server, test credentials, or modify native configuration. Failure returns an unavailable/partial panel while terminals remain usable.

## 6. PTY Lifecycle Contract

1. The frontend allocates the PTY ID and installs its `ptyId -> tabId` routing before invoking spawn.
2. The backend validates and uses that ID, so output and exit events cannot precede frontend routing.
3. The parent drops the slave handle immediately after spawning the child.
4. Backend instance state is registered before the reader thread starts.
5. Natural EOF reaps the child, records the actual exit code/signal, removes backend maps, and emits exactly one exit event.
6. Explicit kill removes the instance first, terminates and waits for the child, clears the writer, and emits exactly one killed event.
7. A blocked input write for one PTY must not hold a global writer lock needed by another PTY or by shutdown.

This design keeps one lock for the instance map and one independent lock per PTY writer. The map lock is held only for lookup/insert/remove, never during blocking I/O or sleeps.

## 7. Argument Contract

Known and custom Claude arguments must preserve argument boundaries. CC Desk does not split a free-form string with a whitespace regex and then concatenate it unsafely into a shell command.

For the current UI, custom arguments are parsed with a small shell-like tokenizer supporting whitespace, single quotes, double quotes, and backslash escapes. The resulting `string[]` crosses Tauri IPC. At the final shell boundary each argument is quoted using the selected shell's rules.

Unknown arguments are passed through unchanged so future Claude Code flags do not require a CC Desk release.

## 8. Configuration-root compatibility

Native resource discovery uses the effective Claude configuration directory. The backend resolves `CLAUDE_CONFIG_DIR` from the launched process environment and falls back to `~/.claude` only when it is absent. Cache identities include the resolved configuration root where native data is involved.

CC Desk may continue to store its own UI data under `~/.cc-box/` for backward compatibility.

## 9. Migration

- Do not delete `~/.cc-box/providers.json` or old downloaded files.
- Stop reading, displaying, activating, or mutating provider data.
- Stop injecting provider-specific state owned by deleted screens. User-defined runtime environment variables remain supported as explicit launch preferences.
- Remove unavailable settings sections from navigation. If a stored settings-section ID no longer exists, reset it to `appearance`.
- Existing project and terminal metadata remain compatible.

## 10. Testing and Release Gates

Required automated coverage:

- A product-boundary test fails if removed provider/installer/MCP-runtime entry points return.
- Hook timeout resolves to monitoring-unavailable without invoking PTY kill.
- A PTY ID is registered before spawn and removed after failed spawn.
- Shell-like custom argument parsing preserves quoted values and rejects unterminated quotes.
- Rust tests cover PTY ID validation, per-writer isolation helpers, argument quoting, and natural exit status extraction.
- Existing frontend and Rust suites remain green.

Required manual smoke tests on Windows, macOS, and Linux:

- Launch a fake CLI that prints immediately and exits non-zero; output is complete and exit code is correct.
- Launch two sessions; block input consumption in one and verify the other remains writable and application shutdown completes.
- Disable or break hook reporting; the Claude terminal remains running with monitoring marked unavailable.
- Run with `CLAUDE_CONFIG_DIR` pointing to a non-default root; project/session/resource discovery follows that root.

## 11. Acceptance Criteria

- No Provider screen, Provider IPC command, Provider Rust module, cc-switch database import, or Provider connection test remains.
- No bundled Claude/Git download, version catalogue, overwrite installer, or kill-all-Claude command remains.
- Skills, agents, MCP, and plugin panels are read-only and do not launch MCP servers for inspection.
- A missing hook cannot terminate a running Claude process.
- Immediate PTY output and rapid process exit are routable before spawn returns.
- CC Desk continues to run the installed Claude CLI and forwards future unknown flags without a product-specific allowlist.
- CI passes and the change is delivered through a protected pull request.