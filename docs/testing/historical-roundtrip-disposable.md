# Disposable Windows historical roundtrip

This is a test-only driver for the fixed official 0.17.7 payload. Production admission remains false. It is not an updater, an installer deployment workflow, or a crash/restart certification. The current runtime result is **BLOCKED_EXTERNAL_TARGET**: no qualifying disposable Windows target or native selector probe has been recorded.

The driver can finish one uninterrupted success scenario or one **injected pre-resume recovery** scenario. Both must pass on separately reset baselines for this roundtrip to be accepted. A source/contract test, a hosted Windows compile, or a completed manager status does not certify either scenario.

## Required external preparation

1. Provision a genuinely disposable Windows x64 VM/account with known synthetic data, NTFS and WebView2. A new directory, redirected profile, hosted-runner label or isolated checkout is insufficient. Keep the reviewable provisioning record outside the application state. Code checks the supplied record and actual token/job/image state; it cannot establish that an arbitrary account is disposable.
2. Build the actual 0.18 acceptance application in an isolated checkout based on `9c981a5093a80b947817af8eebe4855293690185`. Use the resulting acceptance source revision as `buildId`, not the archived base SHA. Set build-time `CC_DESK_BUILD_SHA` to that exact revision before compilation. Substitute the fixed `tests/fixtures/version-history-roundtrip/target.json` with the reviewed target SID, profile/install/evidence directories, UUID `runId` and scenario. Record SHA256 of the exact target file bytes as `bindingSha256`. The app requires `buildId` to match compiled build metadata; runtime environment changes cannot admit or retarget it.
3. Prepare the existing pinned ConPTY runtime and use Tauri CLI `build --debug --features history-roundtrip-acceptance`; package the embedded local-origin frontend in a separately labeled test-only NSIS build with updater artifact generation disabled. Ordinary release/candidate packaging is not this build. Record the NSIS SHA256 and installed application SHA256. Actual Windows build/packaging and embedded-origin verification are still required. There is no assumed public 0.18 installer.
4. Establish the registered same-user 0.18 test installation as a separate fixture setup. Capture its full bundle, including ConPTY, uninstaller and an extra unknown companion. Seed distinguishable original preferences, valid newer Native metadata, unknown Desk and WebView files, and two harmless synthetic CLI/project sentinels outside all Desk, WebView, installation and recovery roots. Do not use credentials or real CLI sessions. Success has both product shortcuts; pre-resume recovery starts with exactly one shortcut absent.
5. Launch the actual source interactively through its installed entry, outside every external job, using an unelevated token. Record its PID. Do not start the source from a hosted runner or use breakaway/impersonation to escape containment. The driver itself also refuses elevated or job-contained execution. Each scenario requires its own freshly reset baseline and separately compiled fixed scenario binding.
6. Probe the actual source, manager and immutable 0.17.7 accessible controls on this target. Record exact Name, native ControlType ID and AutomationId, including the historical preference action and rendered original/fresh/later state checks. Preserve probe screenshots and a hash-bound JSON selector record. Source/manager labels in this driver require English. Missing or ambiguous controls produce `BLOCKED_UI_SELECTOR`; they do not authorize an IPC, browser automation, JavaScript or debugger fallback.

## Invocation and read-only checks

Use PowerShell 7 x64 in the target's interactive unelevated account. Do not enable a global execution-policy exception.

```powershell
pwsh -NoProfile -File scripts/version-history/roundtrip.ps1 `
  -TargetManifest C:\DisposableEvidence\reviewed-build.json `
  -Scenario success `
  -EvidenceDirectory C:\DisposableEvidence\one-fixed-run
```

The recovery scenario uses `-Scenario before-installer-resume` and its distinct reviewed build/baseline/run. `-TimeoutSeconds` bounds each wait (default 120, maximum 1800); elapsed time never establishes a successful outcome. The driver does not kill, replay or retry a transaction. On a blocked/failed run, keep its live manager and all evidence available for an explicit, reviewed recovery decision.

```powershell
# Validation only: no native API call, UI action, installer or profile/registry write.
pwsh -NoProfile -File scripts/version-history/roundtrip.ps1 `
  -TargetManifest C:\DisposableEvidence\reviewed-build.json `
  -Scenario success `
  -EvidenceDirectory C:\DisposableEvidence\one-fixed-run `
  -DryRun

# Parse both scripts and exercise production pure validators with synthetic mutations.
pwsh -NoProfile -File tests/scripts/historyRoundtripDriver.ps1

# Load framework UIA support and compile embedded C# only; no P/Invoke is called.
. ./scripts/version-history/roundtrip-ui.ps1
Initialize-RoundtripNative

# Portable source contracts; executes the PowerShell suite if pwsh is available.
node --test tests/scripts/historyRoundtripDriver.node.mjs
```

`DRY_RUN_VALIDATED` explicitly has `nativeExecution=false` and `roundtripAccepted=false`. The optional `-DryRunProbe <JSON>` validates recorded hypothetical process count/hash/control/wait/report-envelope failure cases, without using any of them to perform native actions. The executable test writes only its own temporary fixture directory. Linux without PowerShell runs the six source contracts and reports the executable contract suite as skipped; that is not PowerShell parsing, C# compilation or Windows execution evidence.

## Reviewed build manifest

All fields below are mandatory. Do not commit a target-bound manifest or change the deny-only repository target to claim execution readiness.

| Field | Meaning |
| --- | --- |
| `schema` | `1` |
| `baseHead` | Exact archived `9c981a5093a80b947817af8eebe4855293690185` |
| `buildId` | Exact resulting acceptance source SHA, distinct from base and matching compiled metadata |
| `runId`, `scenario` | Exact compiled UUID and `success` / `before-installer-resume` |
| `compiledTargetPath`, `bindingSha256` | Reviewed build-input file and hash of its exact bytes |
| `imageSha256` | Actual registered 0.18 acceptance `cc-desk.exe` bytes |
| `packagePath`, `packageSha256` | Separately labeled acceptance NSIS artifact and hash |
| `provisionEvidencePath`, `provisionEvidenceSha256` | External reviewed disposable-target/reset evidence and hash |
| `resetBaselineId`, `sourcePid` | Distinct reset baseline and already interactively launched source PID |
| `deskDirectory`, `webViewDirectory` | Actual bounded native roots, not guessed default paths |
| `nativeProbePath`, `nativeProbeSha256` | Actual rendered native selector evidence and hash |
| `selectors` | The exact observed controls listed below |
| `syntheticAssertions` | `originalFiles` and `laterFiles`; each has `restoredPath`, `retainedPath`, `sha256` |
| `sharedSentinels` | Exactly one `cli` and one `project` sentinel, each with `kind`, `path`, `beforeSha256`, `afterSha256`, `afterUtf8` |

Provision evidence has `approvedDisposableTarget`, `knownSyntheticState`, `ntfs` and `webView2` explicitly true; exact compiled `targetSid`, `profileDirectory`, `installDirectory`; matching `resetBaselineId`; and `sourceLaunchMethod: "native-interactive"`. These are externally reviewed attestations, independently checked against actual source/driver tokens and source image/job state. They are not a code-generated disposability certificate.

Native probe JSON has exact `targetSid`, `runId`, `imageSha256` and `selectors`. Every selector has nonempty `name`, numeric `controlType` (UIA `50000`–`50040`) and `automationId` (empty only if the actual native probe observed no ID). Names are exact, case-sensitive and uniquely visible in the observed process window. Controls needed for actions must be enabled and support their actual UIA pattern. No invented DOM data attributes are selectors.

Common selectors: `sourceNativeMetadata`, `settings`, `sourceOriginalPreference`, `updates`, `selectVersion` (exact `Select version 0.17.7`), `prepare`, `review`, `begin`, `managerReturn` (`Return to previous version`), `managerRestore` (`Restore previous version`), `restoredStatus`, `restoredNormal`, `restoredNativeMetadata`, `restoredOriginalPreference`.

Success selectors: `historicalFresh`, `historicalPreferenceOpen`, `historicalPreference`, `historicalPreferenceChanged`, `managerConfirm`, `managerConfirmSubmit`, `managerConfirmed`. `historicalPreference` additionally records its actual `pattern` (`Invoke`, `SelectionItem` or `Value`); `Value` also requires the reviewed `value`. The changed-state selector must prove the distinctive historical preference is rendered. The original Native metadata selector must identify the known synthetic project/session metadata in the normal workspace. The preference selectors identify rendered original/fresh/later states, not just a generic Settings heading.

Recovery selector: `managerRecoveryRequired`. It must identify the current authenticated manager's actual recovery-required surface.

`originalFiles` covers distinguishable unknown original files and the extra original bundle companion. `laterFiles` covers distinctive later files, including the persisted historical preference or a distinguishable harmless sentinel. Each identifies its expected bytes in one context and absence in the other; use unique synthetic names for this absence check. Complete typed recapture additionally verifies every original file and Native metadata byte, independently of these explicit sentinels. The distinctive preference must also be checked through the real UI. Reviewed retained paths are assertions only; actual retained root/copy locations come from the live owner's report, including randomly named quarantine roots.

## Passive observation contract

Rust writes create-new `<runId>-<Stage>.json` beneath the compiled private evidence directory. The driver never writes these reports, reads them as coordinator authority, or supplies replacement reports to the application. Missing, wrong, incomplete or write-failed reports fail acceptance while the native transaction retains its ordinary custody. The Rust sink allows up to 128 MiB; the driver deliberately accepts at most 32 MiB per report and blocks larger records. Neither side truncates inventories.

Every envelope has `schema:1`, exact `baseHead`, `buildId`, `bindingSha256`, `runId`, `scenario`, `transactionId`, nonzero monotone `generation`, exact `stage` and `value`. Every value carries the identical full typed `binding`; the transaction UUID must match the envelope. Required common stages are `M0`, `SourceSealed`, `FreshReady` and `FinalRestored`. Success also requires `TargetVerified`, `HistoricalLaunched` and `LaterCaptured`. Recovery requires `InstallerSuspended`, `InjectedPreResumeFailure` and `CancelledBeforeResume`.

`M0` carries actual post-source-exit `bundle`, `bundleLogicalDigest`, `context`, complete retained `registration` and `shortcuts`, and `dataRoot`. Prelaunch fixture bytes cannot replace M0 because native logging/WebView shutdown can legitimately write before capture. Bundle `fenced_image_location` can differ across the intentional source-image quarantine; compare its exact logical inventory/name/digest, not that location as restored content.

`FinalRestored` carries `phase:"Restored"`, `pending:false`, read-back terminal `marker`, full original `sourceBundle`, actual restored `bundle`, `bundleLogicalDigest`, verified original `context`, complete `registration`/`shortcuts`, `retainedContext`, `retainedBundle`, `dataRoot`, `laterContextDirectory`, actual `retainedContextLocations` (`Desk`, `WebView`, including proven absence paths) and `retainedBundleDirectory`. The marker's full binding and generation must match, its state must be `Restored`, and its `journal_digest` must be an exact SHA256. Final restoration is reported only after the coordinator's repeated actual `verify_restored()` checks. The preserved original manifest remains different from an independently recaptured state.

Final also supplies actual held `markerLogPath`, `journalLogPath` and inspected `journalHead`. After the manager exits, native bounded readers independently read both append-only logs before reopening. Every newline-delimited canonical frame/record is hashed from its exact raw payload substring, its previous digest and sequence/generation are checked, and its full binding must match. Missing, unknown, torn or digest-mismatched tails fail. The journal must end in `Phase { phase: "Restored" }` at the reported generation/head, without unmatched intents or unknown observations; the final native marker must equal the manager's marker and reference that same journal head. Both raw log hashes are captured twice and must remain unchanged. Marker limits are 4,096 records/16 KiB per frame; journal limits are 100,000 records/128 KiB per record; both logs are bounded to 64 MiB.

`HistoricalLaunched.creation` and `InstallerSuspended.creation` are the actual durable process identity receipts. Driver evidence checks actual PID, creation epoch, token owner, image digest and job containment. Success also independently reads the exact owned job's actual active-process count and requires zero after normal historical close. Pre-resume failure requires no resume attempt, continued manager custody and actual installer process/job presence before the explicit UI Return. `CancelledBeforeResume` requires the same created process and owned job in its actual durable cancellation terminal, zero active processes, `armedPreparation`, a cancellation intent, `resumeAttempted:false` and `emptyOwnedJob:true`. This scenario deliberately never executes NSIS; it is not a real NSIS failure or crash recovery test.

## Required execution order and comparisons

The source's actual Settings → Updates UI selects/verifies 0.17.7 and opens the rendered review before Begin. The driver captures that review and the real source-to-retained-manager transition. Source exit, SourceSealed and FreshReady must be observed. Existing authenticated pages and the normal source/manager/coordinator process route remain the sole mutation authority.

Success checks the actual target image/version and rendered fresh UI, changes the real historical preference, and records confirmation through both real manager confirmation controls. It closes the historical app normally, observes the original process exit and actual owned job zero, and changes only the two reviewed synthetic shared sentinels. It then uses both real Return controls. The recovery scenario waits at the explicit injected pre-resume failure, captures recovery-required UI and live custody, and uses those same two explicit Return controls before checking the real cancellation terminal.

For either scenario the order is strict:

1. Read the completed manager's own final typed verification report while the original owners remain held
2. Close that completed manager through its normal UIA WindowPattern and observe the actual process exit, releasing its exclusive source-image fence
3. Independently recapture bounded native installation, original Desk/WebView, product registration, shortcuts and retained later state; pass all comparisons before reopening
4. Launch the actual installed `cc-desk.exe` with no acceptance-control argument, check its image/hash/version/token, observe rendered normal UI and original Native metadata/preference, and capture a screenshot

Independent recapture pins local-volume ancestors and objects, rejects reparse points, hard-linked files, named streams, changed identities, unsupported attributes, failed access or incomplete enumeration, and preserves exact absence versus an empty existing root. Tree budgets are 100,000 entries, depth 128, 16 GiB total and 8 GiB per file. Registry budgets are 4,096 keys, 16,384 values, depth 32 and 4 MiB aggregate raw data; raw values retain type and bytes. Only the verified same-user Classes alias is supported. The registry capture covers the entire Uninstall, Publisher and four product Explorer trees, their parent namespaces/security and exactly the owned `CC Desk` Run value. It does not capture other shared Run values.

Bundle equality covers all paths, kinds, byte counts, hashes, link counts and DOS/security permissions, including uninstaller/ConPTY/unknown companions. Its logical digest excludes copied-file IDs. The only restoration-specific descriptor tolerance is the exact native `SE_DACL_AUTO_INHERITED` clear-to-set transition already admitted by Rust; owner/group/DACL bytes and every other control bit stay exact. Original Desk/WebView equality also requires original object and location identities. Shortcut equality checks both actual known-folder parents, bytes, hash, permissions and absence; copied shortcut file IDs are intentionally excluded from restored content equality. Retained private copies keep their independently recorded copy IDs and private permissions. Two consecutive independent source/registration/shortcut captures must remain equal.

The driver creates `driver-*.json` and native-window PNG evidence without overwriting prior records. `PASS` is written only after actual restored-app reopening and rendered state checks. A `BLOCKED` or `FAILED_ACCEPTANCE` output preserves live custody/evidence and does not constitute a pass. Both runs must bind their exact base/source/build-input/package/image hashes and distinct reset baselines. Passing them does not enable production admission or certify shared-current-data mode, lost-job/crash restart, all historical versions, real CLI Layer-C or general Windows rendering/scaling.

## Current verification boundary

Portable source contracts have run locally. PowerShell dry-run mutations, PowerShell parsing, native-helper C# compilation, embedded Windows packaging, the genuine target/probe, both native scenarios and actual restored reopening are still pending Windows execution. The CI compile-only job may run parsing, inert build-policy checks and the pure dry-run/report contracts. Hosted CI must never execute this driver without `-DryRun` or describe itself as a disposable native acceptance target.

For an absent retained later root, the observer reports the actual retained empty-copy slot; its original slot is occupied by the restored source. Present later roots report their moved native object locations. The independent comparison preserves that existence distinction.

The shared-sentinel writer checks actual held canonical paths against installation, Desk, WebView, the observed transaction data and control roots, and the fixed evidence directory before any byte change. The manifest must name one CLI sentinel and one project sentinel using normal absolute drive paths; traversal spellings are refused. PowerShell contracts invoke the actual record writer and the actual native helper’s pure path predicate without native API calls.
