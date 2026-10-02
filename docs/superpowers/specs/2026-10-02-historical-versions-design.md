# Historical versions and safe return — approved design

Goal: In Settings → Updates, select and install any available, verifiable official historical CC Desk version, with a recoverable path back. The current delivered source is 006fa33 (0.18.0). The first supported install target is the user's Windows x64 environment; other platforms must show their actual support status, not offer a nonfunctional install action.

## Approved product choice

Offer two data modes. “Keep current data” is available only when an explicit, reviewed compatibility rule covers the exact target's storage capabilities. Unknown compatibility is not inferred from semver or schema number. “Fresh settings, preserve current workspace for return” seals the current Desk-owned data and launches the historical version with its own fresh settings. The UI states which preferences/project metadata are not carried over. Returning restores the saved matching context; it does not merge incompatible writes. Native CLI history, credentials/configuration and project files remain shared and are not rolled back. Older provider-era app features can themselves change shared Claude settings; fresh Desk settings is not a sandbox for those old binaries.

The nine currently published releases (0.14.0–0.17.7) contain installers, detached signatures and SHA256 metadata, but no compatibility manifests. Their presence alone does not qualify them for shared-current-data mode. The fresh-settings mode is the approved way to make available historical versions usable without inventing compatibility.

## Release and download boundary

The backend queries only shawnwu2022/cc-desk's official release catalogue. Every observed version can be shown, with precise blocked reasons for missing platform assets, unsupported package format, missing/invalid signature, unavailable download or unknown packaging boundary. Drafts and workflow test artifacts never become public historical releases. Ordinary update/promotion policy stays disabled.

Selection uses a backend-owned release/asset identity, never a frontend URL or path. Revalidate the exact release, asset ID, size and digest before admission. Download bounded bytes into a private transaction directory; allow only official GitHub asset redirects. Verify the detached Minisign signature against the app's existing trusted public key, then SHA256/size and platform package identity. A checksum by itself does not establish publisher authenticity. Keep verified bytes and identity together; never install bytes newly supplied by the UI. Replaced/deleted assets require a fresh selection, not silent substitution.

## Persistent version/recovery manager

A copied, complete and hash-verified version-manager installation survives replacement of the main app. It starts in a restricted maintenance-only entry mode before normal runtime/profile/bootstrap, with separate WebView data and authenticated backend commands. It does not start CLI sessions. It retains the previous complete application bundle, verified historical package, exact state snapshots and a durable transaction journal. The manager remains accessible after installing an old version that lacks this feature. Returning to the original test build restores its verified local bundle; it does not promote that build to an official release or depend on a nonexistent published 0.18.0 installer.

Before destructive work, the manager requires every relevant CC Desk instance and backend-owned direct PTY child to have a positively observed terminal outcome. Background/unrelated CLI processes are not automatically terminated or restored; observed relevant Desk/WebView lock holders, or overlaps between a Desk snapshot root and configured CLI/project roots, block the switch. User review/cancel is non-destructive. New builds coordinate through a backend maintenance lease; old binaries require verified process/handle quiescence rather than assuming they honor a new lock. Unknown ownership or timeout blocks the switch. No force-kill is implicit.

Snapshots preserve the complete Desk-owned .cc-box root and the actual WebView data root, including unknown files, absence, exact bytes, hashes and relevant permissions. In particular, legacy providers.json and disabled/skills or disabled/agents are retained: the latter may contain user-authored content, not disposable cache. Size or unsupported-link limits block admission rather than silently omitting data. Generated cache/log/plugin data is retained with its context. Backups may contain sensitive legacy configuration: never upload or log their contents. No symlinks/reparse points or caller-provided restore paths are accepted. Restore happens only after all old lock holders/processes have exited and preserves the correct permanent-lock behavior.

## Initial Windows install scope

Admit a single verified, unelevated per-user Windows x64 NSIS installation at its registered original directory. The nine observed release tags use this scope. Explicitly block per-machine/HKLM, mixed-scope, WiX, multiple ambiguous installs and relocated unregistered copies until separately supported. Capture and restore the exact product uninstall/Explorer registration and app shortcuts alongside the complete return bundle; restoring only executable bytes is incomplete. Do not request elevation or silently alter installation scope.

## Supported entrypoint fence

The manager uses a distinct executable basename (cc-desk-version-manager.exe) so historical NSIS name-based termination cannot kill it. After verified owned CLI/application/WebView exit, acquire an exclusive canonical installed-image handle with read/write/delete access and no sharing, verify file identity, and rename that same file object to a same-volume quarantine. Retain the fence until the source context is sealed and the fresh target context is durably ready. The handle fences the old file object, not a future NSIS-created pathname; any early target launch therefore occurs only against already-ready fresh state, and must exit before post-install capture or return. A second/unmanaged instance blocks admission. Independently copied old executables remain unsupported; no sandbox claim is made.

Restoring the current feature-bearing bundle happens while a persistent active-context/transaction marker and startup maintenance lease block ordinary initialization. The marker is checked before ConPTY, logger, native storage or WebView bootstrap and remains authoritative after a manager crash releases OS locks. It binds source/target bundle identity and routes accidental starts of the preserved source image to recovery while a historical context is active. Restore the matching context before durably clearing the marker and releasing the lease. The previous delivered 006fa33 is the source baseline, not the bundle to assume present during future user switches. Windows CI must directly prove repeated/in-flight CreateProcess attempts cannot bypass the selected exclusive-handle/rename sequence.

## Transaction and recovery

States: selected → verified → reviewed → quiescing → snapshot-ready → installing → installed-unconfirmed → completed, with cancelled/failed/recovery-required outcomes. Every destructive transition is durably recorded before proceeding. Operations are idempotent by transaction identity; retry reads the journal instead of replaying installation or restore blindly.

Use a controlled Windows installer process with an observed launch result and terminal exit status. The locked Tauri updater 2.10.1 Windows install helper ignores ShellExecuteW's result and exits unconditionally, so it cannot be used as proof of successful installation. Installer signature authenticates bytes rather than advertised version, so package identity must also be inspected and bound to the selected target. Post-install identity checks distinguish installed from first-launch confirmed; old binaries cannot acknowledge the new protocol. No timer alone turns an unknown installation into success.

Review/download cancellation leaves the application data unchanged. The user closes active sessions explicitly; the switch never implicitly kills them. After switch commit/fencing begins, interruption uses a journaled abort/recovery transaction, not a promise to resurrect processes or an unconditional cancellation success. Installer failure or interrupted recovery preserves both snapshots and verified prior application files. A visible recovery action restores the prior application/context only after process quiescence and manifest verification. Never automatically overwrite data created after the attempted switch. Disk-full, permissions, partial copy, external file changes, signature failure and journal corruption fail closed with an actionable recovery state.

## Acceptance

- Catalogue pagination, missing assets, replaced release/asset, malformed/oversize metadata and official-host redirect validation
- Real signature/digest validation tests, not a stub that approves arbitrary bytes
- Fresh-context switch and exact return without losing newer Native metadata; shared-data mode blocked without evidence
- Multiple instances, active/starting/unknown Native and Legacy runs, late completions and cancellation races
- Crash injection at every journal boundary; partial installer failure and recovery; no user-history deletion
- Normal App and maintenance-only composition/IPC tests, keyboard/accessibility, stale UI selections
- Existing session creation/close/resume/menu/icon regressions and complete frontend/Rust gates
- Actual Windows CI installation/downgrade/return in isolated runner state using authorized verified historical packages, plus real rendered UI inspection
- Final test installer and concise PR evidence; no Release/tag/merge/updater publication

Open evidence: historical packaging/storage boundaries, exact Windows manager/installer path rules, native user-machine CLI/manual acceptance and the original source warning's safe error code. The last two remain external acceptance, not reasons to claim the new implementation complete early.

The fresh-settings/return choice and shared CLI-data boundary were approved on 2026-10-02. Implementation does not authorize installing or rolling back the user’s computer.
