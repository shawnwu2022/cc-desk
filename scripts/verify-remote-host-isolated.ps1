param([string]$OutputDirectory = '.audit/remote-host-isolated-verification', [int]$Repeat005 = 5, [string]$TargetDirectory = $env:CARGO_TARGET_DIR, [string]$RelayRoot = $env:CC_DESK_TEST_RELAY_ROOT, [string]$RelayExe = $env:CC_DESK_TEST_RELAY_EXE)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root
$env:TEMP=Join-Path $root '.audit/runtime-temp'
$env:TMP=$env:TEMP
$env:TMPDIR=$env:TEMP
$env:CARGO_INCREMENTAL='0'
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
$env:CARGO_TARGET_DIR=if ($TargetDirectory) { (Resolve-Path $TargetDirectory).Path } else { Join-Path $root 'target-browser-profile' }
if (-not $RelayRoot -or -not $RelayExe) { throw 'Explicit RelayRoot and RelayExe (or matching environment) are required' }
$env:CC_DESK_TEST_RELAY_ROOT=(Resolve-Path $RelayRoot).Path
$env:CC_DESK_TEST_RELAY_EXE=$RelayExe

$out=[IO.Path]::GetFullPath((Join-Path $root $OutputDirectory))
if (-not $out.StartsWith($root+[IO.Path]::DirectorySeparatorChar)) { throw 'Evidence must remain in project' }
if (Test-Path $out) { throw 'Evidence directory already exists; do not overwrite' }
New-Item -ItemType Directory -Path $out | Out-Null
# 必须使用本次 Cargo 返回的 lib test artifact，不猜测历史文件名。
& node scripts/prepare-conpty.mjs *> (Join-Path $out 'prepare-conpty.log')
if ($LASTEXITCODE -ne 0) { throw 'ConPTY preparation failed' }
& cargo +1.98.1 test --manifest-path src-tauri/Cargo.toml --features remote-host --lib --no-run --offline --locked --message-format=json 1> (Join-Path $out 'build.jsonl') 2> (Join-Path $out 'build.stderr.log')
$buildExit=$LASTEXITCODE
$buildExit | Set-Content (Join-Path $out 'build.exit')
if ($buildExit -ne 0) { throw 'Build failed; no stale artifact fallback' }
$artifacts=@(Get-Content (Join-Path $out 'build.jsonl') | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.executable -and $_.target.name -eq 'cc_desk' -and $_.target.kind -contains 'lib' })
if ($artifacts.Count -ne 1) { throw 'Expected exactly one cc_desk lib test artifact' }
$exe=$artifacts[0].executable
$artifacts[0] | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $out 'selected-artifact.json')
$inventory=@(& $exe --list)
if ($LASTEXITCODE -ne 0) { throw 'Test inventory failed' }
$inventory | Set-Content (Join-Path $out 'inventory.txt')
$names=@($inventory | Where-Object { $_ -match '::NativeRemote_.*: test$' } | ForEach-Object { $_ -replace ': test$','' })
$ids=@($names | ForEach-Object { ($_ -split '_')[-1] } | Sort-Object)
if (($ids -join ',') -ne '001,002,003,004,005,006,007,008,009,010,011,012,013,014,015,016,017,018') { throw 'Expected exact NativeRemote 001..018 inventory' }
$checkout=@($inventory | Where-Object { $_ -match '^remote_host::tests::host_checkout_.*: test$' })
$expectedCheckout=@(
'remote_host::tests::host_checkout_busy_is_pending_and_closing_survives_return: test',
'remote_host::tests::host_checkout_closing_before_return_stops_original_host: test',
'remote_host::tests::host_checkout_unwind_latches_unknown_without_losing_reservation: test')
if (($checkout | Sort-Object) -join ',' -cne (($expectedCheckout | Sort-Object) -join ',')) { throw 'Checkout inventory differs from exact three tests' }
$network=@($inventory | Where-Object { $_ -match '^remote_host_network::.*: test$' })
if ($network.Count -eq 0) { throw 'Network inventory must be nonempty' }
. (Join-Path $PSScriptRoot 'remote-host-owned-timeout.ps1')
$results=[Collections.Generic.List[object]]::new()
function Run-Owned([string]$label, [string[]]$testArgs) {
  $started=[DateTime]::UtcNow
  $p=Start-Process -FilePath $exe -ArgumentList $testArgs -WorkingDirectory $root -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out "$label.stdout.log") -RedirectStandardError (Join-Path $out "$label.stderr.log")
  $timeout=-not $p.WaitForExit(60000)
  $cleanupUnknown=$false; if ($timeout) { $cleanupUnknown=-not (Stop-OwnedTimedOut $p) }
  $p.Refresh()
  $exitCode=Get-OwnedExitCode $p
  $expectedCount=if($label -eq 'network-unit'){$network.Count}elseif($label -eq 'checkout'){3}else{1}
  $summary=Get-Content (Join-Path $out "$label.stdout.log") -Raw
  $summaryValid=$summary -match ("test result: ok\. "+$expectedCount+" passed; 0 failed; 0 ignored;")
  $row=[pscustomobject]@{label=$label;pid=$p.Id;startedUtc=$started;exitCode=$exitCode;timedOut=$timeout;cleanupUnknown=$cleanupUnknown;fixtureChildrenVerified=$false;summaryValid=$summaryValid;expectedCount=$expectedCount;arguments=$testArgs}
  $results.Add($row)
  $results | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $out 'results.json')
  Write-Output "$label exit=$exitCode timeout=$timeout"
  $p.Dispose(); if ($timeout) { throw "Owned test timed out; cleanupUnknown=$cleanupUnknown; fixture children unverified. Stop all subsequent experiments." }
}
$five=$names | Where-Object { $_ -match '_005$' }
for($i=1;$i -le $Repeat005;$i++) { Run-Owned "repeat005-$i" @('--exact',$five,'--nocapture','--test-threads=1') }
Run-Owned 'network-unit' @('remote_host_network::','--nocapture','--test-threads=1')
Run-Owned 'checkout' @('remote_host::tests::host_checkout_','--nocapture','--test-threads=1')
foreach($name in $names) { Run-Owned (($name -split '::')[-1]) @('--exact',$name,'--nocapture','--test-threads=1') }
$files=@($exe,'src-tauri/src/remote_host_network.rs','src-tauri/src/remote_host.rs','src-tauri/src/tests/native_cli_remote_composition.rs',$PSCommandPath,(Join-Path $PSScriptRoot 'remote-host-owned-timeout.ps1'))
$files | ForEach-Object { Get-FileHash -Algorithm SHA256 $_ } | Select-Object Path,Hash | ConvertTo-Json | Set-Content (Join-Path $out 'sha256.json')
git rev-parse HEAD | Set-Content (Join-Path $out 'head.txt')
[pscustomobject]@{TEMP=$env:TEMP;TMP=$env:TMP;TMPDIR=$env:TMPDIR;target=$env:CARGO_TARGET_DIR;incremental=$env:CARGO_INCREMENTAL;relayRoot=$env:CC_DESK_TEST_RELAY_ROOT;relayExe=$env:CC_DESK_TEST_RELAY_EXE} | ConvertTo-Json | Set-Content (Join-Path $out 'environment.json')
if (@($results | Where-Object { $_.timedOut -or $_.exitCode -ne 0 -or -not $_.summaryValid }).Count) { exit 1 }
