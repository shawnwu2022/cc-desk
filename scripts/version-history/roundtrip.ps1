#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$TargetManifest,
    [Parameter(Mandatory)][ValidateSet('success', 'before-installer-resume')][string]$Scenario,
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [ValidateRange(1, 1800)][int]$TimeoutSeconds = 120,
    [switch]$DryRun,
    [string]$DryRunProbe
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Run = $null
$script:TransactionId = $null
$script:Binding = $null
$script:Generation = [uint64]0
$script:EvidenceRoot = $null
$script:NativeLoaded = $false
$script:DriverStarted = [DateTime]::UtcNow
. (Join-Path $PSScriptRoot 'roundtrip-ui.ps1')

function Assert-RoundtripProperty {
    param($Value, [string[]]$Names, [string]$Label)
    if ($null -eq $Value) { throw "INVALID_EVIDENCE: missing $Label" }
    foreach ($name in $Names) {
        if ($Value -is [Collections.IDictionary]) { $has = @($Value.Keys) -ccontains $name }
        else { $has = $null -ne $Value.PSObject.Properties[$name] }
        if (-not $has) { throw "INVALID_EVIDENCE: missing $Label.$name" }
    }
}

function Read-RoundtripJson {
    param([string]$Path, [int]$Maximum = 33554432)
    if (-not [IO.File]::Exists($Path)) { throw "MISSING_REPORT: $Path" }
    if ($script:NativeLoaded) { $bytes = [RoundtripNative]::ReadBoundedFile($Path, $Maximum) }
    else {
        $info = [IO.FileInfo]::new($Path)
        if ($info.Length -gt $Maximum) { throw 'INVALID_EVIDENCE: JSON size bound' }
        $bytes = [IO.File]::ReadAllBytes($Path)
    }
    $text = [Text.UTF8Encoding]::new($false, $true).GetString($bytes)
    $text | ConvertFrom-Json -Depth 128
}

function Get-RoundtripFileHash {
    param([string]$Path)
    if (-not [IO.File]::Exists($Path)) { throw "IMAGE_HASH_MISMATCH: missing $Path" }
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-RoundtripEqual {
    param($Expected, $Actual, [string]$Label)
    # Compare parsed structures including absent/null/empty distinctions; property ordering is irrelevant.
    if ($null -eq $Expected -or $null -eq $Actual) {
        if ($null -ne $Expected -or $null -ne $Actual) { throw "ASSERTION_FAILED: $Label null/existence mismatch" }
        return
    }
    if ($Expected -is [string] -or $Expected -is [ValueType]) {
        if ($Expected -is [string]) { $same = $Actual -is [string] -and [string]::Equals($Expected, $Actual, [StringComparison]::Ordinal) }
        elseif ($Expected -is [bool]) { $same = $Actual -is [bool] -and $Expected -eq $Actual }
        else { $same = $Actual -is [ValueType] -and $Actual -isnot [bool] -and $Expected -eq $Actual }
        if (-not $same) { throw "ASSERTION_FAILED: $Label value mismatch" }
        return
    }
    if ($Expected -is [Collections.IDictionary] -or $Expected -is [pscustomobject]) {
        $keys = if ($Expected -is [Collections.IDictionary]) { @($Expected.Keys) } else { @($Expected.PSObject.Properties.Name) }
        $actualKeys = if ($Actual -is [Collections.IDictionary]) { @($Actual.Keys) } elseif ($Actual -is [pscustomobject]) { @($Actual.PSObject.Properties.Name) } else { throw "ASSERTION_FAILED: $Label object type" }
        if ($keys.Count -ne $actualKeys.Count) { throw "ASSERTION_FAILED: $Label field count" }
        foreach ($key in $keys) {
            if ($actualKeys -cnotcontains $key) { throw "ASSERTION_FAILED: $Label missing field $key" }
            Assert-RoundtripEqual $Expected.$key $Actual.$key "$Label.$key"
        }
        return
    }
    $left = @($Expected); $right = @($Actual)
    if ($left.Count -ne $right.Count) { throw "ASSERTION_FAILED: $Label entry count" }
    for ($i = 0; $i -lt $left.Count; $i++) { Assert-RoundtripEqual $left[$i] $right[$i] "$Label[$i]" }
}

function Assert-RoundtripDescriptor {
    param($Expected, $Actual, [string]$Label, [switch]$RestoredFile)
    $left = [byte[]]@($Expected); $right = [byte[]]@($Actual)
    if ($left.Length -ne $right.Length -or $left.Length -lt 20 -or $left.Length -gt 65536) { throw "ASSERTION_FAILED: $Label descriptor bound" }
    $control = [BitConverter]::ToUInt16($left, 2)
    $after = [BitConverter]::ToUInt16($right, 2)
    $allowed = $RestoredFile -and $left[0] -eq 1 -and ($control -band 0x8004) -eq 0x8004 -and ($control -band 0x400) -eq 0 -and $after -eq ($control -bor 0x400)
    for ($i = 0; $i -lt $left.Length; $i++) {
        if ($left[$i] -ne $right[$i] -and -not ($allowed -and ($i -eq 2 -or $i -eq 3))) { throw "ASSERTION_FAILED: $Label permissions mismatch" }
    }
}

function Assert-RoundtripTree {
    param($Expected, $Actual, [string]$Label, [switch]$LogicalBundle, [switch]$Relocated)
    Assert-RoundtripProperty $Expected @('schema', 'location_identity', 'entries') $Label
    Assert-RoundtripProperty $Actual @('schema', 'location_identity', 'entries') $Label
    Assert-RoundtripEqual $Expected.schema $Actual.schema "$Label.schema"
    if (-not $LogicalBundle -and -not $Relocated) { Assert-RoundtripEqual $Expected.location_identity $Actual.location_identity "$Label.location_identity" }
    $left = @($Expected.entries); $right = @($Actual.entries)
    if ($left.Count -ne $right.Count) { throw "ASSERTION_FAILED: $Label complete inventory count" }
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    for ($i = 0; $i -lt $left.Count; $i++) {
        $a = $left[$i]; $b = $right[$i]
        Assert-RoundtripProperty $a @('metadata', 'sha256') "$Label.entry"
        Assert-RoundtripProperty $a.metadata @('path', 'kind', 'size', 'object_identity', 'link_count', 'permissions') "$Label.metadata"
        if (-not $names.Add([string]$a.metadata.path)) { throw "INVALID_EVIDENCE: $Label duplicate path" }
        foreach ($field in @('path', 'kind', 'size', 'link_count')) { Assert-RoundtripEqual $a.metadata.$field $b.metadata.$field "$Label.$field" }
        if (-not $LogicalBundle) { Assert-RoundtripEqual $a.metadata.object_identity $b.metadata.object_identity "$Label.object_identity" }
        Assert-RoundtripProperty $a.metadata.permissions @('Windows') "$Label.permissions"
        Assert-RoundtripEqual $a.metadata.permissions.Windows.attributes $b.metadata.permissions.Windows.attributes "$Label.attributes"
        Assert-RoundtripDescriptor $a.metadata.permissions.Windows.descriptor $b.metadata.permissions.Windows.descriptor "$Label.descriptor" -RestoredFile:$LogicalBundle
        Assert-RoundtripEqual $a.sha256 $b.sha256 "$Label.sha256"
    }
}

function Get-RoundtripLogicalDigest {
    param($Tree, [string]$ImageName, $ExpectedTree)
    # Rust hashes [1,imageName,[[path,kind,size,link_count,permissions,sha256],...]].
    # Restoration admits only the native AUTO_INHERITED clear-to-set transition.
    # Use the verified original descriptor for that permitted transition; no physical copied-file IDs.
    $entries = [Collections.Generic.List[object]]::new()
    for ($i = 0; $i -lt @($Tree.entries).Count; $i++) {
        $entry = $Tree.entries[$i]
        $descriptor = $entry.metadata.permissions.Windows.descriptor
        if ($ExpectedTree) { $descriptor = $ExpectedTree.entries[$i].metadata.permissions.Windows.descriptor }
        $permission = [RoundtripNative]::Map('Windows', [RoundtripNative]::Map('descriptor', [byte[]]$descriptor, 'attributes', $entry.metadata.permissions.Windows.attributes))
        $entries.Add([object[]]@($entry.metadata.path, $entry.metadata.kind, $entry.metadata.size, $entry.metadata.link_count, $permission, $entry.sha256))
    }
    [RoundtripNative]::Identity([object[]]@(1, $ImageName, $entries))
}

function Assert-RoundtripManifest {
    param($Manifest)
    Assert-RoundtripProperty $Manifest @('schema', 'baseHead', 'buildId', 'runId', 'scenario', 'bindingSha256', 'compiledTargetPath', 'imageSha256', 'packagePath', 'packageSha256', 'provisionEvidencePath', 'provisionEvidenceSha256', 'resetBaselineId', 'sourcePid', 'deskDirectory', 'webViewDirectory', 'nativeProbePath', 'nativeProbeSha256', 'selectors', 'syntheticAssertions', 'sharedSentinels') 'target manifest'
    if ($Manifest.schema -ne 1 -or $Manifest.baseHead -cne '9c981a5093a80b947817af8eebe4855293690185' -or $Manifest.buildId -cnotmatch '^[0-9a-f]{40}$' -or $Manifest.buildId -ceq $Manifest.baseHead) { throw 'TARGET_MISMATCH: exact base and distinct resulting acceptance source SHA required' }
    if ($Manifest.scenario -cne $Scenario) { throw 'SCENARIO_MISMATCH: runtime input differs from reviewed build' }
    foreach ($field in @('bindingSha256', 'imageSha256', 'packageSha256', 'provisionEvidenceSha256', 'nativeProbeSha256')) {
        if ($Manifest.$field -cnotmatch '^[0-9a-f]{64}$') { throw "TARGET_MISMATCH: missing exact $field" }
    }
    if ((Get-RoundtripFileHash $Manifest.compiledTargetPath) -cne $Manifest.bindingSha256) { throw 'TARGET_MISMATCH: compiled target bytes changed' }
    $binding = Read-RoundtripJson $Manifest.compiledTargetPath
    Assert-RoundtripProperty $binding @('schema', 'enabled', 'baseHead', 'buildId', 'runId', 'scenario', 'targetSid', 'profileDirectory', 'installDirectory', 'evidenceDirectory') 'compiled target'
    if ($binding.schema -ne 1 -or $binding.enabled -ne $true) { throw 'BLOCKED_EXTERNAL_TARGET: deny-only or missing reviewed disposable build binding' }
    foreach ($field in @('baseHead', 'buildId', 'runId', 'scenario')) {
        if ($binding.$field -cne $Manifest.$field) { throw "TARGET_MISMATCH: compiled $field differs from reviewed build" }
    }
    if ($Manifest.runId -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$' -or [string]::IsNullOrEmpty($Manifest.resetBaselineId) -or [int]$Manifest.sourcePid -le 0) { throw 'TARGET_MISMATCH: run/baseline/source identity missing' }
    if (-not [string]::Equals($binding.evidenceDirectory.TrimEnd('\'), $EvidenceDirectory.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) { throw 'TARGET_MISMATCH: evidence directory differs from compiled target' }
    if ((Get-RoundtripFileHash $Manifest.provisionEvidencePath) -cne $Manifest.provisionEvidenceSha256) { throw 'BLOCKED_EXTERNAL_TARGET: reviewed provisioning record changed' }
    $provision = Read-RoundtripJson $Manifest.provisionEvidencePath
    Assert-RoundtripProperty $provision @('approvedDisposableTarget', 'knownSyntheticState', 'ntfs', 'webView2', 'targetSid', 'profileDirectory', 'installDirectory', 'resetBaselineId', 'sourceLaunchMethod') 'external provisioning'
    foreach ($field in @('approvedDisposableTarget', 'knownSyntheticState', 'ntfs', 'webView2')) { if ($provision.$field -ne $true) { throw "BLOCKED_EXTERNAL_TARGET: external $field is unavailable" } }
    foreach ($field in @('targetSid', 'profileDirectory', 'installDirectory')) { if ($provision.$field -cne $binding.$field) { throw "TARGET_MISMATCH: provisioning $field differs" } }
    if ($provision.resetBaselineId -cne $Manifest.resetBaselineId -or $provision.sourceLaunchMethod -cne 'native-interactive') { throw 'BLOCKED_EXTERNAL_TARGET: independent reset and interactive source launch not established' }
    if ((Get-RoundtripFileHash $Manifest.packagePath) -cne $Manifest.packageSha256) { throw 'IMAGE_HASH_MISMATCH: acceptance NSIS package differs' }
    if ((Get-RoundtripFileHash $Manifest.nativeProbePath) -cne $Manifest.nativeProbeSha256) { throw 'BLOCKED_UI_SELECTOR: native probe evidence changed' }
    $probe = Read-RoundtripJson $Manifest.nativeProbePath
    Assert-RoundtripProperty $probe @('targetSid', 'runId', 'imageSha256', 'selectors') 'native UI probe'
    if ($probe.targetSid -cne $binding.targetSid -or $probe.runId -cne $Manifest.runId -or $probe.imageSha256 -cne $Manifest.imageSha256) { throw 'BLOCKED_UI_SELECTOR: probe does not belong to this target/build/run' }
    Assert-RoundtripEqual $probe.selectors $Manifest.selectors 'native probed selectors'
    $required = @('settings', 'updates', 'selectVersion', 'prepare', 'review', 'begin', 'managerReturn', 'managerRestore', 'restoredStatus', 'restoredNormal', 'sourceOriginalPreference', 'restoredOriginalPreference', 'sourceNativeMetadata', 'restoredNativeMetadata')
    if ($Scenario -eq 'success') { $required += @('historicalFresh', 'historicalPreferenceOpen', 'historicalPreference', 'historicalPreferenceChanged', 'managerConfirm', 'managerConfirmSubmit', 'managerConfirmed') }
    else { $required += @('managerRecoveryRequired') }
    foreach ($name in $required) {
        if (-not $Manifest.selectors.PSObject.Properties[$name]) { throw "BLOCKED_UI_SELECTOR: missing $name" }
        Assert-RoundtripProperty $Manifest.selectors.$name @('name', 'controlType', 'automationId') "selector $name"
        if ([string]::IsNullOrEmpty($Manifest.selectors.$name.name) -or [int]$Manifest.selectors.$name.controlType -lt 50000 -or [int]$Manifest.selectors.$name.controlType -gt 50040) { throw "BLOCKED_UI_SELECTOR: unobserved $name" }
    }
    if ($Manifest.selectors.selectVersion.name -cne 'Select version 0.17.7' -or $Manifest.selectors.managerReturn.name -cne 'Return to previous version' -or $Manifest.selectors.managerRestore.name -cne 'Restore previous version') { throw 'BLOCKED_UI_SELECTOR: exact reviewed English source/manager controls required' }
    Assert-RoundtripSentinelKinds $Manifest.sharedSentinels
    $Manifest | Add-Member -NotePropertyName compiledTarget -NotePropertyValue $binding
    return $Manifest
}

function Assert-RoundtripSentinelKinds {
    param($Sentinels)
    $values = @($Sentinels)
    if ($values.Count -ne 2 -or @($values | Where-Object kind -CEQ 'cli').Count -ne 1 -or @($values | Where-Object kind -CEQ 'project').Count -ne 1) {
        throw 'TARGET_MISMATCH: exactly one synthetic CLI and one project sentinel required'
    }
    foreach ($value in $values) {
        Assert-RoundtripProperty $value @('path') 'synthetic sentinel'
        if ($value.path -cnotmatch '^[A-Za-z]:\\' -or $value.path -match '(^|[\\/])\.{1,2}([\\/]|$)' -or $value.path.Contains('/')) {
            throw 'TARGET_MISMATCH: sentinel path must be a normal absolute drive path'
        }
    }
}

function Assert-RoundtripReport {
    param($Report, [string]$Stage)
    Assert-RoundtripProperty $Report @('schema', 'baseHead', 'buildId', 'runId', 'scenario', 'bindingSha256', 'transactionId', 'generation', 'stage', 'value') "report $Stage"
    if ($Report.schema -ne 1 -or $Report.stage -cne $Stage) { throw "INVALID_EVIDENCE: wrong schema/stage $Stage" }
    foreach ($field in @('baseHead', 'buildId', 'runId', 'scenario', 'bindingSha256')) { if ($Report.$field -cne $script:Run.$field) { throw "INVALID_EVIDENCE: foreign report $field" } }
    if ($Report.transactionId -cnotmatch '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$') { throw 'INVALID_EVIDENCE: missing transaction UUID' }
    Assert-RoundtripProperty $Report.value @('binding') "report $Stage value"
    Assert-RoundtripProperty $Report.value.binding @('transaction_id', 'source_context', 'target_context', 'user_installation', 'source_bundle', 'target_package', 'target_payload', 'roots') "report $Stage binding"
    if ($Report.value.binding.transaction_id -cne $Report.transactionId -or [uint64]$Report.generation -eq 0) { throw 'INVALID_EVIDENCE: report transaction/generation differs' }
    if ($script:TransactionId -and $Report.transactionId -cne $script:TransactionId) { throw 'INVALID_EVIDENCE: foreign transaction report' }
    if ($script:Binding) { Assert-RoundtripEqual $script:Binding $Report.value.binding 'all observation stage bindings' }
    if ([uint64]$Report.generation -lt $script:Generation) { throw 'INVALID_EVIDENCE: observation generation regressed' }
    $script:TransactionId = $Report.transactionId
    $script:Binding = $Report.value.binding
    $script:Generation = [uint64]$Report.generation
    return $Report
}

function Wait-RoundtripReport {
    param([string]$Stage)
    $path = Join-Path $script:EvidenceRoot "$($script:Run.runId)-$Stage.json"
    Wait-RoundtripCondition -TimeoutSeconds $TimeoutSeconds -Description "required report $Stage (MISSING_REPORT if absent)" -Check {
        if ([IO.File]::Exists($path)) { Assert-RoundtripReport (Read-RoundtripJson $path) $Stage }
    }
}

function Write-RoundtripRecord {
    param([string]$Name, $Value)
    $path = Join-Path $script:EvidenceRoot "driver-$Name.json"
    $record = [ordered]@{ schema = 1; baseHead = $script:Run.baseHead; buildId = $script:Run.buildId; runId = $script:Run.runId; scenario = $Scenario; bindingSha256 = $script:Run.bindingSha256; transactionId = $script:TransactionId; recordedUtc = [DateTime]::UtcNow.ToString('o'); value = $Value }
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($record | ConvertTo-Json -Depth 100 -Compress))
    if ($bytes.Length -gt 67108864) { throw 'INVALID_EVIDENCE: driver record bound' }
    $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
}

function Assert-RoundtripProcess {
    param([Diagnostics.Process]$Process, [string]$ImagePath, [string]$ImageHash, [string]$Version, [switch]$JobFree)
    $facts = [RoundtripNative]::ProcessFacts([uint32]$Process.Id)
    if ($facts.userSid -cne $script:Run.compiledTarget.targetSid -or $facts.elevated -ne $false -or ($JobFree -and $facts.inAnyJob -ne $false)) { throw 'BLOCKED_EXTERNAL_TARGET: actual source token or external-job status is not eligible' }
    if (-not [string]::Equals($facts.imagePath, $ImagePath, [StringComparison]::OrdinalIgnoreCase)) { throw 'TARGET_MISMATCH: actual process image path differs' }
    if ((Get-RoundtripFileHash $facts.imagePath) -cne $ImageHash) { throw 'IMAGE_HASH_MISMATCH: actual process image differs' }
    if ($Version -and [Diagnostics.FileVersionInfo]::GetVersionInfo($facts.imagePath).ProductVersion -cne $Version) { throw 'TARGET_MISMATCH: actual image version differs' }
    return $facts
}

function Assert-RoundtripCreation {
    param($Creation, $Facts, [string]$Kind)
    Assert-RoundtripProperty $Creation @('schema', 'launch', 'intent', 'process', 'command_digest', 'job', 'job_phase', 'lease') 'durable native creation receipt'
    Assert-RoundtripProperty $Creation.process @('pid', 'created', 'session', 'image', 'image_digest') 'created native process'
    if ($Creation.schema -ne 3 -or $Creation.job.kind -cne $Kind -or $Creation.job.owner -cne $script:Run.compiledTarget.targetSid -or $Creation.job_phase -cne 'armedPreparation') { throw 'INVALID_EVIDENCE: foreign/unowned process creation' }
    Assert-RoundtripEqual $Creation.process.pid $Facts.pid 'actual created PID'
    Assert-RoundtripEqual $Creation.process.created $Facts.createdFileTime 'actual process creation epoch'
    Assert-RoundtripEqual $Creation.process.image_digest (Get-RoundtripFileHash $Facts.imagePath) 'actual creation image digest'
    if ($Facts.inAnyJob -ne $true) { throw 'ASSERTION_FAILED: observed owned child is outside the native job' }
}

function Get-RoundtripSingleProcess {
    param([string]$Name, [string]$CommandSuffix)
    $candidates = @(Get-CimInstance Win32_Process -Filter "Name='$Name'")
    if ($candidates.Count -gt 1) { throw "DUPLICATE_PROCESS: $Name must have exactly one process" }
    if ($candidates.Count -eq 0) { return $null }
    if ($CommandSuffix -and $candidates[0].CommandLine -cnotmatch ('(?:^|\s)--version-manager\s+' + [regex]::Escape($CommandSuffix) + '\s*$')) { throw 'TARGET_MISMATCH: manager command line transaction differs' }
    [Diagnostics.Process]::GetProcessById([int]$candidates[0].ProcessId)
}

function Get-RoundtripIndependentCapture {
    param($M0, $Final)
    $install = [RoundtripNative]::CaptureTree($script:Run.compiledTarget.installDirectory)
    $desk = [RoundtripNative]::CaptureTree($script:Run.deskDirectory)
    $webview = [RoundtripNative]::CaptureTree($script:Run.webViewDirectory)
    $registration = [RoundtripNative]::CaptureRegistration($script:Run.compiledTarget.targetSid, $install.fileIdentity)
    $shortcuts = @([RoundtripNative]::CaptureShortcut('Desktop'), [RoundtripNative]::CaptureShortcut('StartMenu'))
    # Repeat complete bounded captures before releasing this verification barrier.
    $second = [RoundtripNative]::CaptureTree($script:Run.compiledTarget.installDirectory)
    Assert-RoundtripEqual $install $second 'independent bundle capture stability'
    Assert-RoundtripEqual $desk ([RoundtripNative]::CaptureTree($script:Run.deskDirectory)) 'independent Desk capture stability'
    Assert-RoundtripEqual $webview ([RoundtripNative]::CaptureTree($script:Run.webViewDirectory)) 'independent WebView capture stability'
    Assert-RoundtripEqual $registration ([RoundtripNative]::CaptureRegistration($script:Run.compiledTarget.targetSid, $second.fileIdentity)) 'independent registration capture stability'
    Assert-RoundtripEqual $shortcuts @([RoundtripNative]::CaptureShortcut('Desktop'), [RoundtripNative]::CaptureShortcut('StartMenu')) 'independent shortcut capture stability'
    Assert-RoundtripProperty $Final.value @('markerLogPath', 'journalLogPath', 'journalHead') 'native terminal log locations'
    $markerBytes = [RoundtripNative]::ReadBoundedFile($Final.value.markerLogPath, 67108864)
    $journalBytes = [RoundtripNative]::ReadBoundedFile($Final.value.journalLogPath, 67108864)
    $terminalLogs = Assert-RoundtripTerminalLogs $Final $markerBytes $journalBytes
    Assert-RoundtripEqual (Get-RoundtripBytesHash $markerBytes) (Get-RoundtripBytesHash ([RoundtripNative]::ReadBoundedFile($Final.value.markerLogPath, 67108864))) 'independent marker log stability'
    Assert-RoundtripEqual (Get-RoundtripBytesHash $journalBytes) (Get-RoundtripBytesHash ([RoundtripNative]::ReadBoundedFile($Final.value.journalLogPath, 67108864))) 'independent journal log stability'
    [ordered]@{ bundle = $install.manifest; logicalDigest = Get-RoundtripLogicalDigest $install.manifest $M0.value.bundle.original_image_name $M0.value.bundle.tree; desk = $desk.manifest; webView = $webview.manifest; registration = $registration; shortcuts = $shortcuts; terminalLogs = $terminalLogs }
}

function Get-RoundtripBytesHash {
    param([byte[]]$Bytes)
    $hash = [Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($hash.ComputeHash($Bytes)).Replace('-', '').ToLowerInvariant() } finally { $hash.Dispose() }
}

function Assert-RoundtripLogChain {
    param([byte[]]$Bytes, [ValidateSet('frame', 'record')][string]$PayloadName, $Binding)
    if (-not $Bytes -or $Bytes.Length -gt 67108864 -or $Bytes[-1] -ne 10) { throw 'INVALID_EVIDENCE: missing/oversized/torn terminal log tail' }
    $text = [Text.UTF8Encoding]::new($false, $true).GetString($Bytes)
    $lines = $text.Substring(0, $text.Length - 1).Split("`n")
    $limit = if ($PayloadName -eq 'frame') { 4096 } else { 100000 }
    $recordBound = if ($PayloadName -eq 'frame') { 16384 } else { 131072 }
    if ($lines.Count -gt $limit) { throw 'INVALID_EVIDENCE: terminal log record count bound' }
    $previous = $null; $last = $null; $markerGeneration = [uint64]0
    $events = @('RetainSourcePartial', 'AdmitBundleStart', 'PrepareBundleBackup', 'Begin', 'PrivateBackupPlan', 'PrepareLaterBackup', 'CompleteLaterBackup', 'AdmitPreinstallReturn', 'ConfirmContextRoot', 'PrepareRootBackup', 'ConfirmRootReturned', 'AdmitRootReverse', 'Manifest', 'Intent', 'Observed', 'Phase', 'AbortPreContext', 'CompensateUnknown')
    $pending = [Collections.Generic.Dictionary[string, uint64]]::new([StringComparer]::Ordinal)
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if ([Text.Encoding]::UTF8.GetByteCount($line) + 1 -gt $recordBound) { throw 'INVALID_EVIDENCE: terminal log record byte bound' }
        $match = [regex]::Match($line, '^\{"' + $PayloadName + '":(?<payload>\{.*\}),"digest":"(?<digest>[0-9a-f]{64})"\}$')
        if (-not $match.Success) { throw 'INVALID_EVIDENCE: unknown/noncanonical terminal log envelope' }
        $raw = $match.Groups['payload'].Value
        $digest = Get-RoundtripBytesHash ([Text.UTF8Encoding]::new($false).GetBytes($raw))
        if ($digest -cne $match.Groups['digest'].Value) { throw 'INVALID_EVIDENCE: terminal log raw payload digest mismatch' }
        $payload = $raw | ConvertFrom-Json -Depth 128
        Assert-RoundtripEqual $previous $payload.previous 'terminal raw log chain predecessor'
        if ($PayloadName -eq 'frame') {
            Assert-RoundtripProperty $payload @('schema', 'sequence', 'previous', 'marker') 'native marker frame'
            if (@($payload.PSObject.Properties).Count -ne 4 -or $payload.schema -ne 1 -or [uint64]$payload.sequence -ne $i) { throw 'INVALID_EVIDENCE: marker frame schema/sequence' }
            Assert-RoundtripEqual $Binding $payload.marker.binding 'native marker log transaction binding'
            if ($payload.marker.state -cnotin @('Transition', 'Restored', 'PreContextAborted') -or [uint64]$payload.marker.generation -lt $markerGeneration) { throw 'INVALID_EVIDENCE: unknown/regressed marker state' }
            $markerGeneration = [uint64]$payload.marker.generation
        } else {
            Assert-RoundtripProperty $payload @('schema', 'binding', 'generation', 'previous', 'lane', 'event') 'native journal record'
            if (@($payload.PSObject.Properties).Count -ne 6 -or $payload.schema -ne 2 -or [uint64]$payload.generation -ne $i) { throw 'INVALID_EVIDENCE: journal schema/generation' }
            Assert-RoundtripEqual $Binding $payload.binding 'native journal log transaction binding'
            $names = @($payload.event.PSObject.Properties.Name)
            if ($names.Count -ne 1 -or $names[0] -cnotin $events -or ($i -eq 0 -and ($names[0] -cne 'Begin' -or $null -ne $payload.lane)) -or ($i -gt 0 -and $payload.lane -cnotin @('Forward', 'Recovery'))) { throw 'INVALID_EVIDENCE: unknown journal event/lane' }
            if ($names[0] -ceq 'Observed' -and $payload.event.Observed.result.observation -ceq 'Unknown') { throw 'INVALID_EVIDENCE: unknown transaction outcome is outside uninterrupted acceptance' }
            if ($names[0] -ceq 'Intent') {
                $effect = [string]$payload.event.Intent.effect.effect_id
                if ([string]::IsNullOrEmpty($effect) -or $pending.ContainsKey($effect)) { throw 'INVALID_EVIDENCE: duplicated/missing native effect intent' }
                $pending.Add($effect, [uint64]$payload.generation)
            }
            if ($names[0] -ceq 'Observed') {
                $effect = [string]$payload.event.Observed.effect_id
                if (-not $pending.ContainsKey($effect) -or $pending[$effect] -ne [uint64]$payload.event.Observed.intent_generation -or $payload.event.Observed.result.observation -cnotin @('Applied', 'NotApplied')) { throw 'INVALID_EVIDENCE: unknown or unmatched native effect receipt' }
                $pending.Remove($effect) | Out-Null
            }
        }
        $previous = $digest; $last = $payload
    }
    if ($PayloadName -eq 'record' -and $pending.Count -ne 0) { throw 'INVALID_EVIDENCE: pending native transaction intent remains' }
    [ordered]@{ head = $previous; records = $lines.Count; rawSha256 = Get-RoundtripBytesHash $Bytes; last = $last }
}

function Assert-RoundtripTerminalLogs {
    param($Final, [byte[]]$MarkerBytes, [byte[]]$JournalBytes)
    Assert-RoundtripProperty $Final.value @('journalHead', 'marker', 'binding') 'final terminal chain'
    $marker = Assert-RoundtripLogChain $MarkerBytes 'frame' $Final.value.binding
    $journal = Assert-RoundtripLogChain $JournalBytes 'record' $Final.value.binding
    Assert-RoundtripEqual $Final.value.marker $marker.last.marker 'independent terminal marker readback'
    Assert-RoundtripEqual $Final.generation $journal.last.generation 'independent terminal journal generation'
    if ($journal.last.event.PSObject.Properties.Name -cnotcontains 'Phase' -or $journal.last.event.Phase.phase -cne 'Restored' -or $marker.last.marker.state -cne 'Restored') { throw 'INVALID_EVIDENCE: nonterminal native journal/marker tail' }
    Assert-RoundtripEqual $Final.value.journalHead $journal.head 'independent terminal journal head'
    Assert-RoundtripEqual $journal.head $marker.last.marker.journal_digest 'independent marker journal head'
    [ordered]@{ markerRawSha256 = $marker.rawSha256; markerRecords = $marker.records; markerHead = $marker.head; journalRawSha256 = $journal.rawSha256; journalRecords = $journal.records; journalHead = $journal.head; generation = $journal.last.generation; state = 'Restored' }
}

function Assert-RoundtripRestoredCapture {
    param($M0, $Final, $Capture)
    foreach ($field in @('binding', 'bundle', 'bundleLogicalDigest', 'context', 'registration', 'shortcuts', 'dataRoot')) { Assert-RoundtripProperty $M0.value @($field) 'M0' }
    Assert-RoundtripProperty $Final.value @('binding', 'phase', 'pending', 'marker', 'bundle', 'sourceBundle', 'bundleLogicalDigest', 'context', 'registration', 'shortcuts', 'retainedContext', 'retainedBundle', 'dataRoot', 'laterContextDirectory') 'FinalRestored'
    Assert-RoundtripEqual $M0.value.binding $Final.value.binding 'final transaction binding'
    Assert-RoundtripProperty $Final.value.marker @('schema', 'binding', 'generation', 'journal_digest', 'state') 'terminal marker'
    if ($Final.value.marker.schema -ne 1 -or $Final.value.marker.journal_digest -cnotmatch '^[0-9a-f]{64}$') { throw 'ASSERTION_FAILED: incomplete terminal marker' }
    if ($Final.value.phase -cne 'Restored' -or $Final.value.pending -ne $false -or $Final.value.marker.state -cne 'Restored') { throw 'ASSERTION_FAILED: nonterminal final journal/marker' }
    Assert-RoundtripEqual $Final.value.binding $Final.value.marker.binding 'terminal marker binding'
    Assert-RoundtripEqual $Final.generation $Final.value.marker.generation 'terminal marker generation'
    Assert-RoundtripEqual $M0.value.bundleLogicalDigest $Final.value.bundleLogicalDigest 'manager source bundleLogicalDigest'
    Assert-RoundtripEqual $M0.value.bundle.original_image_name $Final.value.sourceBundle.original_image_name 'original image name'
    Assert-RoundtripTree $M0.value.bundle.tree $Final.value.sourceBundle.tree 'complete typed original bundle' -LogicalBundle
    Assert-RoundtripEqual $M0.value.binding.source_bundle $M0.value.bundleLogicalDigest 'journal original bundle identity'
    Assert-RoundtripEqual $M0.value.bundleLogicalDigest $Capture.logicalDigest 'independent source logicalDigest'
    Assert-RoundtripTree $M0.value.bundle.tree $Capture.bundle 'complete original bundle' -LogicalBundle
    Assert-RoundtripTree $Final.value.bundle $Capture.bundle 'manager final bundle readback' -LogicalBundle
    foreach ($root in @('Desk', 'WebView')) {
        $original = @($M0.value.context.roots | Where-Object root -CEQ $root)
        $verified = @($Final.value.context.roots | Where-Object root -CEQ $root)
        if ($original.Count -ne 1 -or $verified.Count -ne 1) { throw 'ASSERTION_FAILED: complete source context roots missing/duplicated' }
        Assert-RoundtripEqual $original[0] $verified[0] "manager original $root context"
        $expected = [ordered]@{ schema = 1; location_identity = $original[0].location_identity; entries = $original[0].entries }
        $actual = if ($root -eq 'Desk') { $Capture.desk } else { $Capture.webView }
        Assert-RoundtripTree $expected $actual "independent original $root context"
    }
    Assert-RoundtripEqual $M0.value.context.context_id $Final.value.context.context_id 'original context identity'
    Assert-RoundtripEqual $M0.value.context.context_id $M0.value.binding.source_context 'source context role binding'
    Assert-RoundtripEqual $M0.value.registration.snapshot $Final.value.registration.snapshot 'manager complete registration'
    Assert-RoundtripEqual $M0.value.registration.snapshot $Capture.registration 'independent complete registration and owned Run'
    $originalLinks = @($M0.value.shortcuts.entries); $actualLinks = @($Capture.shortcuts)
    Assert-RoundtripEqual $M0.value.shortcuts $Final.value.shortcuts 'manager complete original shortcut manifest'
    if ($originalLinks.Count -ne 2 -or $actualLinks.Count -ne 2) { throw 'ASSERTION_FAILED: both shortcut slots required' }
    for ($i = 0; $i -lt 2; $i++) {
        $a = $originalLinks[$i]; $b = $actualLinks[$i]
        foreach ($field in @('slot', 'parent', 'parent_path', 'leaf')) { Assert-RoundtripEqual $a.$field $b.$field "shortcut $field" }
        if ($a.state -is [string]) { Assert-RoundtripEqual $a.state $b.state 'shortcut absence' }
        else {
            Assert-RoundtripProperty $b.state @('Present') 'shortcut present'
            foreach ($field in @('attributes', 'bytes', 'sha256')) { Assert-RoundtripEqual $a.state.Present.$field $b.state.Present.$field "shortcut $field" }
            Assert-RoundtripDescriptor $a.state.Present.descriptor $b.state.Present.descriptor 'shortcut permissions' -RestoredFile
        }
    }
    $present = @($originalLinks | Where-Object { $_.state -isnot [string] }).Count
    if (($Scenario -eq 'success' -and $present -ne 2) -or ($Scenario -eq 'before-installer-resume' -and $present -ne 1)) { throw 'ASSERTION_FAILED: scenario shortcut baseline differs' }
    $retained = @($Final.value.retainedContext)
    if ($retained.Count -ne 4 -or $retained[0] -ne 1 -or $retained[2] -cne $M0.value.binding.target_context -or @($retained[3]).Count -ne 2) { throw 'ASSERTION_FAILED: missing complete retained target context' }
    Assert-RoundtripEqual $M0.value.binding $retained[1] 'retained target binding'
}

function Assert-RoundtripRetained {
    param($M0, $Final, $Later, $Target)
    $tuple = @($Final.value.retainedContext)
    if ($tuple.Count -ne 4 -or $tuple[0] -ne 1 -or $tuple[2] -cne $Final.value.binding.target_context) { throw 'ASSERTION_FAILED: retained target context role' }
    Assert-RoundtripEqual $tuple[1] $Final.value.binding 'retained context binding'
    if ($Later) { Assert-RoundtripEqual $Later.value.retainedContext $Final.value.retainedContext 'complete retained later context' }
    $roots = @($tuple[3]); if ($roots.Count -ne 2) { throw 'ASSERTION_FAILED: both retained context roots required' }
    foreach ($pair in $roots) {
        $root = [string]$pair[0]
        if ($root -cnotin @('Desk', 'WebView')) { throw 'ASSERTION_FAILED: unknown retained context role' }
        Assert-RoundtripProperty $Final.value @('retainedContextLocations') 'final native retained context locations'
        Assert-RoundtripProperty $Final.value.retainedContextLocations @($root) 'retained native root path'
        $location = $Final.value.retainedContextLocations.$root
        $actual = [RoundtripNative]::CaptureTree($location)
        Assert-RoundtripTree $pair[1] $actual.manifest "retained $root bytes/identities/permissions" -Relocated
    }
    Assert-RoundtripProperty $Final.value.retainedBundle @('source', 'copy') 'retained target bundle'
    if ($Target) { Assert-RoundtripTree $Target.value.bundle.tree $Final.value.retainedBundle.source 'complete retained target bundle source' -LogicalBundle }
    # The copy location is separately captured from the durable PrivateCopyManifest root identity.
    Assert-RoundtripProperty $script:Run.syntheticAssertions @('originalFiles', 'laterFiles') 'synthetic assertions'
    Assert-RoundtripProperty $Final.value @('retainedBundleDirectory') 'final native retained bundle location'
    $copy = [RoundtripNative]::CaptureTree($Final.value.retainedBundleDirectory)
    Assert-RoundtripTree $Final.value.retainedBundle.copy $copy.manifest 'independent complete retained target bundle' -Relocated
    foreach ($assertion in @($script:Run.syntheticAssertions.originalFiles)) {
        Assert-RoundtripSyntheticFile $assertion $false
    }
    if ($Scenario -eq 'success') {
        if (@($script:Run.syntheticAssertions.laterFiles).Count -eq 0) { throw 'ASSERTION_FAILED: later preference/sentinel evidence missing' }
        foreach ($assertion in @($script:Run.syntheticAssertions.laterFiles)) { Assert-RoundtripSyntheticFile $assertion $true }
    }
    foreach ($sentinel in @($script:Run.sharedSentinels)) { Assert-RoundtripEqual $sentinel.afterSha256 (Get-RoundtripFileHash $sentinel.path) 'shared synthetic newer bytes survive Return' }
}

function Assert-RoundtripSyntheticFile {
    param($Assertion, [bool]$Later)
    Assert-RoundtripProperty $Assertion @('restoredPath', 'retainedPath', 'sha256') 'synthetic file assertion'
    $presentPath = if ($Later) { $Assertion.retainedPath } else { $Assertion.restoredPath }
    $absentPath = if ($Later) { $Assertion.restoredPath } else { $Assertion.retainedPath }
    Assert-RoundtripEqual $Assertion.sha256 (Get-RoundtripFileHash $presentPath) 'synthetic distinguishable file bytes'
    if ([IO.File]::Exists($absentPath)) { throw 'ASSERTION_FAILED: synthetic original/later file crossed contexts' }
}

function Set-RoundtripSharedSentinels {
    param($M0)
    Assert-RoundtripProperty $M0.value @('dataRoot', 'controlDirectory') 'actual source recovery locations'
    Assert-RoundtripSentinelKinds $script:Run.sharedSentinels
    $protected = [string[]]@($script:Run.compiledTarget.installDirectory, $script:Run.deskDirectory, $script:Run.webViewDirectory, $M0.value.dataRoot, $M0.value.controlDirectory, $script:Run.compiledTarget.evidenceDirectory)
    foreach ($sentinel in @($script:Run.sharedSentinels)) {
        Assert-RoundtripProperty $sentinel @('kind', 'path', 'beforeSha256', 'afterSha256', 'afterUtf8') 'shared sentinel'
        if ($sentinel.kind -cnotin @('cli', 'project')) { throw 'TARGET_MISMATCH: only synthetic CLI/project sentinels allowed' }
        Assert-RoundtripEqual $sentinel.beforeSha256 (Get-RoundtripFileHash $sentinel.path) 'synthetic shared baseline'
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($sentinel.afterUtf8)
        Assert-RoundtripEqual $sentinel.afterSha256 ([RoundtripNative]::Hash($bytes)) 'synthetic shared reviewed change'
        # Only reviewed harmless synthetic sentinels are edited; no CLI execution or session.
        [RoundtripNative]::WriteSyntheticSentinel($sentinel.path, $sentinel.beforeSha256, $bytes, $protected)
    }
}

function Start-RoundtripRestoredApp {
    param($M0)
    if (Get-RoundtripSingleProcess 'cc-desk.exe') { throw 'DUPLICATE_PROCESS: restored image already running before verified reopen' }
    $path = Join-Path $script:Run.compiledTarget.installDirectory 'cc-desk.exe'
    $process = Start-Process -FilePath $path -WorkingDirectory $script:Run.compiledTarget.installDirectory -PassThru
    $facts = Assert-RoundtripProcess $process $path $script:Run.imageSha256 '0.18.0' -JobFree
    Wait-RoundtripControl $process $script:Run.selectors.restoredNormal $TimeoutSeconds | Out-Null
    Wait-RoundtripControl $process $script:Run.selectors.restoredNativeMetadata $TimeoutSeconds | Out-Null
    Invoke-RoundtripControl $process $script:Run.selectors.settings $TimeoutSeconds
    Wait-RoundtripControl $process $script:Run.selectors.restoredOriginalPreference $TimeoutSeconds | Out-Null
    Save-RoundtripScreenshot $process (Join-Path $script:EvidenceRoot 'driver-restored-normal-ui.png')
    Write-RoundtripRecord 'restored-process' $facts
    return $process
}

try {
    $script:Run = Assert-RoundtripManifest (Read-RoundtripJson $TargetManifest)
    if ($DryRun) {
        # Read-only contract checks only. Synthetic probes cannot execute or certify a transaction.
        if ($DryRunProbe) {
            $probe = Read-RoundtripJson $DryRunProbe
            Assert-RoundtripProperty $probe @('sourceCount', 'imageSha256', 'controlsAvailable', 'timedOut', 'reports') 'dry-run probe'
            if ($probe.sourceCount -ne 1) { throw 'DUPLICATE_PROCESS: dry-run source count' }
            if ($probe.imageSha256 -cne $script:Run.imageSha256) { throw 'IMAGE_HASH_MISMATCH: dry-run image' }
            if ($probe.controlsAvailable -ne $true) { throw 'BLOCKED_UI_SELECTOR: dry-run missing native controls' }
            if ($probe.timedOut -ne $false) { throw 'WAIT_TIMEOUT: dry-run bounded wait' }
            $stages = if ($Scenario -eq 'success') { @('M0', 'SourceSealed', 'FreshReady', 'TargetVerified', 'HistoricalLaunched', 'LaterCaptured', 'FinalRestored') } else { @('M0', 'SourceSealed', 'FreshReady', 'InstallerSuspended', 'InjectedPreResumeFailure', 'CancelledBeforeResume', 'FinalRestored') }
            foreach ($stage in $stages) {
                $reports = @($probe.reports | Where-Object stage -CEQ $stage)
                if ($reports.Count -ne 1) { throw "MISSING_REPORT: dry-run $stage missing or duplicated" }
                Assert-RoundtripReport $reports[0] $stage | Out-Null
            }
        }
        [ordered]@{ status = 'DRY_RUN_VALIDATED'; nativeExecution = $false; roundtripAccepted = $false; scenario = $Scenario; runId = $script:Run.runId } | ConvertTo-Json -Compress
        exit 0
    }
    if (-not $IsWindows -or -not [Environment]::Is64BitProcess -or -not [Environment]::UserInteractive) { throw 'BLOCKED_EXTERNAL_TARGET: interactive Windows x64 desktop required' }
    Initialize-RoundtripNative
    $script:NativeLoaded = $true
    $script:EvidenceRoot = $script:Run.compiledTarget.evidenceDirectory
    if (-not [IO.Directory]::Exists($script:EvidenceRoot)) { throw 'BLOCKED_EXTERNAL_TARGET: preprovisioned private evidence directory missing' }
    $self = [RoundtripNative]::ProcessFacts([uint32]$PID)
    if ($self.userSid -cne $script:Run.compiledTarget.targetSid -or $self.elevated -ne $false -or $self.inAnyJob -ne $false -or $env:USERPROFILE -cne $script:Run.compiledTarget.profileDirectory) { throw 'BLOCKED_EXTERNAL_TARGET: driver target/token/job/profile mismatch' }
    if (Get-RoundtripSingleProcess 'cc-desk-version-manager.exe') { throw 'DUPLICATE_PROCESS: manager already exists before the new scenario' }
    $source = Get-RoundtripSingleProcess 'cc-desk.exe'
    if (-not $source -or $source.Id -ne [int]$script:Run.sourcePid) { throw 'TARGET_MISMATCH: observed source differs from reviewed interactive launch' }
    $sourcePath = Join-Path $script:Run.compiledTarget.installDirectory 'cc-desk.exe'
    $sourceFacts = Assert-RoundtripProcess $source $sourcePath $script:Run.imageSha256 '0.18.0' -JobFree
    Write-RoundtripRecord 'source-process' $sourceFacts

    # Actual UI scenario. No report or fixture is used to call coordinator methods.
    Wait-RoundtripControl $source $script:Run.selectors.sourceNativeMetadata $TimeoutSeconds | Out-Null
    Invoke-RoundtripControl $source $script:Run.selectors.settings $TimeoutSeconds
    Wait-RoundtripControl $source $script:Run.selectors.sourceOriginalPreference $TimeoutSeconds | Out-Null
    Invoke-RoundtripControl $source $script:Run.selectors.updates $TimeoutSeconds
    Invoke-RoundtripControl $source $script:Run.selectors.selectVersion $TimeoutSeconds
    Invoke-RoundtripControl $source $script:Run.selectors.prepare $TimeoutSeconds
    Invoke-RoundtripControl $source $script:Run.selectors.review $TimeoutSeconds
    Save-RoundtripScreenshot $source (Join-Path $script:EvidenceRoot 'driver-source-review.png')
    Invoke-RoundtripControl $source $script:Run.selectors.begin $TimeoutSeconds
    $manager = Wait-RoundtripCondition -TimeoutSeconds $TimeoutSeconds -Description 'retained manager native window/process' -Check { Get-RoundtripSingleProcess 'cc-desk-version-manager.exe' }
    Wait-RoundtripProcessExit -Process $source -TimeoutSeconds $TimeoutSeconds
    $m0 = Wait-RoundtripReport -Stage M0
    $sealed = Wait-RoundtripReport -Stage SourceSealed
    $fresh = Wait-RoundtripReport -Stage FreshReady
    $managerCim = Get-RoundtripSingleProcess 'cc-desk-version-manager.exe' $script:TransactionId
    if ($managerCim.Id -ne $manager.Id) { throw 'TARGET_MISMATCH: retained manager was replaced' }
    $managerFacts = [RoundtripNative]::ProcessFacts([uint32]$manager.Id)
    if ($managerFacts.userSid -cne $script:Run.compiledTarget.targetSid -or $managerFacts.elevated -ne $false -or (Get-RoundtripFileHash $managerFacts.imagePath) -cne $script:Run.imageSha256) { throw 'TARGET_MISMATCH: retained manager token/image differs' }
    Write-RoundtripRecord 'manager-process' $managerFacts
    $target = $null; $later = $null
    if ($Scenario -eq 'success') {
        $target = Wait-RoundtripReport -Stage TargetVerified
        $launched = Wait-RoundtripReport -Stage HistoricalLaunched
        $historical = Wait-RoundtripCondition -TimeoutSeconds $TimeoutSeconds -Description 'actual historical application' -Check { Get-RoundtripSingleProcess 'cc-desk.exe' }
        $image = @($target.value.bundle.tree.entries | Where-Object { $_.metadata.path -ceq 'cc-desk.exe' })
        if ($image.Count -ne 1) { throw 'INVALID_EVIDENCE: verified target image missing' }
        $historicalFacts = Assert-RoundtripProcess $historical $sourcePath $image[0].sha256 '0.17.7'
        Assert-RoundtripCreation $launched.value.creation $historicalFacts 'HistoricalApplication'
        if ($launched.value.resumeApplied -ne $true) { throw 'INVALID_EVIDENCE: actual historical resume receipt missing' }
        Write-RoundtripRecord 'historical-process' $historicalFacts
        Wait-RoundtripControl $historical $script:Run.selectors.historicalFresh $TimeoutSeconds | Out-Null
        Save-RoundtripScreenshot $historical (Join-Path $script:EvidenceRoot 'driver-historical-fresh-ui.png')
        Invoke-RoundtripControl $historical $script:Run.selectors.historicalPreferenceOpen $TimeoutSeconds
        Set-RoundtripPreference $historical $script:Run.selectors.historicalPreference $TimeoutSeconds
        Wait-RoundtripControl $historical $script:Run.selectors.historicalPreferenceChanged $TimeoutSeconds | Out-Null
        Save-RoundtripScreenshot $historical (Join-Path $script:EvidenceRoot 'driver-historical-changed-ui.png')
        Invoke-RoundtripControl $manager $script:Run.selectors.managerConfirm $TimeoutSeconds
        Invoke-RoundtripControl $manager $script:Run.selectors.managerConfirmSubmit $TimeoutSeconds
        Wait-RoundtripControl $manager $script:Run.selectors.managerConfirmed $TimeoutSeconds | Out-Null
        Close-RoundtripWindow -Process $historical
        Wait-RoundtripProcessExit -Process $historical -TimeoutSeconds $TimeoutSeconds
        Wait-RoundtripCondition -TimeoutSeconds $TimeoutSeconds -Description 'actual historical owned job zero' -Check { [RoundtripNative]::JobActiveProcesses($launched.value.creation.job.name) -eq 0 } | Out-Null
        Write-RoundtripRecord 'historical-job-zero' @{ name = $launched.value.creation.job.name; activeProcesses = [RoundtripNative]::JobActiveProcesses($launched.value.creation.job.name) }
        Set-RoundtripSharedSentinels -M0 $m0
    } else {
        $suspended = Wait-RoundtripReport -Stage InstallerSuspended
        $failure = Wait-RoundtripReport -Stage InjectedPreResumeFailure
        if ($failure.value.resumeAttempted -ne $false) { throw 'ASSERTION_FAILED: injected failure occurred after a resume attempt' }
        Assert-RoundtripProperty $suspended.value @('creation') 'actual suspended installer creation receipt'
        $installerFacts = [RoundtripNative]::ProcessFacts([uint32]$suspended.value.creation.process.pid)
        Assert-RoundtripCreation $suspended.value.creation $installerFacts 'Installer'
        if ($suspended.value.resumeAttempted -ne $false -or [RoundtripNative]::JobActiveProcesses($suspended.value.creation.job.name) -ne 1) { throw 'ASSERTION_FAILED: suspended installer custody differs' }
        Write-RoundtripRecord 'suspended-installer-process' $installerFacts
        Wait-RoundtripControl $manager $script:Run.selectors.managerRecoveryRequired $TimeoutSeconds | Out-Null
        Save-RoundtripScreenshot $manager (Join-Path $script:EvidenceRoot 'driver-injected-pre-resume-ui.png')
        $manager.Refresh(); if ($manager.HasExited) { throw 'ASSERTION_FAILED: manager custody lost before explicit Return' }
        Set-RoundtripSharedSentinels -M0 $m0
    }
    Invoke-RoundtripControl $manager $script:Run.selectors.managerReturn $TimeoutSeconds
    Invoke-RoundtripControl $manager $script:Run.selectors.managerRestore $TimeoutSeconds
    if ($Scenario -eq 'before-installer-resume') {
        $cancelled = Wait-RoundtripReport -Stage CancelledBeforeResume
        if ($cancelled.value.emptyOwnedJob -ne $true -or $cancelled.value.resumeAttempted -ne $false) { throw 'ASSERTION_FAILED: actual cancelled-before-resume terminal/empty job missing' }
        Assert-RoundtripProperty $cancelled.value @('terminal') 'native installer cancellation terminal'
        Assert-RoundtripProperty $cancelled.value.terminal @('schema', 'process', 'job', 'jobPhase', 'activeProcesses', 'cancellationIntent') 'durable actual cancellation terminal'
        if ($cancelled.value.terminal.schema -ne 3 -or $cancelled.value.terminal.jobPhase -cne 'armedPreparation' -or $cancelled.value.terminal.activeProcesses -ne 0 -or $cancelled.value.terminal.cancellationIntent -cnotmatch '^[0-9a-f]{64}$') { throw 'ASSERTION_FAILED: cancelled-before-resume durable terminal differs' }
        Assert-RoundtripEqual $suspended.value.creation.process $cancelled.value.terminal.process 'same real installer creation/terminal process'
        Assert-RoundtripEqual $suspended.value.creation.job $cancelled.value.terminal.job 'same real installer owned job'
    } else { $later = Wait-RoundtripReport -Stage LaterCaptured }
    $final = Wait-RoundtripReport -Stage FinalRestored
    Wait-RoundtripControl $manager $script:Run.selectors.restoredStatus $TimeoutSeconds | Out-Null
    Save-RoundtripScreenshot $manager (Join-Path $script:EvidenceRoot 'driver-manager-restored-ui.png')
    Write-RoundtripRecord 'manager-final-read' @{ finalReportSha256 = Get-RoundtripFileHash (Join-Path $script:EvidenceRoot "$($script:Run.runId)-FinalRestored.json"); managerFacts = [RoundtripNative]::ProcessFacts([uint32]$manager.Id) }
    Close-RoundtripWindow -Process $manager
    Wait-RoundtripProcessExit -Process $manager -TimeoutSeconds $TimeoutSeconds
    Write-RoundtripRecord 'manager-exited' @{ pid = $manager.Id; originalCreatedFileTime = $managerFacts.createdFileTime; exclusiveSourceImageFenceReleasedByExit = $true }
    if (Get-RoundtripSingleProcess 'cc-desk.exe') { throw 'DUPLICATE_PROCESS: application reopened before independent recapture' }
    $capture = Get-RoundtripIndependentCapture -M0 $m0 -Final $final
    Assert-RoundtripRestoredCapture -M0 $m0 -Final $final -Capture $capture
    Assert-RoundtripRetained -M0 $m0 -Final $final -Later $later -Target $target
    Write-RoundtripRecord 'independent-pre-reopen-capture' $capture
    $restored = Start-RoundtripRestoredApp -M0 $m0
    Write-RoundtripRecord 'result' @{ status = 'PASS'; scenario = $Scenario; scenarioLabel = $(if ($Scenario -eq 'success') { 'uninterrupted measured roundtrip' } else { 'injected pre-resume recovery' }); restoredAppReopened = $true; restoredPid = $restored.Id; managerExitedBeforeRecapture = $true; independentRecaptureBeforeReopen = $true; productionAdmission = $false }
    @{ status = 'PASS'; scenario = $Scenario; restoredAppReopened = $true; runId = $script:Run.runId } | ConvertTo-Json -Compress
    exit 0
} catch {
    $message = $_.Exception.Message
    $status = if ($message -match '^(BLOCKED_|TARGET_MISMATCH|SCENARIO_MISMATCH|IMAGE_HASH_MISMATCH|DUPLICATE_PROCESS|MISSING_REPORT|WAIT_TIMEOUT)') { 'BLOCKED' } else { 'FAILED_ACCEPTANCE' }
    $result = @{ status = $status; scenario = $Scenario; restoredAppReopened = $false; detail = $message; preserveLiveTransaction = $true }
    if ($script:EvidenceRoot) {
        try { Write-RoundtripRecord 'blocked-result' $result } catch { $result.evidenceWriteFailure = $_.Exception.Message }
    }
    $result | ConvertTo-Json -Compress
    exit 2
}
