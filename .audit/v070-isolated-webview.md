# 0.70 isolated Windows WebView acceptance

Date: 2026-11-12. Computer: WSK-MSI. Base: 038f4acab7e2dce1f3ce37cdb4c7d046743870f9.

## Scope and limits

This is a test-only Tauri/WebView host using the production document bridge, history projection, delete panel, deletion service and a fixed Codex 0.167.0 executable. It is not the signed rc5 installer or full application acceptance. The installed rc4 app, real CLI sessions, credentials and Windows account/security settings were not modified.

WindowsSandbox.exe was absent. VMware Remote Console is not a local VM host. The existing Hyper-V query returned a security error and was not escalated. Existing managed accounts were not reused. Windows KnownFolder APIs make HOME/USERPROFILE/APPDATA overrides insufficient for the installed app, so no copy of that app was launched.

The host uses fresh synthetic projects, explicit workspace repositories, separate main/peer WebView UDFs, cleared environment, pinned CLI, loopback-only dummy provider, disabled plugins/hooks/cloud model activity, owned Windows Jobs and held directories. Pre-PID or unowned browsers remain pending. Exit requires worker reap, own Job empty, every held browser handle signaled and no unresolved creation. There is no broad process-name cleanup.

An initial worktree outside the project was a coordination mistake. Work there stopped; it was retained, copied into this project worktree and source hashes were checked. Final runs explicitly use project-local TEMP/TMP. Early temporary fixtures and the original worktree were retained, not cleaned up.

## Changes

- Admit and retain the exact browser PID/handle in the parent's Job before checking actual UDF identity or starting the page. A UDF failure no longer loses the known lifetime evidence.
- Publish creation and held/rejected markers only after complete write and sync, using same-directory atomic persist_noclobber. Existing final markers are not replaced. Controlled partial-write and publication-failure regressions exercise both marker kinds.
- Preserve only a strictly validated inherited SystemDrive consistent with absolute SystemRoot in the cleared fixture environment. Do not override ProgramData or weaken system_configuration_absent. An independent Rust env_clear probe showed missing SystemDrive caused SHGetKnownFolderPath(ProgramData) HRESULT 0x80070003; retaining the actual drive restored a successful KnownFolder lookup. The actual OpenAI namespace remained absent.
- Add fixed, matching frontend/backend diagnostic code allowlists; arbitrary exceptions are not forwarded into reports. A success result cannot include a failure code.
- The custom frontend host must invalidate the previous profile-revision history context after its deliberate source change, matching the production useUnifiedWorkspaceRuntime profile watcher. Without that lifecycle step, its native-ID-only selection chose the stale row. No production adapter or authorization check was changed. NoNative now also opens and cancels a preview using the replacement revision.

## Executed evidence

- Browser-order RED: 6 passed, 2 failed, 3 ignored; GREEN: 8 passed, 3 ignored.
- Held-marker partial-write/failure RED: 2 failed; GREEN: 10 passed, 3 ignored.
- Creation-marker RED: 2 failed; GREEN: 12 passed, 3 ignored.
- SystemDrive RED reproduced absent inheritance; final native_cli_delete_live module: 21 passed, 8 ignored; strict all-target Clippy passed.
- Fixed frontend diagnostic RED: expected PREVIEW_TIMEOUT was previously collapsed; final native_cli_delete_live module: 22 passed, 8 ignored; strict all-target Clippy passed. These default suites do not run ignored real GUI/CLI cases.
- Production frontend fixture build succeeded: 89 bundled modules, 48 recorded production modules. Current asset manifest SHA256: e62dc74078d871cb0c3d23737fe11c3d1d653ee8167d3c1f7fb112769068903e.

The first NoNative run occurred prematurely before the held-marker review finished. This was reported as a coordination error. It passed but was not used to dismiss that race; the race was fixed and reviewed before later fresh runs.

Fresh NoNative runs after fixes passed with 2 verified UDFs, 7 checkpoints, 6 negative confirmation IPC calls, zero inspect calls, no actual CLI, worker exit 0, own Job empty, root reaped, forced=false and isolationFailed=false. Logs: no-native-final.log, no-native-with-system-drive.log, no-native-diagnostic.log. The diagnostic run took 8.45 seconds and used parent tIIj0r / worker 9Ue3nA beneath .audit/tmp/v027-codex-delete-live-*.

Native failures are retained, not counted as passes:

1. native-final.log: PROGRAM_DATA_UNAVAILABLE before GUI/native action. This exposed the SystemDrive fixture omission.
2. native-with-system-drive.log: strict-config succeeded at Codex 0.167.0 and both UDFs were verified, but frontend acceptance failed after the seventh checkpoint. Only review was captured, confirm IPC count remained 6, inspect count 0. Target/expiry archived fixtures still existed; sqlite remained empty and no installation_id existed. Worker root OTXLcM beneath parent 0K9gAV. Job empty/root reaped, no forced cleanup.
3. native-diagnostic.log: same safe failure reproduced with exact PREVIEW_TIMEOUT. Parent dRgKxO / worker gWzwOG. Confirm 6, inspect 0, actual UDF 2. Driver exited after 49.33 seconds with Job empty/root reaped, forced=false/isolationFailed=false. No final native deletion confirmation was sent. This led to the test-driver profile lifecycle fix above.

The lifecycle-fix frontend passed npm run typecheck and independent read-only review. Final fresh runs passed:

- NoNative: 4.73 seconds, parent 0ddVHG / worker GTvTty. The replacement-revision preview and cancellation ran. The final report was explicitly checked for confirmIpcCount=6 and inspectIpcCount=0 (not merely success=true), actualUdfCount=2 and metadataOwnerSettled=true. Driver: worker exit 0, rootReaped=true, jobEmpty=true, browserCount=2, forced=false, isolationFailed=false. Log: .audit/no-native-lifecycle-final.log.
- Native: 157.05 seconds, parent JVnC1y / worker VpwA5G. All 12 checkpoints passed: initial history, review, cancel, independent second confirmation, missing acknowledgements, invalid document/source/peer, changed source ticket, actual synthetic deletion, duplicate receipt, inspect without replay, and real 120-second preview expiry. Final counts: confirm=10, inspect=1, actual UDF=2; metadata owner settled. Driver: worker exit 0, rootReaped=true, jobEmpty=true, browserCount=2, forced=false, isolationFailed=false. Log: .audit/native-lifecycle-final.log.
- Native target absence and unchanged active/unrelated/other-source/expiry controls were checked by the host; descendant results are still explicitly unverified. No wider subtree-completeness claim is made.
- Reports and screenshots: .audit/tmp/v027-codex-delete-live-JVnC1y/temporary/v027-codex-delete-live-VpwA5G/reports/. The confirmation and result screenshots were visually inspected. The separate acknowledgements, risk text, resulting native acknowledgement and read-only inspect action were visible. This is WebView2 147.0.3912.98, not the installed full app.

Final pins (.audit/final-lifecycle-source.json):
- Rust WebView host: E14421F72857B3FBED8CF8BCCB97E02712DAC9EF6DC4F1D49FF002110F10ECC8.
- Rust CLI fixture: B2598ED4BEA2F5540D9D78DBB0F33C0BC6C629DAA0BA6CB7A9D5511FA84541E1.
- Frontend driver: DF5029AB302E2C8D51E3428E94480B2E9925D78E9738DBCC087D65A60F734054.
- Frozen test executable: 32459ADF3A5D6C22FF5B2EF8D941B91BC6BB9CAAB049FA194CE31738EB4ECF56.
- Native webview-result.json: 3BC245635A6CA1155B472B32E62508B25F256EAFEC2C3FD4C4357E3DE89191E4.
- Native confirmation.png: 0C4A78C571E2122A7C00AF58094586A77B5320A8B23FB65650F5B33162732897.
- Native result.png: 3DF8EFAC691238BD281FE5B48C0EBA8F5FA07A28C2227603F354ED80043978ED.

No failed or unexecuted native step is counted as complete. The buildHead identifies the unchanged base; the dirty test-source hashes above identify the exact executed additions.

## Reproduction

Use the project-local .audit/diagnostic-webview-bin/cc-desk-isolated-tests.exe with its copied ConPTY resources, matching source hashes and assets. Set TEMP/TMP under this worktree's .audit/tmp. Run only the exact ignored test, one thread. The NoNative gate is CC_DESK_NATIVE_DELETE_WEBVIEW_GO=stage11-no-native and test tests::native_cli_delete_live::webview::NativeDeleteWebView_NoNative_090. The separately reviewed Native gate is stage11-native and NativeDeleteWebView_Native_091. Never invoke the Worker_099 test directly. NoNative must pass first on the matching frozen source/assets.

Do not reuse an uncertain fixture or delete retained owners. Real Native tests may modify/delete only their newly created synthetic session data. Do not substitute another CLI, user profile, installed Desk or environment without revalidating the isolation contract.

## Remaining evidence gaps

Signed rc5 installation/upgrade/rollback, full installed application archive/restore UI, external active-session/subtree interaction, other providers, real-model activity, and actual injected browser-UDF failure cleanup are not established by this host. The older stage11 unknown browser lifetime is not retroactively resolved by fresh successes. Unit closure-order/marker regressions are not real browser failure-injection evidence.