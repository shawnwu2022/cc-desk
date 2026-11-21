# Local review draft only: no Actions trigger or downloader.
param([Parameter(Mandatory)][string]$Installer, [Parameter(Mandatory)][string]$FixtureCodex, [Parameter(Mandatory)][string]$Python)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'test-fresh-runner-admission.ps1')
$receipt=[ordered]@{stage='admission'; preexistingWebView=$false; proxyEnvironmentAbsent=$false; directWinHttp=$false; directWinInet=$false; cleanTestProfile=$false; officialRun=44842769369; officialRunAttempt=1; finalArtifact=14682820278; installerVerified=$false; installerInvocations=0; installedImageVerified=$false; installerUnchanged=$false}
$heldInstaller=$null
$heldApp=$null
$heldCodex=$null
$setup=$null

function Assert-LocalCandidate([string]$InputFile, [string]$Leaf, [string]$Root) {
    $file=Get-Item -LiteralPath $InputFile
    if ($file.PSIsContainer -or $file.Name -cne $Leaf -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        !$file.FullName.StartsWith($Root,[StringComparison]::OrdinalIgnoreCase)) { throw 'LOCAL_INPUT_IDENTITY' }
    for ($dir=$file.Directory; $dir; $dir=$dir.Parent) {
        if ($dir.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'LOCAL_INPUT_ANCESTOR' }
    }
    return $file.FullName
}
function Read-VerifyJson([string]$Script, [string[]]$Arguments) {
    $body=& $Python (Join-Path $PSScriptRoot $Script) @Arguments 2>$null
    if ($LASTEXITCODE -ne 0 -or @($body).Count -ne 1 -or ([string]$body).Length -gt 1024) { throw 'PINNED_INPUT_BINDING' }
    return $body | ConvertFrom-Json
}

try {
    if (![IO.Path]::IsPathFullyQualified($Python) -or (Get-Item -LiteralPath $Python).Name -ine "python.exe") { throw "PYTHON_PATH_REQUIRED" }
    Assert-FreshRunner $receipt
    $temp=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
    $receipt.stage='installer-member'
    $installerSha='0fe089112ed137bada3130454c810326f0f5ad6a10c8ff4f9a52161c32ffeaa6'
    $appSha='cd55c6504093a7aa884cc8936bc51b58a133a512d8d496ea0530d64e9999b6f5'
    $appBytes=56135680
    $pin=Assert-LocalCandidate $Installer 'CC-Desk-0.28.0-rc.1-test-04a2f0cedc8e-x64-setup.exe' $temp
    $codex=Assert-LocalCandidate $FixtureCodex 'codex.exe' $temp
    $heldCodex=[IO.File]::Open($codex,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    if ($heldCodex.Length -ne 382647088 -or (Get-FileHash -InputStream $heldCodex).Hash -ine 'f76f94d08b0793bfa129ae62c09341ee32ae96d57e69968ae6b8d7c71a20d8c4') { throw 'CODEX_PIN' }
    $heldInstaller=[IO.File]::Open($pin,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    if ($heldInstaller.Length -ne 14424921 -or (Get-FileHash -InputStream $heldInstaller).Hash -ine $installerSha) { throw 'INSTALLER_PIN' }
    $named=Read-VerifyJson 'verify_named_nsis.py' @('--installer',$pin,'--sha256',$installerSha)
    if ($named.installerSha256 -cne $installerSha -or $named.member -cne 'cc-desk.exe' -or $named.memberSha256 -cne $appSha -or $named.memberBytes -ne $appBytes) { throw 'INSTALLER_MEMBER' }
    $receipt.installerVerified=$true
    $target=Join-Path $temp ('ccdesk-rename-install-'+[guid]::NewGuid().ToString('N'))
    if (Test-Path -LiteralPath $target) { throw 'EXISTING_INSTALL_TARGET' }
    $receipt.stage='installer-invocation'
    $invoked=(Resolve-Path -LiteralPath $pin).Path
    $binding=Read-VerifyJson 'next_gate_binding.py' @('installer',$invoked,$pin,$installerSha)
    if ($binding.installerSha256 -cne $installerSha -or $binding.installerBytes -ne $heldInstaller.Length) { throw 'INSTALLER_INVOKED_BYTES' }
    $receipt.installerInvocations++
    $setup=Start-Process -FilePath $invoked -ArgumentList "/S /D=$target" -PassThru
    if (!$setup.WaitForExit(180000) -or $setup.ExitCode -ne 0) { throw 'INSTALLER_FAILED' }
    $receipt.stage='installed-image'
    $selected=Join-Path $target 'cc-desk.exe'
    $heldApp=[IO.File]::Open($selected,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    $installed=Read-VerifyJson 'next_gate_binding.py' @('installed',$selected,$target,'cc-desk.exe',$appSha,[string]$appBytes)
    $heldInstaller.Position=0
    $receipt.installerUnchanged=(Get-FileHash -InputStream $heldInstaller).Hash -ieq $installerSha
    if (!$receipt.installerUnchanged -or $installed.installedSha256 -cne $appSha -or $installed.installedBytes -ne $appBytes) { throw 'INSTALLED_BYTES' }
    $receipt.installedImageVerified=$true
    $receipt.stage='isolated-ordinary-ui'
    & (Join-Path $PSScriptRoot 'test-installed-rename.ps1') -Exe $selected -FixtureCodex $codex
    $receipt.stage='passed'
} catch {
    throw 'Official test-only fresh-rename gate blocked; see safe stage.'
} finally {
    if ($setup) { if (!$setup.HasExited) { $setup.Kill(); $null=$setup.WaitForExit(10000) }; $setup.Dispose() }
    if ($heldApp) { $heldApp.Dispose() }
    if ($heldInstaller) { $heldInstaller.Dispose() }
    if ($heldCodex) { $heldCodex.Dispose() }
    $out=Join-Path (Get-Location) 'official-rename-evidence'
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    $receipt | ConvertTo-Json -Compress | Set-Content -Encoding utf8 (Join-Path $out 'invocation.json')
}
