#Requires -Version 7.0
param([Parameter(Mandatory)][ValidateSet('Compile', 'HostedSuite', 'CompileAndGate', 'All', 'Channel', 'Launch')][string]$Action)
$ErrorActionPreference = 'Stop'

function Read-TestTargets([string[]]$Lines, [string]$Manifest) {
    $records = @($Lines | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
    $finished = @($records | Where-Object reason -eq 'build-finished')
    if ($finished.Count -ne 1 -or $finished[0].success -ne $true) { throw 'Missing successful build-finished record' }
    $targets = @($records | Where-Object {
        $_.reason -eq 'compiler-artifact' -and $_.profile.test -eq $true -and $_.executable -and
        [IO.Path]::GetFullPath($_.manifest_path) -eq [IO.Path]::GetFullPath($Manifest)
    } | Sort-Object executable -Unique)
    if (@($targets | Where-Object { $_.target.kind -contains 'lib' }).Count -ne 1) { throw 'Expected one library test harness' }
    return $targets
}

function Invoke-Libtest([string]$Executable, [string[]]$TestArgs, [bool]$Library, [int]$Expected = -1) {
    $listing = @(& $Executable @TestArgs --list --format terse 2>&1)
    if ($LASTEXITCODE -ne 0) { $listing | Out-Host; throw 'libtest inventory failed' }
    $count = @($listing | Where-Object { "$_" -match '^.+: (test|benchmark)$' }).Count
    if ($Expected -ge 0 -and $count -ne $Expected) { throw "Expected $Expected tests, inventoried $count" }
    if ($Library -and $count -eq 0) { throw 'The library inventory is empty' }
    Write-Host "LIBTEST executable=$Executable inventoried=$count"
    # No --ignored: original ignored workers remain owned by their supervising tests.
    & $Executable @TestArgs 2>&1 | Tee-Object -Variable output | Out-Host
    $code = $LASTEXITCODE
    $summaries = @($output | ForEach-Object {
        if ("$_" -match '^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;') {
            [pscustomobject]@{ passed = [int]$Matches[1]; failed = [int]$Matches[2]; ignored = [int]$Matches[3]; measured = [int]$Matches[4] }
        }
    })
    if ($summaries.Count -eq 0) { throw 'Missing libtest summary' }
    $summary = $summaries[-1] # Workers can print nested summaries; only the final outer summary counts.
    if ($summary.passed + $summary.failed + $summary.ignored + $summary.measured -ne $count) { throw 'libtest inventory/summary count mismatch' }
    Write-Host "LIBTEST_RESULT exit=$code passed=$($summary.passed) failed=$($summary.failed) ignored=$($summary.ignored) measured=$($summary.measured)"
    return ($code -eq 0 -and $summary.failed -eq 0)
}

function Test-ExactSet([string[]]$A, [string[]]$B) {
    return ($A.Count -eq $B.Count -and @($A | Where-Object { $B -cnotcontains $_ }).Count -eq 0)
}

function Read-HostedInventory([string]$Executable, [string[]]$TestArgs, [string]$LogPath) {
    $lines = @(& $Executable @TestArgs --list --format pretty 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
    $lines | Set-Content -LiteralPath $LogPath -Encoding utf8
    if ($code -ne 0) { throw 'libtest inventory failed' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $entries = @($lines | ForEach-Object {
        if ($_ -cmatch '^(.+): (test|benchmark)$') {
            $name = $Matches[1]; $type = $Matches[2]
            if (!$seen.Add($name)) { throw 'duplicate libtest inventory name' }
            [pscustomobject]@{ name = $name; type = $type }
        }
    })
    $footers = @($lines | Where-Object { $_ -cmatch '^\d+ tests?, \d+ benchmarks?$' })
    if ($footers.Count -ne 1) { throw 'missing libtest inventory footer' }
    $null = $footers[0] -cmatch '^(\d+) tests?, (\d+) benchmarks?$'
    if ([int]$Matches[1] -ne @($entries | Where-Object type -eq 'test').Count -or [int]$Matches[2] -ne @($entries | Where-Object type -eq 'benchmark').Count) { throw 'libtest inventory/footer count mismatch' }
    return [pscustomobject]@{ entries = $entries; names = @($entries | ForEach-Object name) }
}

function Get-HostedTestSelection($Full, [string[]]$Ignored, [bool]$Library, [bool]$Contained, $Scope) {
    $names = @($Full | ForEach-Object name)
    $policy = @($Scope.jobFreeTests)
    if ($Library) {
        foreach ($name in $policy) {
            if (@($names | Where-Object { $_ -ceq $name }).Count -ne 1 -or $Ignored -ccontains $name) { throw 'Job-free policy drift: entry missing, duplicate or ignored' }
        }
    } elseif (@($names | Where-Object { $policy -ccontains $_ }).Count -gt 0) { throw 'Job-free policy entry outside library' }
    $excluded = if ($Library -and $Contained) { $policy } else { @() }
    foreach ($name in $excluded) {
        if (@($names | Where-Object { $_ -cne $name -and $_.Contains($name, [StringComparison]::Ordinal) }).Count -gt 0) { throw 'libtest full-name skip substring collision' }
    }
    $selected = @($names | Where-Object { $excluded -cnotcontains $_ })
    if ($Library) {
        if (@($Full | Where-Object { $_.type -eq 'test' -and $selected -ccontains $_.name -and $Ignored -cnotcontains $_.name }).Count -eq 0) { throw 'library selected nonignored inventory is empty' }
        foreach ($name in @($Scope.requiredSelectedTests)) {
            if ($selected -cnotcontains $name -or $Ignored -ccontains $name) { throw 'required ordinary/Wry test not selected' }
        }
    }
    return [pscustomobject]@{ selected = $selected; excluded = @($excluded) }
}

function Assert-HostedSelection([string[]]$Full, [string[]]$Ignored, [string[]]$Selected, [string[]]$Excluded) {
    if (@($Ignored | Where-Object { $Full -cnotcontains $_ -or $Selected -cnotcontains $_ }).Count -gt 0) { throw 'original ignored inventory changed' }
    if (@($Excluded | Where-Object { $Full -cnotcontains $_ -or $Ignored -ccontains $_ }).Count -gt 0) { throw 'excluded test missing or ignored' }
    if (!(Test-ExactSet $Selected @($Full | Where-Object { $Excluded -cnotcontains $_ }))) { throw 'selection differs from full minus exact exclusions' }
}

function Read-HostedResult([string[]]$Lines, [int]$Code) {
    $summaries = @($Lines | ForEach-Object {
        if ($_ -cmatch '^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;') {
            [pscustomobject]@{ exitCode = $Code; passed = [int]$Matches[1]; failed = [int]$Matches[2]; ignored = [int]$Matches[3]; measured = [int]$Matches[4]; filteredOut = [int]$Matches[5] }
        }
    })
    if ($summaries.Count -eq 0) { throw 'missing outer libtest summary' }
    return $summaries[-1]
}

function Invoke-HostedHarness($Plan, [string]$Directory) {
    # Plan was checked against the unfiltered inventory before any body executes.
    $testArgs = @($Plan.excluded | ForEach-Object { '--skip'; $_ })
    $output = @(& $Plan.executable @testArgs 2>&1 | Tee-Object -FilePath (Join-Path $Directory $Plan.logs.execution) | ForEach-Object { Write-Host "$_"; $_ })
    $code = $LASTEXITCODE
    $lines = @($output | ForEach-Object { "$_" })
    $Plan.result = Read-HostedResult $lines $code
    $Plan.defaultIgnored = @($Plan.ignored | ForEach-Object {
        $name = $_; $reason = $null; $found = $false
        foreach ($line in $lines) {
            if ($line -cmatch '^test (.+) \.\.\. ignored(?:, (.*))?$' -and $Matches[1] -ceq $name) {
                $reason = if ($Matches.ContainsKey(2)) { $Matches[2] } else { $null }; $found = $true
            }
        }
        if (!$found) { throw 'missing original ignored execution record' }
        [pscustomobject]@{ name = $name; reason = $reason; classification = 'original-default-ignore' }
    })
    $r = $Plan.result
    if ($r.passed + $r.failed + $r.ignored + $r.measured -ne $Plan.selected.Count -or $r.ignored -ne $Plan.ignored.Count -or $r.filteredOut -ne $Plan.excluded.Count) { throw 'outer result/inventory count mismatch' }
    return ($code -eq 0 -and $r.failed -eq 0)
}

function Invoke-HostedSuite($Targets, [bool]$Contained, [string]$Manifest, [string]$ScriptsDirectory) {
    $validator = Join-Path $ScriptsDirectory 'windows-native-validation.mjs'
    $scope = Get-Content -LiteralPath (Join-Path $ScriptsDirectory 'windows-native-scope.json') -Raw -Encoding utf8 | ConvertFrom-Json
    if ($scope.schema -ne 1 -or $scope.jobFreeTests.Count -ne 18 -or $scope.harnesses.Count -ne 4) { throw 'invalid checked-in hosted scope' }
    $sourceSha = [string]$env:GITHUB_SHA; $runId = [string]$env:GITHUB_RUN_ID; $attempt = 0
    if (![int]::TryParse([string]$env:GITHUB_RUN_ATTEMPT, [ref]$attempt)) { throw 'missing workflow attempt binding' }
    $coverageName = @(& node $validator artifact-name $sourceSha $runId $attempt)
    if ($LASTEXITCODE -ne 0 -or $coverageName.Count -ne 1) { throw 'invalid source/run/attempt binding' }
    $actualSha = @(& git rev-parse HEAD)
    if ($LASTEXITCODE -ne 0 -or $actualSha.Count -ne 1 -or $actualSha[0] -cne $sourceSha) { throw 'checkout source binding mismatch' }
    $directory = Join-Path (Split-Path $Manifest) 'target/windows-native-coverage'
    [IO.Directory]::CreateDirectory((Join-Path $directory 'logs')) | Out-Null
    $reportFile = Join-Path $directory 'windows-native-coverage.json'
    if ($env:GITHUB_OUTPUT) {
        Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value @("coverage_name=$($coverageName[0])", 'coverage_path=src-tauri/target/windows-native-coverage', 'coverage_report=src-tauri/target/windows-native-coverage/windows-native-coverage.json')
    }
    $report = [ordered]@{
        schema = 1; policy = 'required-checks-and-disclosed-host-unverified-v1'; completed = $false
        sourceSha = $sourceSha; runId = $runId; runAttempt = $attempt
        host = @{ jobQuerySucceeded = $true; inJob = $Contained }; harnesses = @(); doctests = $null
        nativeJobSuite = @{ status = 'unverified'; reason = 'not_executed'; unverifiedNames = @($scope.jobFreeTests) }
        nativeAll = @{ status = 'unverified'; reason = 'original_all_not_run' }; nativeAcceptanceProven = $false
    }
    $report | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $reportFile -Encoding utf8
    $ok = $true
    try {
        $identities = @($Targets | ForEach-Object {
            $kind = if ($_.target.kind -contains 'lib') { 'lib' } elseif ($_.target.kind -contains 'bin') { 'bin' } elseif ($_.target.kind -contains 'test') { 'test' } else { throw 'unsupported default harness kind' }
            "$($kind):$($_.target.name)"
        })
        if (!(Test-ExactSet $identities @($scope.harnesses | ForEach-Object { "$($_.kind):$($_.name)" }))) { throw 'default four-harness identity mismatch' }
        # Inventory every target first. No selected test body starts after a rejected selection.
        foreach ($target in $Targets) {
            $kind = if ($target.target.kind -contains 'lib') { 'lib' } elseif ($target.target.kind -contains 'bin') { 'bin' } else { 'test' }
            $name = $target.target.name
            $logs = @{ full = "logs/$name-full.log"; ignored = "logs/$name-ignored.log"; selected = "logs/$name-selected.log"; execution = "logs/$name-execution.log" }
            $full = Read-HostedInventory $target.executable @() (Join-Path $directory $logs.full)
            $ignored = Read-HostedInventory $target.executable @('--ignored') (Join-Path $directory $logs.ignored)
            $selection = Get-HostedTestSelection $full.entries $ignored.names ($kind -eq 'lib') $Contained $scope
            $testArgs = @($selection.excluded | ForEach-Object { '--skip'; $_ })
            $selected = Read-HostedInventory $target.executable $testArgs (Join-Path $directory $logs.selected)
            Assert-HostedSelection $full.names $ignored.names $selected.names $selection.excluded
            $plan = [pscustomobject]@{ identity = @{ name = $name; kind = $kind }; executable = $target.executable; full = @($full.entries); ignored = @($ignored.names); selected = @($selected.names); excluded = @($selection.excluded); logs = $logs; result = $null; defaultIgnored = @() }
            $report.harnesses += $plan
        }
        foreach ($plan in $report.harnesses) {
            try { if (!(Invoke-HostedHarness $plan $directory)) { $ok = $false } }
            catch { Write-Host "HOSTED_HARNESS_REJECTED $($_.Exception.Message)"; $ok = $false }
        }
        # --no-run omits doctests. Keep the original full, unfiltered doctest command.
        $docOutput = @(& cargo test --locked --doc 2>&1 | Tee-Object -FilePath (Join-Path $directory 'logs/doctests.log') | ForEach-Object { Write-Host "$_"; $_ })
        $docCode = $LASTEXITCODE
        $report.doctests = @{ command = 'cargo test --locked --doc'; exitCode = $docCode; logs = @('logs/doctests.log'); result = (Read-HostedResult @($docOutput | ForEach-Object { "$_" }) $docCode) }
        if ($docCode -ne 0 -or $report.doctests.result.failed -ne 0) { $ok = $false }
        $report.nativeJobSuite = @{ status = $(if ($Contained) { 'unverified' } else { 'executed' }); reason = $(if ($Contained) { 'external_job' } else { $null }); unverifiedNames = $(if ($Contained) { @($scope.jobFreeTests) } else { @() }) }
        $report.completed = $true
    } finally {
        $report | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $reportFile -Encoding utf8
    }
    & node $validator validate $directory $sourceSha $runId $attempt | Out-Host
    if ($LASTEXITCODE -ne 0) { $ok = $false }
    return $ok
}

# Read-only probe of this ordinary pwsh, before invoking Cargo or any test process.
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WindowsRustCiJob {
    [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsProcessInJob(IntPtr process, IntPtr job, [MarshalAs(UnmanagedType.Bool)] out bool contained);
}
'@
$contained = $false
if (![WindowsRustCiJob]::IsProcessInJob([WindowsRustCiJob]::GetCurrentProcess(), [IntPtr]::Zero, [ref]$contained)) {
    throw "IsProcessInJob failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}
Write-Host "WINDOWS_RUST_CI_PARENT pid=$PID inJob=$contained"
if ($env:GITHUB_OUTPUT) { Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "job_free=$((!$contained).ToString().ToLowerInvariant())" }
if ($contained -and $Action -in @('CompileAndGate', 'All', 'Channel', 'Launch')) { throw 'Ordinary PowerShell is contained in an external Job; direct native tests are blocked' }

$manifest = (Resolve-Path 'Cargo.toml').Path
$inventory = Join-Path (Split-Path $manifest) 'target/ci-test-artifacts.jsonl'
if ($Action -in @('Compile', 'CompileAndGate')) {
    [IO.Directory]::CreateDirectory((Split-Path $inventory)) | Out-Null
    & cargo test --locked --no-run --message-format=json > $inventory
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
$targets = @(Read-TestTargets (Get-Content -LiteralPath $inventory) $manifest)
$library = @($targets | Where-Object { $_.target.kind -contains 'lib' })[0]
if ($Action -eq 'Compile') {
    if ($env:GITHUB_OUTPUT) { Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value 'compiled=true' }
    $ok = $true
} elseif ($Action -eq 'HostedSuite') {
    $ok = Invoke-HostedSuite $targets $contained $manifest $PSScriptRoot
} elseif ($Action -eq 'CompileAndGate') {
    # The following CI step may run the compiled inventory even if the exact gate fails.
    if ($env:GITHUB_OUTPUT) { Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value 'compiled=true' }
    $ok = Invoke-Libtest $library.executable @('version_history::windows::manager_process::tests::HistoryManagerProcess_SuspendedIdentity_001', '--exact', '--nocapture', '--test-threads=1') $true 1
} elseif ($Action -eq 'All') {
    $ok = $true
    foreach ($target in $targets) {
        # Includes library, binary and integration harnesses from the original default cargo test target set.
        # The tiny 001 gate runs again here so no test is excluded from the aggregate.
        try { if (!(Invoke-Libtest $target.executable @() ($target.target.kind -contains 'lib'))) { $ok = $false } }
        catch { Write-Host "LIBTEST_REJECTED $($_.Exception.Message)"; $ok = $false }
    }
    # --no-run does not compile doctests; preserve the original cargo test doctest coverage explicitly.
    & cargo test --locked --doc
    if ($LASTEXITCODE -ne 0) { $ok = $false }
} else {
    $name = if ($Action -eq 'Channel') { 'tests::native_cli_channel_live::D11_Channel_Native_011' } else { 'tests::native_cli_launch_live::worker::D11_Launch_Native_001' }
    $ok = Invoke-Libtest $library.executable @($name, '--exact', '--nocapture', '--test-threads=1') $true 1
}
if (!$ok) { exit 1 }
