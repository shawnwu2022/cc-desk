#Requires -Version 7.0
param([Parameter(Mandatory)][ValidateSet('CompileAndGate', 'All', 'Channel', 'Launch')][string]$Action)
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
if ($contained) { throw 'Ordinary PowerShell is contained in an external Job; direct native tests are blocked' }

$manifest = (Resolve-Path 'Cargo.toml').Path
$inventory = Join-Path (Split-Path $manifest) 'target/ci-test-artifacts.jsonl'
if ($Action -eq 'CompileAndGate') {
    [IO.Directory]::CreateDirectory((Split-Path $inventory)) | Out-Null
    & cargo test --locked --no-run --message-format=json > $inventory
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
$targets = @(Read-TestTargets (Get-Content -LiteralPath $inventory) $manifest)
$library = @($targets | Where-Object { $_.target.kind -contains 'lib' })[0]
if ($Action -eq 'CompileAndGate') {
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
