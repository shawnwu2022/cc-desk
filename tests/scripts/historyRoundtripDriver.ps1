#Requires -Version 7.0
# Executable dry-run and pure-report contracts. No native API, installer, profile, or registry writes.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$driverPath = Join-Path $root 'scripts/version-history/roundtrip.ps1'
$uiPath = Join-Path $root 'scripts/version-history/roundtrip-ui.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($driverPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) { throw ($errors | Out-String) }
$uiAst = [Management.Automation.Language.Parser]::ParseFile($uiPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) { throw ($errors | Out-String) }
$functions = $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $true)
foreach ($function in $functions) { . ([scriptblock]::Create($function.Extent.Text)) }
$script:NativeLoaded = $false
$script:TransactionId = $null
$script:Binding = $null
$script:Generation = [uint64]0
$script:Scenario = 'success'
$passed = 0

function Check-Passes([string]$Name, [scriptblock]$Action) {
    & $Action; $script:passed++; Write-Output "PASS $Name"
}
function Check-Rejects([string]$Name, [scriptblock]$Action, [string]$Expected) {
    $message = $null
    try { & $Action } catch { $message = $_.Exception.Message }
    if (-not $message -or $message -notmatch $Expected) { throw "$Name expected $Expected, got $message" }
    $script:passed++; Write-Output "PASS $Name"
}
function Copy-TestValue($Value) { $Value | ConvertTo-Json -Depth 100 -Compress | ConvertFrom-Json -Depth 100 }

$temporary = Join-Path ([IO.Path]::GetTempPath()) ('ccdesk-roundtrip-contract-' + [guid]::NewGuid())
[IO.Directory]::CreateDirectory($temporary) | Out-Null
try {
    $uuid = '11111111-2222-4333-8444-555555555555'
    $binding = [ordered]@{ transaction_id = $uuid; source_context = '22222222-2222-4333-8444-555555555555'; target_context = '33333333-2222-4333-8444-555555555555'; user_installation = 'b' * 64; source_bundle = 'a' * 64; target_package = 'e9ffbc5ba627f0c133a4385db404342a7344729339185e6f9b8ee6b5969086ac'; target_payload = 'd' * 64; roots = 'e' * 64 }
    $permission = [ordered]@{ Windows = [ordered]@{ descriptor = @(1, 0, 4, 128) + @(0) * 16; attributes = 32 } }
    $entry = [ordered]@{ metadata = [ordered]@{ path = 'cc-desk.exe'; kind = 'File'; size = 3; object_identity = 'f' * 64; link_count = 1; permissions = $permission }; sha256 = '1' * 64 }
    $tree = [ordered]@{ schema = 1; location_identity = '2' * 64; entries = @($entry) }
    $bundle = [ordered]@{ schema = 1; original_image_name = 'cc-desk.exe'; fenced_image_location = '3' * 64; tree = $tree }
    $context = [ordered]@{ schema = 1; binding = $binding; context_id = $binding.source_context; roots = @([ordered]@{ root = 'Desk'; location_identity = '4' * 64; entries = @($entry) }, [ordered]@{ root = 'WebView'; location_identity = '5' * 64; entries = @() }) }
    $registration = [ordered]@{ format = 2; control_root = @{ volume = 1; id = @(0) * 16 }; binding = $binding; snapshot = @{ schema = 2; user_sid = 'S-1-5-21-1-2-3-1001'; installation = @{ volume = 1; id = @(1) * 16 }; trees = @('Uninstall', 'Publisher', 'DeskDirectory', 'DeskDirectoryBackground', 'LegacyDirectory', 'LegacyDirectoryBackground' | ForEach-Object { @{ slot = $_; parent = @{ namespace = $null; security = $null }; nodes = @() } }); run = @{ namespace = @(65); parent_security = @(1); value = @{ kind = 1; bytes = @(65, 0) } } } }
    $links = @('Desktop', 'StartMenu' | ForEach-Object { [ordered]@{ slot = $_; parent = @{ volume = 1; id = @(2) * 16 }; parent_path = @(65); leaf = 'CC Desk.lnk'; state = @{ Present = @{ identity = @{ volume = 1; id = @(3) * 16 }; attributes = 32; bytes = @(65); sha256 = '6' * 64; descriptor = $permission.Windows.descriptor } } } })
    $shortcuts = @{ format = 2; binding = $binding; entries = $links }
    $m0 = Copy-TestValue @{ generation = 5; value = @{ binding = $binding; bundle = $bundle; bundleLogicalDigest = 'a' * 64; context = $context; registration = $registration; shortcuts = $shortcuts; dataRoot = 'C:\synthetic\recovery' } }
    $final = Copy-TestValue @{ generation = 50; value = @{ binding = $binding; phase = 'Restored'; pending = $false; marker = @{ schema = 1; binding = $binding; generation = 50; journal_digest = '7' * 64; state = 'Restored' }; bundle = $tree; sourceBundle = $bundle; bundleLogicalDigest = 'a' * 64; context = $context; registration = $registration; shortcuts = $shortcuts; retainedContext = @(1, $binding, $binding.target_context, @(@('Desk', $tree), @('WebView', $tree))); retainedBundle = @{ schema = 1; source = $tree; copy = $tree }; dataRoot = 'C:\synthetic\recovery'; laterContextDirectory = 'C:\synthetic\recovery\later'; retainedContextLocations = @{ Desk = 'C:\synthetic\recovery\later\a'; WebView = 'C:\synthetic\recovery\later\b' }; retainedBundleDirectory = 'C:\synthetic\recovery\bundle' } }
    $capture = Copy-TestValue @{ bundle = $tree; logicalDigest = 'a' * 64; desk = @{ schema = 1; location_identity = '4' * 64; entries = @($entry) }; webView = @{ schema = 1; location_identity = '5' * 64; entries = @() }; registration = $registration.snapshot; shortcuts = $links }
    Check-Passes 'Report_CompleteLogicalRestore_001' { Assert-RoundtripRestoredCapture $m0 $final $capture }
    Check-Passes 'Report_ChangedFenceLocation_027' { $copy = Copy-TestValue $final; $copy.value.sourceBundle.fenced_image_location = '8' * 64; Assert-RoundtripRestoredCapture $m0 $copy $capture }
    Check-Rejects 'Report_MissingBundleEntry_002' { $copy = Copy-TestValue $capture; $copy.bundle.entries = @(); Assert-RoundtripRestoredCapture $m0 $final $copy } 'complete inventory count'
    Check-Rejects 'Report_ExtraBundleEntry_003' { $copy = Copy-TestValue $capture; $copy.bundle.entries += $copy.bundle.entries[0]; Assert-RoundtripRestoredCapture $m0 $final $copy } 'complete inventory count'
    Check-Rejects 'Report_ChangedBundleBytes_004' { $copy = Copy-TestValue $capture; $copy.bundle.entries[0].sha256 = '8' * 64; Assert-RoundtripRestoredCapture $m0 $final $copy } 'sha256'
    Check-Rejects 'Report_ChangedBundlePermissions_005' { $copy = Copy-TestValue $capture; $copy.bundle.entries[0].metadata.permissions.Windows.descriptor[4] = 1; Assert-RoundtripRestoredCapture $m0 $final $copy } 'permissions mismatch'
    Check-Passes 'Report_AllowedInheritanceFlag_006' { $copy = Copy-TestValue $capture; $copy.bundle.entries[0].metadata.permissions.Windows.descriptor[3] = 132; Assert-RoundtripRestoredCapture $m0 $final $copy }
    Check-Rejects 'Report_ChangedContextIdentity_007' { $copy = Copy-TestValue $capture; $copy.desk.entries[0].metadata.object_identity = '9' * 64; Assert-RoundtripRestoredCapture $m0 $final $copy } 'object_identity'
    Check-Rejects 'Report_AbsentContextBecamePresent_008' { $copy = Copy-TestValue $capture; $copy.webView.entries = @($copy.desk.entries[0]); Assert-RoundtripRestoredCapture $m0 $final $copy } 'complete inventory count'
    Check-Rejects 'Report_ForeignContextBinding_009' { $copy = Copy-TestValue $final; $copy.value.binding.target_context = '44444444-2222-4333-8444-555555555555'; Assert-RoundtripRestoredCapture $m0 $copy $capture } 'transaction binding'
    Check-Rejects 'Report_ForeignMarkerGeneration_010' { $copy = Copy-TestValue $final; $copy.value.marker.generation = 49; Assert-RoundtripRestoredCapture $m0 $copy $capture } 'marker generation'
    foreach ($index in @(0, 1)) {
        Check-Rejects "Report_ChangedShortcut_${index}" { $copy = Copy-TestValue $capture; $copy.shortcuts[$index].state.Present.bytes = @(66); Assert-RoundtripRestoredCapture $m0 $final $copy } 'shortcut bytes'
    }
    Check-Rejects 'Report_MissingRetainedContext_013' { $copy = Copy-TestValue $final; $copy.value.retainedContext = $null; Assert-RoundtripRestoredCapture $m0 $copy $capture } 'retained target context'
    Check-Rejects 'Report_NonterminalMarker_014' { $copy = Copy-TestValue $final; $copy.value.marker.state = 'Transition'; Assert-RoundtripRestoredCapture $m0 $copy $capture } 'nonterminal'
    Check-Rejects 'Report_ChangedOwnedRun_015' { $copy = Copy-TestValue $capture; $copy.registration.run.value.bytes = @(66, 0); Assert-RoundtripRestoredCapture $m0 $final $copy } 'owned Run'
    Check-Rejects 'Report_PendingOutcome_016' { $copy = Copy-TestValue $final; $copy.value.pending = $true; Assert-RoundtripRestoredCapture $m0 $copy $capture } 'nonterminal'

    function Test-LogEnvelope([string]$Name, $Payload) {
        $raw = $Payload | ConvertTo-Json -Depth 100 -Compress
        $digest = Get-RoundtripBytesHash ([Text.UTF8Encoding]::new($false).GetBytes($raw))
        @{ text = ('{"' + $Name + '":' + $raw + ',"digest":"' + $digest + '"}' + "`n"); digest = $digest }
    }
    $genesis = Test-LogEnvelope 'record' ([ordered]@{ schema = 2; binding = $binding; generation = 0; previous = $null; lane = $null; event = @{ Begin = @{ capacity = @{} } } })
    $terminal = Test-LogEnvelope 'record' ([ordered]@{ schema = 2; binding = $binding; generation = 1; previous = $genesis.digest; lane = 'Recovery'; event = @{ Phase = @{ phase = 'Restored' } } })
    $logFinal = Copy-TestValue $final
    $logFinal.generation = 1; $logFinal.value.marker.generation = 1; $logFinal.value.marker.journal_digest = $terminal.digest
    $logFinal.value | Add-Member -NotePropertyName journalHead -NotePropertyValue $terminal.digest
    $marker = Test-LogEnvelope 'frame' ([ordered]@{ schema = 1; sequence = 0; previous = $null; marker = $logFinal.value.marker })
    $journalBytes = [Text.UTF8Encoding]::new($false).GetBytes($genesis.text + $terminal.text)
    $markerBytes = [Text.UTF8Encoding]::new($false).GetBytes($marker.text)
    Check-Passes 'Log_IndependentTerminalChains_028' { Assert-RoundtripTerminalLogs $logFinal $markerBytes $journalBytes | Out-Null }
    Check-Rejects 'Log_MissingChain_029' { Assert-RoundtripTerminalLogs $logFinal $markerBytes ([byte[]]@()) } 'missing/oversized/torn'
    Check-Rejects 'Log_TornTail_030' { Assert-RoundtripTerminalLogs $logFinal $markerBytes $journalBytes[0..($journalBytes.Length - 2)] } 'missing/oversized/torn'
    Check-Rejects 'Log_UnknownTail_031' { $unknown = Test-LogEnvelope 'record' ([ordered]@{ schema = 2; binding = $binding; generation = 2; previous = $terminal.digest; lane = 'Recovery'; event = @{ ForgedSuccess = @{} } }); Assert-RoundtripTerminalLogs $logFinal $markerBytes ([Text.Encoding]::UTF8.GetBytes($genesis.text + $terminal.text + $unknown.text)) } 'unknown journal event'
    Check-Rejects 'Log_ForeignHead_032' { $copy = Copy-TestValue $logFinal; $copy.value.journalHead = '9' * 64; Assert-RoundtripTerminalLogs $copy $markerBytes $journalBytes } 'journal head'
    Check-Rejects 'Log_PendingIntent_033' { $intent = Test-LogEnvelope 'record' ([ordered]@{ schema = 2; binding = $binding; generation = 1; previous = $genesis.digest; lane = 'Forward'; event = @{ Intent = @{ effect = @{ effect_id = 'owned-effect' } } } }); Assert-RoundtripTerminalLogs $logFinal $markerBytes ([Text.Encoding]::UTF8.GetBytes($genesis.text + $intent.text)) } 'pending native transaction intent'

    $target = @{ schema = 1; enabled = $true; baseHead = '9c981a5093a80b947817af8eebe4855293690185'; buildId = '1' * 40; runId = $uuid; scenario = 'success'; targetSid = 'S-1-5-21-1-2-3-1001'; profileDirectory = 'C:\synthetic'; installDirectory = 'C:\synthetic\CC Desk'; evidenceDirectory = 'C:\synthetic\evidence' }
    $targetPath = Join-Path $temporary 'target.json'; $target | ConvertTo-Json -Compress | Set-Content -LiteralPath $targetPath -Encoding utf8NoBOM
    $provisionPath = Join-Path $temporary 'provision.json'
    @{ approvedDisposableTarget = $true; knownSyntheticState = $true; ntfs = $true; webView2 = $true; targetSid = $target.targetSid; profileDirectory = $target.profileDirectory; installDirectory = $target.installDirectory; resetBaselineId = 'pure-contract-only'; sourceLaunchMethod = 'native-interactive' } | ConvertTo-Json -Compress | Set-Content -LiteralPath $provisionPath -Encoding utf8NoBOM
    $selectors = [ordered]@{}
    foreach ($name in @('settings', 'updates', 'selectVersion', 'prepare', 'review', 'begin', 'managerReturn', 'managerRestore', 'restoredStatus', 'restoredNormal', 'sourceOriginalPreference', 'restoredOriginalPreference', 'sourceNativeMetadata', 'restoredNativeMetadata', 'historicalFresh', 'historicalPreferenceOpen', 'historicalPreference', 'historicalPreferenceChanged', 'managerConfirm', 'managerConfirmSubmit', 'managerConfirmed')) { $selectors[$name] = @{ name = $name; controlType = 50000; automationId = '' } }
    $selectors.selectVersion.name = 'Select version 0.17.7'; $selectors.managerReturn.name = 'Return to previous version'; $selectors.managerRestore.name = 'Restore previous version'
    $probePath = Join-Path $temporary 'native-probe.json'; $imageHash = '2' * 64
    @{ targetSid = $target.targetSid; runId = $uuid; imageSha256 = $imageHash; selectors = $selectors } | ConvertTo-Json -Depth 100 -Compress | Set-Content -LiteralPath $probePath -Encoding utf8NoBOM
    $packagePath = Join-Path $temporary 'compile-only-package.bin'; [IO.File]::WriteAllBytes($packagePath, [byte[]]@(65))
    $manifest = @{ schema = 1; baseHead = $target.baseHead; buildId = $target.buildId; runId = $uuid; scenario = 'success'; bindingSha256 = Get-RoundtripFileHash $targetPath; compiledTargetPath = $targetPath; imageSha256 = $imageHash; packagePath = $packagePath; packageSha256 = Get-RoundtripFileHash $packagePath; provisionEvidencePath = $provisionPath; provisionEvidenceSha256 = Get-RoundtripFileHash $provisionPath; resetBaselineId = 'pure-contract-only'; sourcePid = 123; deskDirectory = 'C:\synthetic\.cc-box'; webViewDirectory = 'C:\synthetic\UDF'; nativeProbePath = $probePath; nativeProbeSha256 = Get-RoundtripFileHash $probePath; selectors = $selectors; syntheticAssertions = @{ originalFiles = @(); laterFiles = @() }; sharedSentinels = @(@{ kind = 'cli'; path = 'C:\synthetic\shared-cli\sentinel.txt' }, @{ kind = 'project'; path = 'C:\synthetic\project\sentinel.txt' }) }
    $manifestPath = Join-Path $temporary 'manifest.json'
    $reports = @('M0', 'SourceSealed', 'FreshReady', 'TargetVerified', 'HistoricalLaunched', 'LaterCaptured', 'FinalRestored' | ForEach-Object { @{ schema = 1; baseHead = $target.baseHead; buildId = $target.buildId; runId = $uuid; scenario = 'success'; bindingSha256 = $manifest.bindingSha256; transactionId = $uuid; generation = 5; stage = $_; value = @{ binding = $binding } } })
    $dryProbe = @{ sourceCount = 1; imageSha256 = $imageHash; controlsAvailable = $true; timedOut = $false; reports = $reports }
    $dryProbePath = Join-Path $temporary 'dry-probe.json'
    function Run-Dry($Value, $Probe, [string]$ScenarioValue = 'success', [string]$Evidence = 'C:\synthetic\evidence') {
        $Value | ConvertTo-Json -Depth 100 -Compress | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
        $Probe | ConvertTo-Json -Depth 100 -Compress | Set-Content -LiteralPath $dryProbePath -Encoding utf8NoBOM
        $executable = Join-Path $PSHOME $(if ($IsWindows) { 'pwsh.exe' } else { 'pwsh' })
        $output = & $executable -NoProfile -File $driverPath -TargetManifest $manifestPath -Scenario $ScenarioValue -EvidenceDirectory $Evidence -DryRun -DryRunProbe $dryProbePath
        $value = ($output | Select-Object -Last 1) | ConvertFrom-Json
        if ($value.status -ne 'DRY_RUN_VALIDATED') { throw $value.detail }
        if ($value.nativeExecution -ne $false -or $value.roundtripAccepted -ne $false) { throw 'Dry-run must never certify native acceptance' }
    }
    Check-Passes 'DryRun_ReadOnlyValidation_017' { Run-Dry $manifest $dryProbe }
    Check-Rejects 'DryRun_TargetMismatch_018' { Run-Dry $manifest $dryProbe 'success' 'C:\foreign' } 'TARGET_MISMATCH'
    Check-Rejects 'DryRun_ScenarioMismatch_019' { Run-Dry $manifest $dryProbe 'before-installer-resume' } 'SCENARIO_MISMATCH'
    Check-Rejects 'DryRun_HashMismatch_020' { $copy = Copy-TestValue $dryProbe; $copy.imageSha256 = '3' * 64; Run-Dry $manifest $copy } 'IMAGE_HASH_MISMATCH'
    Check-Rejects 'DryRun_MissingControls_021' { $copy = Copy-TestValue $dryProbe; $copy.controlsAvailable = $false; Run-Dry $manifest $copy } 'BLOCKED_UI_SELECTOR'
    Check-Rejects 'DryRun_DuplicateProcess_022' { $copy = Copy-TestValue $dryProbe; $copy.sourceCount = 2; Run-Dry $manifest $copy } 'DUPLICATE_PROCESS'
    Check-Rejects 'DryRun_MissingReports_023' { $copy = Copy-TestValue $dryProbe; $copy.reports = @(); Run-Dry $manifest $copy } 'MISSING_REPORT'
    Check-Rejects 'DryRun_Timeout_024' { $copy = Copy-TestValue $dryProbe; $copy.timedOut = $true; Run-Dry $manifest $copy } 'WAIT_TIMEOUT'
    Check-Rejects 'DryRun_ForeignTransaction_025' { $copy = Copy-TestValue $dryProbe; $copy.reports[-1].transactionId = '44444444-2222-4333-8444-555555555555'; Run-Dry $manifest $copy } 'INVALID_EVIDENCE'
    Check-Rejects 'DryRun_StaleGeneration_026' { $copy = Copy-TestValue $dryProbe; $copy.reports[-1].generation = 4; Run-Dry $manifest $copy } 'generation regressed'
    $script:Run = Copy-TestValue $manifest
    $script:EvidenceRoot = $temporary
    $script:TransactionId = $uuid
    $writerValue = @{ status = 'contract'; nested = @{ entries = @(1, 2, 3) } }
    Check-Passes 'Record_ActualCreateNewWriter_034' {
        Write-RoundtripRecord -Name 'actual-writer' -Value $writerValue
        $record = Read-RoundtripJson (Join-Path $temporary 'driver-actual-writer.json')
        Assert-RoundtripEqual 1 $record.schema 'writer schema'
        foreach ($field in @('baseHead', 'buildId', 'runId', 'scenario', 'bindingSha256')) {
            Assert-RoundtripEqual $script:Run.$field $record.$field "writer $field"
        }
        Assert-RoundtripEqual $uuid $record.transactionId 'writer transaction'
        Assert-RoundtripEqual (Copy-TestValue $writerValue) $record.value 'writer value'
        if ([string]::IsNullOrEmpty($record.recordedUtc)) { throw 'writer timestamp missing' }
    }
    $writtenHash = Get-RoundtripFileHash (Join-Path $temporary 'driver-actual-writer.json')
    Check-Rejects 'Record_ActualWriterRefusesOverwrite_035' {
        Write-RoundtripRecord -Name 'actual-writer' -Value @{ status = 'replacement' }
    } 'exists|exist'
    Assert-RoundtripEqual $writtenHash (Get-RoundtripFileHash (Join-Path $temporary 'driver-actual-writer.json')) 'create-new retained original record'
    Check-Passes 'Sentinel_DistinctKinds_036' { Assert-RoundtripSentinelKinds $manifest.sharedSentinels }
    Check-Rejects 'Sentinel_DuplicateKinds_037' {
        $copy = Copy-TestValue $manifest.sharedSentinels; $copy[1].kind = 'cli'; Assert-RoundtripSentinelKinds $copy
    } 'exactly one'
    Check-Rejects 'Sentinel_TraversalAlias_038' {
        $copy = Copy-TestValue $manifest.sharedSentinels; $copy[1].path = 'C:\synthetic\shared\..\CC Desk\sentinel.txt'; Assert-RoundtripSentinelKinds $copy
    } 'normal absolute'
    # Compile the actual helper source, then invoke only its pure path guard.
    # No P/Invoke, source process, installer, registry or profile operation runs.
    if (-not ('RoundtripNative' -as [type])) {
        $nativeSource = @($uiAst.FindAll({ param($node)
            $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.Contains('public static class RoundtripNative')
        }, $true))
        if ($nativeSource.Count -ne 1) { throw 'Actual native helper source missing or duplicated' }
        Add-Type -TypeDefinition $nativeSource[0].Value
    }
    $canonicalVolume = '\\?\Volume{11111111-2222-4333-8444-555555555555}\'
    $protectedRoots = [string[]]@('install', 'desk', 'webview', 'recovery', 'control', 'evidence' | ForEach-Object { $canonicalVolume + $_ })
    Check-Passes 'Sentinel_ActualGuardAdmitsOutside_039' {
        [RoundtripNative]::RequireSentinelDisjoint($canonicalVolume + 'shared-cli\sentinel.txt', $protectedRoots)
    }
    foreach ($index in 0..5) {
        Check-Rejects "Sentinel_ActualGuardRejectsProtected_$index" {
            [RoundtripNative]::RequireSentinelDisjoint($protectedRoots[$index] + '\sentinel.txt', $protectedRoots)
        } 'overlaps a protected root'
    }
    Check-Rejects 'Sentinel_ActualGuardRejectsCanonicalCaseAlias_046' {
        [RoundtripNative]::RequireSentinelDisjoint($canonicalVolume + 'RECOVERY\sentinel.txt', $protectedRoots)
    } 'overlaps a protected root'
    Check-Rejects 'Sentinel_ActualGuardRequiresAllRoots_047' {
        [RoundtripNative]::RequireSentinelDisjoint($canonicalVolume + 'outside\sentinel.txt', [string[]]@())
    } 'root contract'
    Check-Passes 'Sentinel_ActualGuardAllowsPrefixSibling_048' {
        [RoundtripNative]::RequireSentinelDisjoint($canonicalVolume + 'recovery-other\sentinel.txt', $protectedRoots)
    }
    Write-Output "POWERSHELL_CONTRACT_PASS checks=$passed nativeExecution=false roundtripAccepted=false"
} finally {
    # The test owns only this newly allocated temporary fixture directory.
    Remove-Item -LiteralPath $temporary -Recurse -Force
}
