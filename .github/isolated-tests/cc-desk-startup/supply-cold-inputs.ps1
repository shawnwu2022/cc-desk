$ErrorActionPreference='Stop'
if ((Get-Location).Path -ine (Join-Path $env:GITHUB_WORKSPACE 'product-source') -or $PSScriptRoot -ine $env:CCDESK_TEST_HELPERS_ROOT) { throw 'Unexpected isolated source or reviewed helper root' }
function Invoke-Quiet([string]$Tool,[string[]]$Options,[string]$SafeLabel) {
  & $Tool @Options *> (Join-Path $env:RUNNER_TEMP ($SafeLabel+'.local.log'))
  if ($LASTEXITCODE -ne 0) { throw "Pinned input step failed: $SafeLabel" }
  Write-Host "Pinned input step passed: $SafeLabel"
}
if ((git rev-parse HEAD) -cne '04a2f0cedc8e7acc080e8736723c6e8799127f58' -or
    @(git status --porcelain).Count -ne 0 -or (Test-Path -LiteralPath 'src-tauri/target') -or
    (Test-Path -LiteralPath 'dist') -or $env:CARGO_TARGET_DIR -or $env:RUSTC_WRAPPER -or
    $env:TAURI_DEBUG -or $env:CC_DESK_BUILD_SHA) { throw 'Expected clean exact source and cold native build inputs' }
if ((node --version) -cne 'v24.18.0' -or (npm --version) -cne '11.16.0' -or (rustc --version) -cnotmatch '^rustc 1\.98\.1 ') { throw 'Pinned tool versions differ' }
Invoke-Quiet npm @('ci') 'locked-npm'
Invoke-Quiet npm @('run','prepare:session-sdk') 'session-sdk'
Invoke-Quiet npm @('run','prepare:opencode-archive') 'archive-sdk'
Invoke-Quiet node @('scripts/prepare-conpty.mjs') 'conpty-runtime'
Invoke-Quiet rustup @('target','add','wasm32-unknown-unknown','--toolchain','1.98.1') 'wasm-target'
try { ./scripts/prepare-windows-browser-tools.ps1 *> (Join-Path $env:RUNNER_TEMP 'browser-tools.local.log') } catch { throw 'Pinned browser tool preparation failed' }
Invoke-Quiet cargo @('fetch','--locked','--manifest-path','remote/Cargo.toml') 'locked-browser-fetch'
