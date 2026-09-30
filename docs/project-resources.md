# Unified project resources (Task 15)

The normal App context slot renders six read-only categories: project instructions, configuration, MCP, skills, agents, and plugins. The existing AppShell owns docking/overlay behavior, width, focus and close controls. No second global drawer or terminal tab system is introduced. Opening or refreshing resources cannot create, stop, resume, restart or mutate a CLI/resource.

## Ownership and integration

`useProjectResourcesStore` derives `ProjectResourceContext` from the actual selected unified session and owning runtime store. It loads only while the workspace resource drawer is visible. No selected session means no resource authority, even if a project or default launch configuration is selected elsewhere.

- An owning Native tab with an admitted/active attempt uses its exact run/generation. Scope failures, malformed identities and revoked scope never fall back to current defaults. The returned CLI, profile ID and frozen profile revision must match that tab.
- A freshly created/restarted stopped tab may use its exact saved configuration revision plus matching registered project only while the runtime store still holds positive unstarted proof for that exact request/run/generation. The private proof is revoked before start preparation and upon known/unknown launch evidence; failed/null receipts never prove non-submission. Explicit historical Native sessions use their saved origin. A missing/changed configuration or project is unavailable; it is not silently upgraded. A disappeared non-history tab cannot turn into profile authority.
- A running tab retains its launch snapshot when today's configuration changes. It does not switch to the latest configuration.
- Request ownership includes unified session ID, project, CLI, runtime, configuration revision, run/request/generation (or Legacy PTY ID/generation), and resource category. Scope continuations, read completions, errors and finally handlers all respect selection/attempt ownership. Closing/hiding the drawer stops publication; changing ownership clears old items synchronously.
- Refresh preserves previously successful data only for exactly the same owner and category, with a stale/loading notice. Failed refreshes keep that same-owner data explicitly stale. Switching owner/category clears it before the new read.

`nativeTabs.hasUnstartedAttempt` checks the private exact-attempt proof created only by create/restart. This changes resource authority selection only; runtime retry and terminal I/O behavior are unchanged.

`nativeProjection.readScoped` is an independent, authenticated single-page read; it does not use or clear the compatibility panel's mutable projection slot. Existing native transport validation and backend authority stay unchanged.

## Display DTO and privacy

`ProjectResourceItem` contains kind-specific display fields only. Settings names are restricted to model, language, outputStyle, model_reasoning_effort, approval_policy and sandbox_mode. Origin becomes a fixed localized category. Plugin IDs, source handles, paths, URLs, commands, argv, environment maps, headers, credentials, raw config and transport errors never enter the display DTO.

Free-text fields receive a conservative additional privacy screen: secret/credential/header/environment/token-looking strings, credential prefixes, environment assignments, path-like slash/backslash text, control/format characters, raw JSON-looking objects and unusually long opaque strings are withheld as whole fields. This can hide benign prose containing paths or these words. It is defense in depth for display, not a universal secret detector or an export sanitizer. `.claude/CLAUDE.md` is the one fixed, known relative instruction filename allowed verbatim. Text uses Vue interpolation only; no HTML, Markdown execution, external links or JSON dump.

## Bounded and unavailable observations

Native reads keep the existing strict typed contract: maximum 200 items in the requested page, maximum 2 MiB wire result, 16 KiB instruction text, 2 KiB descriptions, and the existing per-field validators. The drawer requests offset zero only. `hasMore` is visibly partial; there is no snapshot token to assert completeness across pages. Truncated document text is labeled. Ready-empty, partial, loading, stale and unavailable remain distinct. An empty partial page does not claim that the complete source is empty.

Legacy read-only readers lack launch-root identity and may mix ambient `~/.claude` records. Their new per-request methods take an explicit absolute project path and do not publish into compatibility sidebar caches. Settings and MCP use the existing `getProjectConfig` DTO and require exact internal `source.path` matches: selected-project `.claude/settings.json` or `.claude/settings.local.json` for settings, and selected-project `.mcp.json` for MCP. Missing, relative, ancestor and other-project source paths are omitted; source paths never enter the display DTO. The `getAllMcpServers` reader is deliberately not used because it labels ancestor `.mcp.json` observations as project without returning path provenance. Skills/agents retain only the existing exact-project reader observations; project plugins additionally require their exact matching `projectPath`. User/global/plugin-child ambient observations are excluded, even with the same name as a project item. The view always says project-only/partial. It describes file/configuration observations, not effective CLI precedence or live MCP status. Legacy instruction documents have no existing typed reader and are explicitly unavailable. No arbitrary filesystem reader, new protocol or default-home fallback was added.

## Verification boundary

Focused Vitest/jsdom tests cover structured DOM, all six categories, fixed labels, safe values, exact authority, refresh retention, late request/scope rejection and the normal App dock/overlay slot with terminal/selection preservation. These are frontend/host-contract evidence only. Actual installed Claude Code/Codex behavior, real Legacy CLI reader behavior, Windows 1024×640 at 100%/125%/150%, macOS/Linux rendering and terminal process/scrollback continuity remain unperformed platform acceptance. See the Task 15 checklist in `manual-test-cases.md`.
