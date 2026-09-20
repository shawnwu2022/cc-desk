# Disposable Windows CI only: runs the actual packaged executable, never Claude.
param([Parameter(Mandatory=$true)][string]$Installer)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Join-Path $env:RUNNER_TEMP ('cc-conpty-install-' + [guid]::NewGuid())
$installed = Join-Path $root 'CC Desk 安装'
$cwd = Join-Path $root 'untrusted cwd'
New-Item -ItemType Directory -Path $cwd -Force | Out-Null
# Must never load either of these files from the working directory.
[IO.File]::WriteAllText((Join-Path $cwd 'conpty.dll'), 'not executable')
[IO.File]::WriteAllText((Join-Path $cwd 'OpenConsole.exe'), 'not executable')
$results = [Collections.Generic.List[object]]::new()
function Run-Probe([string]$directory, [bool]$expected, [string]$label) {
    $report = Join-Path $root ($label + '.json')
    $exe = Join-Path $directory 'cc-desk.exe'
    $process = Start-Process -FilePath $exe -ArgumentList "--check-conpty `"$report`"" -WorkingDirectory $cwd -PassThru
    if (!$process.WaitForExit(30000)) { $process.Kill(); throw "Probe timed out: $label" }
    $process.Refresh()
    if (!(Test-Path -LiteralPath $report)) { throw "No probe report: $label, exit=$($process.ExitCode)" }
    $data = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    if ($data.ok -ne $expected -or ($expected -and $process.ExitCode -ne 0) -or (!$expected -and $process.ExitCode -eq 0)) {
        throw "Wrong fail-closed result: $label $($data | ConvertTo-Json -Compress)"
    }
    if ($expected) {
        if ($data.backend -ne 'bundled' -or !$data.ptyLifecycle -or $data.version -ne '1.24.2607.10001') { throw "Wrong backend: $label" }
        # Canonical paths returned by Windows may carry the extended-length prefix.
        if (($data.dll -replace '^\\\\\?\\','') -ine (Join-Path $directory 'conpty.dll')) { throw "Unexpected loaded DLL: $($data.dll)" }
    }
    $results.Add([ordered]@{case=$label; expectedSuccess=$expected; exitCode=$process.ExitCode; result=$data})
    Write-Output "PASS $label"
}
try {
    for ($iteration=1; $iteration -le 2; $iteration++) {
        $setup = Start-Process -FilePath (Resolve-Path -LiteralPath $Installer).Path -ArgumentList "/S /D=$installed" -PassThru
        if (!$setup.WaitForExit(180000)) { $setup.Kill(); throw 'Installer timed out' }
        $setup.Refresh()
        if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)" }
        & node scripts/prepare-conpty.mjs --verify $installed
        if ($LASTEXITCODE -ne 0) { throw 'Installed runtime differs from pinned files' }
        Run-Probe $installed $true "install-$iteration"
    }
    $relocated = Join-Path $root '移动后的 CC Desk'
    Copy-Item -LiteralPath $installed -Destination $relocated -Recurse
    Run-Probe $relocated $true 'relocated'
    $host = Join-Path $relocated 'OpenConsole.exe'
    Move-Item -LiteralPath $host -Destination ($host + '.saved')
    Run-Probe $relocated $false 'missing-host'
    Move-Item -LiteralPath ($host + '.saved') -Destination $host
    $dll = Join-Path $relocated 'conpty.dll'
    $bytes = [IO.File]::ReadAllBytes($dll); $bytes[$bytes.Length-1] = $bytes[$bytes.Length-1] -bxor 1
    [IO.File]::WriteAllBytes($dll, $bytes)
    Run-Probe $relocated $false 'corrupt-dll'
    Remove-Item -LiteralPath $dll
    Run-Probe $relocated $false 'missing-dll-cwd-decoy'
    $results | ConvertTo-Json -Depth 8 | Set-Content conpty-install-results.json -Encoding utf8
} finally {
    # The runner is disposable; retain no user's application/configuration.
    $uninstaller = Join-Path $installed 'uninstall.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        $uninstall = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru
        if (!$uninstall.WaitForExit(60000)) { $uninstall.Kill() }
    }
}
