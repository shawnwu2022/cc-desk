#Requires -Version 7.0
# Pure inventory contracts and disposable real libtest fixtures; no application/user data.
$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot '../../scripts/windows-rust-tests.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($helper, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0 -or !(Test-Path -LiteralPath $helper)) { throw 'Windows Rust CI helper missing or invalid' }
foreach ($function in $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] }, $true)) {
    . ([scriptblock]::Create($function.Extent.Text))
}
$passed = 0
function Pass([string]$name, [scriptblock]$check) { & $check; $script:passed++; Write-Host "PASS $name" }
function Reject([string]$name, [scriptblock]$check, [string]$expected) {
    $message = $null
    try { & $check } catch { $message = $_.Exception.Message }
    if (!$message -or $message -notmatch $expected) { throw "$name expected $expected, got $message" }
    $script:passed++; Write-Host "PASS $name"
}
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('ccdesk-ci-libtest-' + [guid]::NewGuid())
[IO.Directory]::CreateDirectory($fixture) | Out-Null
# Keep the disposable directory as evidence; never touch application/user data.
$manifest = Join-Path $fixture 'Cargo.toml'
[IO.File]::WriteAllText($manifest, '')
$library = @{ reason = 'compiler-artifact'; manifest_path = $manifest; target = @{ kind = @('lib'); name = 'fixture' }; profile = @{ test = $true }; executable = 'fixture.exe' }
$binary = @{ reason = 'compiler-artifact'; manifest_path = $manifest; target = @{ kind = @('bin'); name = 'fixture-bin' }; profile = @{ test = $true }; executable = 'bin.exe' }
$done = @{ reason = 'build-finished'; success = $true }
function JsonLines($items) { @($items | ForEach-Object { $_ | ConvertTo-Json -Depth 8 -Compress }) }
Pass 'all default test executables retained and duplicates removed' {
    $targets = @(Read-TestTargets (JsonLines @($library, $binary, $library, $done)) $manifest)
    if ($targets.Count -ne 2) { throw 'lost or duplicated test target' }
}
Reject 'incomplete compile cannot supply an inventory' { Read-TestTargets (JsonLines @($library)) $manifest } 'successful build-finished'
Reject 'failed compile cannot supply an inventory' { Read-TestTargets (JsonLines @($library, @{reason='build-finished';success=$false})) $manifest } 'successful build-finished'
Reject 'missing library is not an empty suite pass' { Read-TestTargets (JsonLines @($binary, $done)) $manifest } 'one library'
Pass 'dependency artifacts excluded' {
    $dependency = $library.Clone(); $dependency.manifest_path = Join-Path $fixture 'dependency/Cargo.toml'
    if (@(Read-TestTargets (JsonLines @($dependency, $library, $done)) $manifest).Count -ne 1) { throw 'dependency selected' }
}
Pass 'both original integration targets remain alongside library and binary' {
    $transport = $library.Clone(); $transport.target = @{kind=@('test');name='paste_transport'}; $transport.executable='paste_transport.exe'
    $claude = $library.Clone(); $claude.target = @{kind=@('test');name='paste_claude_e2e'}; $claude.executable='paste_claude_e2e.exe'
    $targets = @(Read-TestTargets (JsonLines @($library,$binary,$transport,$claude,$done)) $manifest)
    if ($targets.Count -ne 4 -or @($targets | Where-Object { $_.target.kind -contains 'test' }).Count -ne 2) { throw 'integration coverage lost' }
}
$rust = Join-Path $fixture 'fixture.rs'
$exe = Join-Path $fixture 'fixture.exe'
[IO.File]::WriteAllText($rust, @'
#[test] fn passing() {}
#[test] fn failing() { panic!("intentional CI contract failure"); }
#[test] #[ignore] fn worker() { panic!("must remain ignored"); }
'@)
& rustc --test $rust -o $exe
if ($LASTEXITCODE -ne 0) { throw 'fixture compilation failed' }
Pass 'real passing exact libtest with one ignored worker untouched' { if (!(Invoke-Libtest $exe @('passing', '--exact') $false 1)) { throw 'passing fixture refused' } }
Pass 'real failing libtest keeps its native failure' { if (Invoke-Libtest $exe @() $false) { throw 'native failure swallowed' } }
Reject 'exact zero-match test is rejected' { Invoke-Libtest $exe @('missing', '--exact') $false 1 } 'Expected 1'
Reject 'empty library cannot count as coverage' { Invoke-Libtest $exe @('missing', '--exact') $true } 'library inventory is empty'
Pass 'empty binary remains an explicit empty harness' { if (!(Invoke-Libtest $exe @('missing', '--exact') $false)) { throw 'genuine empty binary failed' } }
Write-Host "WINDOWS_RUST_CI_CONTRACTS passed=$passed fixture=$fixture"
