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

# Exercise the actual hosted selectors/summary reader with effect-free libtest binaries.
$logs = Join-Path $fixture 'logs'; [IO.Directory]::CreateDirectory($logs) | Out-Null
$hostedRust = Join-Path $fixture 'hosted.rs'
$hostedExe = Join-Path $fixture 'hosted.exe'
$collisionExe = Join-Path $fixture 'collision.exe'
$failedExe = Join-Path $fixture 'selected-failure.exe'
[IO.File]::WriteAllText($hostedRust, @'
#[test] fn passing() {}
#[test] fn ordinary_new() {}
#[test] fn excluded_exact() { panic!("fixture host requirement unavailable"); }
#[test] #[ignore = "supervised only"] fn worker() { panic!("must stay ignored"); }
#[cfg(prefix_collision)] #[test] fn excluded_exact_suffix() {}
#[cfg(selected_failure)] #[test] fn selected_failure() { panic!("selected failure must block"); }
'@)
& rustc --test $hostedRust -o $hostedExe
if ($LASTEXITCODE -ne 0) { throw 'hosted fixture compilation failed' }
& rustc --test --cfg prefix_collision $hostedRust -o $collisionExe
if ($LASTEXITCODE -ne 0) { throw 'collision fixture compilation failed' }
& rustc --test --cfg selected_failure $hostedRust -o $failedExe
if ($LASTEXITCODE -ne 0) { throw 'failure fixture compilation failed' }
$fixtureScope = [pscustomobject]@{ jobFreeTests = @('excluded_exact'); unelevatedTests = @(); requiredSelectedTests = @('passing'); ordinaryRequiredSelectedTests = @() }
$full = Read-HostedInventory $hostedExe @() (Join-Path $logs 'full.log')
$ignored = Read-HostedInventory $hostedExe @('--ignored') (Join-Path $logs 'ignored.log')
$selection = Get-HostedTestSelection -Full $full.entries -Ignored $ignored.names -Library $true -Contained $true -Elevated $false -Scope $fixtureScope
$selected = Read-HostedInventory $hostedExe @('--skip', 'excluded_exact') (Join-Path $logs 'selected.log')
Pass 'hosted selection exactly reconciles F/I/S/E and unknown ordinary name remains selected' {
    Assert-HostedSelection $full.names $ignored.names $selected.names $selection.excluded
    if ($selected.names -cnotcontains 'ordinary_new' -or $ignored.names.Count -ne 1 -or $selection.excluded.Count -ne 1) { throw 'selection lost ordinary/ignored/excluded distinction' }
}
Pass 'actual passing hosted body preserves ignored reason and exact filtered count' {
    $plan = [pscustomobject]@{ executable = $hostedExe; ignored = @($ignored.names); selected = @($selected.names); excluded = @($selection.excluded); logs = @{execution='logs/body.log'}; result=$null; defaultIgnored=@() }
    if (!(Invoke-HostedHarness $plan $fixture)) { throw 'hosted passing fixture refused' }
    if ($plan.result.passed -ne 2 -or $plan.result.filteredOut -ne 1 -or $plan.result.ignored -ne 1 -or $plan.defaultIgnored[0].reason -cne 'supervised only') { throw 'outer summary/ignored reason wrong' }
}
Reject 'actual substring collision is rejected before test body' {
    $collision = Read-HostedInventory $collisionExe @() (Join-Path $logs 'collision-full.log')
    Get-HostedTestSelection -Full $collision.entries -Ignored $ignored.names -Library $true -Contained $true -Elevated $false -Scope $fixtureScope
} 'collision'
Reject 'actual libtest prefix filtering cannot conceal an extra omitted test' {
    $collision = Read-HostedInventory $collisionExe @() (Join-Path $logs 'collision-full.log')
    $collisionSelected = Read-HostedInventory $collisionExe @('--skip', 'excluded_exact') (Join-Path $logs 'collision-selected.log')
    Assert-HostedSelection $collision.names $ignored.names $collisionSelected.names @('excluded_exact')
} 'selection'
Reject 'nonexistent skip is not a covered unavailable test' { Assert-HostedSelection $full.names $ignored.names $full.names @('missing') } 'missing'
Reject 'broad skip cannot omit an ordinary test' {
    $broad = Read-HostedInventory $hostedExe @('--skip', 'passing') (Join-Path $logs 'broad.log')
    Assert-HostedSelection $full.names $ignored.names $broad.names @('excluded_exact')
} 'selection'
Reject 'ignored worker cannot be relabeled unavailable' { Assert-HostedSelection $full.names $ignored.names $selected.names @('worker') } 'ignored'
Reject 'empty nonignored library cannot become a hosted pass' {
    Get-HostedTestSelection -Full @([pscustomobject]@{name='worker';type='test'}) -Ignored @('worker') -Library $true -Contained $true -Elevated $false -Scope ([pscustomobject]@{jobFreeTests=@();unelevatedTests=@();requiredSelectedTests=@();ordinaryRequiredSelectedTests=@()})
} 'empty'
Reject 'policy entry changed to ignored is rejected' { Get-HostedTestSelection -Full $full.entries -Ignored @('excluded_exact') -Library $true -Contained $true -Elevated $false -Scope $fixtureScope } 'policy drift'
Pass 'Job-free selector includes every full inventory name' {
    $all = Get-HostedTestSelection -Full $full.entries -Ignored $ignored.names -Library $true -Contained $false -Elevated $false -Scope $fixtureScope
    if ($all.excluded.Count -ne 0 -or !(Test-ExactSet $all.selected $full.names)) { throw 'job-free selector filtered a test' }
}
Pass 'actual elevated selection separates ordinary unavailable from mandatory contracts' {
    $elevationScope = [pscustomobject]@{jobFreeTests=@('excluded_exact');unelevatedTests=@('ordinary_new');requiredSelectedTests=@('passing');ordinaryRequiredSelectedTests=@('passing')}
    $elevatedSelection = Get-HostedTestSelection -Full $full.entries -Ignored $ignored.names -Library $true -Contained $true -Elevated $true -Scope $elevationScope
    if (!(Test-ExactSet $elevatedSelection.excluded @('excluded_exact','ordinary_new')) -or $elevatedSelection.selected -cnotcontains 'passing' -or $elevatedSelection.selected -cnotcontains 'worker') { throw 'elevated classification lost mandatory/original ignored distinction' }
    $naturalSelection = Get-HostedTestSelection -Full $full.entries -Ignored $ignored.names -Library $true -Contained $true -Elevated $false -Scope $elevationScope
    if ($naturalSelection.selected -cnotcontains 'ordinary_new' -or !(Test-ExactSet $naturalSelection.excluded @('excluded_exact'))) { throw 'unelevated ordinary integration omitted' }
}
Pass 'real selected failure keeps exit101 and failed outer count' {
    $failedFull = Read-HostedInventory $failedExe @() (Join-Path $logs 'failure-full.log')
    $failedSelected = Read-HostedInventory $failedExe @('--skip', 'excluded_exact') (Join-Path $logs 'failure-selected.log')
    $plan = [pscustomobject]@{ executable = $failedExe; ignored = @($ignored.names); selected = @($failedSelected.names); excluded = @('excluded_exact'); logs = @{execution='logs/failure-body.log'}; result=$null; defaultIgnored=@() }
    if (Invoke-HostedHarness $plan $fixture) { throw 'selected native failure swallowed' }
    if ($plan.result.exitCode -ne 101 -or $plan.result.failed -ne 1) { throw 'failed native result lost' }
}
Pass 'final outer summary wins over a nested intentionally failing worker summary' {
    $result = Read-HostedResult @('test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out;', 'test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 1 filtered out;') 0
    if ($result.failed -ne 0 -or $result.passed -ne 2 -or $result.filteredOut -ne 1) { throw 'nested worker became outer result' }
}

function Write-FixtureRustNamespace($Node) {
    foreach ($moduleName in $Node.modules.Keys) { "mod $moduleName {"; Write-FixtureRustNamespace $Node.modules[$moduleName]; '}' }
    foreach ($testName in $Node.tests) { "#[test] fn $testName() {}" }
}
Pass 'whole hosted helper inventories four real std harnesses and validates artifact with a Boolean result' {
    $scopePath = Join-Path $PSScriptRoot '../../scripts/windows-native-scope.json'
    $audited = Get-Content -LiteralPath $scopePath -Raw -Encoding utf8 | ConvertFrom-Json
    $tree = @{ modules = @{}; tests = @() }
    foreach ($name in @($audited.jobFreeTests) + @($audited.requiredSelectedTests) + @($audited.unelevatedTests) + @($audited.ordinaryRequiredSelectedTests)) {
        $parts = $name.Split('::', [StringSplitOptions]::None); $node = $tree
        for ($i = 0; $i -lt $parts.Count - 1; $i++) {
            if (!$node.modules.ContainsKey($parts[$i])) { $node.modules[$parts[$i]] = @{ modules = @{}; tests = @() } }
            $node = $node.modules[$parts[$i]]
        }
        $node.tests += $parts[-1]
    }
    $body = @('#![allow(non_snake_case)]', (Write-FixtureRustNamespace $tree), '#[test] fn future_ordinary() {}', '#[test] #[ignore = "supervised fixture"] fn ignored_worker() { panic!("must stay ignored"); }')
    $libraryRust = Join-Path $fixture 'all-names.rs'; $libraryExe = Join-Path $fixture 'all-names.exe'
    $body | Set-Content -LiteralPath $libraryRust -Encoding utf8
    & rustc --test $libraryRust -o $libraryExe
    if ($LASTEXITCODE -ne 0) { throw 'whole hosted library fixture compilation failed' }
    $emptyRust = Join-Path $fixture 'empty.rs'; $emptyExe = Join-Path $fixture 'empty.exe'
    [IO.File]::WriteAllText($emptyRust, '')
    & rustc --test $emptyRust -o $emptyExe
    if ($LASTEXITCODE -ne 0) { throw 'whole hosted empty fixture compilation failed' }
    $ownedTargets = @($audited.harnesses | ForEach-Object {
        [pscustomobject]@{ target = @{name=$_.name;kind=@($_.kind)}; executable=$(if ($_.kind -eq 'lib') { $libraryExe } else { $emptyExe }) }
    })
    # The controlled doc adapter executes the owned empty std harness. It does
    # not compile or inventory application code and is not real doctest proof.
    function cargo { & $emptyExe }
    $oldSha = $env:GITHUB_SHA; $oldRun = $env:GITHUB_RUN_ID; $oldAttempt = $env:GITHUB_RUN_ATTEMPT; $oldOutput = $env:GITHUB_OUTPUT
    try {
        $env:GITHUB_SHA = (& git rev-parse HEAD).Trim(); $env:GITHUB_RUN_ID = '12345'; $env:GITHUB_RUN_ATTEMPT = '2'; $env:GITHUB_OUTPUT = Join-Path $fixture 'github-output'
        $result = Invoke-HostedSuite -Targets $ownedTargets -Contained $true -Elevated $false -Manifest $manifest -ScriptsDirectory (Resolve-Path (Join-Path $PSScriptRoot '../../scripts')).Path
        if ($result -isnot [bool] -or !$result) { throw 'whole helper did not return exactly Boolean true' }
        $reportPath = Join-Path $fixture 'target/windows-native-coverage/windows-native-coverage.json'
        $report = Get-Content -LiteralPath $reportPath -Raw -Encoding utf8 | ConvertFrom-Json
        if (!$report.completed -or $report.harnesses.Count -ne 4 -or $report.harnesses[0].excluded.Count -ne 18 -or $report.nativeAll.status -ne 'unverified' -or $report.nativeAcceptanceProven) { throw 'whole helper report contract wrong' }
        $emitted = Get-Content -LiteralPath $env:GITHUB_OUTPUT -Raw -Encoding utf8
        if ($emitted -notmatch 'coverage_name=windows-native-coverage-' -or $emitted -notmatch 'coverage_path=src-tauri/target/windows-native-coverage') { throw 'coverage outputs missing' }
        $elevatedResult = Invoke-HostedSuite -Targets $ownedTargets -Contained $true -Elevated $true -Manifest $manifest -ScriptsDirectory (Resolve-Path (Join-Path $PSScriptRoot '../../scripts')).Path
        if ($elevatedResult -isnot [bool] -or !$elevatedResult) { throw 'elevated helper did not return Boolean true' }
        $elevatedReport = Get-Content -LiteralPath $reportPath -Raw -Encoding utf8 | ConvertFrom-Json
        if (!$elevatedReport.host.elevationQuerySucceeded -or !$elevatedReport.host.elevated -or $elevatedReport.harnesses[0].excluded.Count -ne 22 -or $elevatedReport.nativeJobSuite.unverifiedNames.Count -ne 18 -or $elevatedReport.nativeUnelevatedSuite.unverifiedNames.Count -ne 4 -or $elevatedReport.nativeUnelevatedSuite.reason -cne 'elevated_host') { throw 'elevated whole helper disclosure contract wrong' }
        foreach ($name in $audited.ordinaryRequiredSelectedTests) { if ($elevatedReport.harnesses[0].selected -cnotcontains $name) { throw 'mandatory admission/cleanup contract omitted' } }
        $env:GITHUB_SHA = 'b' * 40
        Reject 'whole helper refuses mismatched source binding before any body' { Invoke-HostedSuite -Targets $ownedTargets -Contained $true -Elevated $false -Manifest $manifest -ScriptsDirectory (Resolve-Path (Join-Path $PSScriptRoot '../../scripts')).Path } 'source binding'
    } finally {
        $env:GITHUB_SHA=$oldSha; $env:GITHUB_RUN_ID=$oldRun; $env:GITHUB_RUN_ATTEMPT=$oldAttempt; $env:GITHUB_OUTPUT=$oldOutput
        Remove-Item -LiteralPath 'Function:cargo'
    }
}
Write-Host "WINDOWS_RUST_CI_CONTRACTS passed=$passed fixture=$fixture"
