param([string]$ProgramPath)
$ErrorActionPreference = 'Stop'
if (-not $ProgramPath) { $ProgramPath = (Get-Command agy -ErrorAction Stop).Source }
$ProgramPath = (Resolve-Path -LiteralPath $ProgramPath).Path
$signature = Get-AuthenticodeSignature -LiteralPath $ProgramPath
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(^|, )O=Google LLC(,|$)') {
    throw 'A valid Google LLC signed Antigravity CLI is required'
}
$version = & $ProgramPath --version
if ($LASTEXITCODE -ne 0) { throw 'The installed CLI version probe failed' }
Write-Output "AGY_SMOKE_SIGNATURE=Valid; VERSION=$version; SHA256=$((Get-FileHash -LiteralPath $ProgramPath -Algorithm SHA256).Hash)"
$old = $env:CC_DESK_AGY_SMOKE_EXE
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Push-Location (Join-Path $root 'src-tauri')
try {
    $env:CC_DESK_AGY_SMOKE_EXE = $ProgramPath
    & cargo test --locked --lib --features antigravity-acceptance tests::native_cli_antigravity_acceptance::Antigravity_InstalledDiscoveryAndOwnedTui_001 -- --exact --nocapture
    $result = $LASTEXITCODE
} finally {
    $env:CC_DESK_AGY_SMOKE_EXE = $old
    Pop-Location
}
exit $result
