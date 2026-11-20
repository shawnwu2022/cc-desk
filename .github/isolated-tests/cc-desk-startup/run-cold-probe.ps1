$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if ((Get-Location).Path -ine (Join-Path $env:GITHUB_WORKSPACE 'product-source') -or $PSScriptRoot -ine $env:CCDESK_TEST_HELPERS_ROOT) { throw 'Unexpected isolated source or reviewed helper root' }
if (!$IsWindows -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or $env:GITHUB_ACTIONS -cne 'true' -or ![Environment]::UserInteractive -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Disposable interactive hosted Windows only' }
$source='04a2f0cedc8e7acc080e8736723c6e8799127f58'
$tree='933bae502bb1e262e6f0be2cd0bb600cb8992b8a'
$receipt=[ordered]@{
    testOnly=$true; publishable=$false; updaterPublication=$false; originalInstallAndSmokeGatesRetained=$true;
    workflowCommit=$env:GITHUB_SHA; workflowRun=[int64]$env:GITHUB_RUN_ID; workflowAttempt=[int]$env:GITHUB_RUN_ATTEMPT;
    productSource=$source; productTree=$tree; productVersion='0.28.0-rc.1';
    runnerImage=if ($env:ImageVersion -cmatch '^\d{8}\.\d{1,5}\.\d{1,5}$') { $env:ImageVersion } else { 'unavailable' };
    npmCacheRestored=$env:NPM_RESTORE_HIT -ceq 'true'; browserArchiveCacheRestored=$env:BROWSER_RESTORE_HIT -ceq 'true';
    rustTargetRestored=$false; coldNativeTarget=$false; trackedBuildSourceClean=$false; phase='preflight';
    distIndexSha256=$null; distModuleGraphSha256=$null; distInventorySha256=$null; distReferencesResolved=$false; frontendIdentityMatches=$false; moduleScripts=0; localReferences=0;
    installerSha256=$null; installerBytes=$null; memberName=$null; memberSha256=$null; memberBytes=$null; installerUnchanged=$false; installerInvocationsVerified=0; installedImagesVerified=0; installedSha256=$null; installedBytes=$null;
    originalInstall1=$false; originalInstall2=$false; relocationAndNegativeRuntimeChecks=$false; smoke=$null;
}
$receiptPath=Join-Path $env:GITHUB_WORKSPACE 'cold-startup-receipt.json'
$diagnostic=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'test-desktop-webview.ps1') -Raw
$success=$false
$heldInstaller=$null
try {
    $tokens=$null
    $errors=$null
    $null=[System.Management.Automation.Language.Parser]::ParseInput($diagnostic,[ref]$tokens,[ref]$errors)
    if (@($errors).Count -ne 0) { throw 'Diagnostic PowerShell syntax validation failed' }
    if ((git rev-parse HEAD) -cne $source -or (git rev-parse 'HEAD^{tree}') -cne $tree -or
        @(git status --porcelain | Where-Object { $_ -cnotmatch '^\?\? ' }).Count -ne 0 -or
        (Test-Path -LiteralPath 'src-tauri/target/release') -or (Test-Path -LiteralPath 'dist/index.html')) { throw 'Build does not have exact cold source' }
    if ((Get-Content -LiteralPath 'package.json' -Raw | ConvertFrom-Json).version -cne '0.28.0-rc.1' -or
        (Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json).version -cne '0.28.0-rc.1') { throw 'Exact manifests disagree' }
    $receipt.coldNativeTarget=$true
    $receipt.phase='ordinary-nsis-build'
    $env:CC_DESK_BUILD_SHA=$source
    & npm run tauri -- build --bundles nsis --config '{"bundle":{"createUpdaterArtifacts":false}}' *> (Join-Path $env:RUNNER_TEMP 'ordinary-nsis.local.log')
    if ($LASTEXITCODE -ne 0) { throw 'Original NSIS build failed' }
    [Environment]::SetEnvironmentVariable('CC_DESK_BUILD_SHA',$null,'Process')
    git diff --quiet HEAD --
    if ($LASTEXITCODE -ne 0 -or (git rev-parse HEAD) -cne $source) { throw 'Tracked product source changed during build' }
    $receipt.trackedBuildSourceClean=$true
    $receipt.phase='dist-validation'
    $assetProbe=Join-Path $PSScriptRoot 'dist-receipt.cjs'

    & node --check $assetProbe *> (Join-Path $env:RUNNER_TEMP 'dist-syntax.local.log')
    if ($LASTEXITCODE -ne 0) { throw 'Diagnostic asset verifier syntax rejected' }
    $distOutput= & node $assetProbe 'dist' $source 2>$null
    $distExit=$LASTEXITCODE
    if (!$distOutput -or "$distOutput".Length -gt 4096) { throw 'Missing bounded frontend receipt' }
    $data=$distOutput | ConvertFrom-Json -AsHashtable
    if ($data.stage -cnotin @('dist-html','dist-local-references','dist-inventory','dist-vite-identity','dist-complete')) { throw 'Unexpected dist stage' }
    $receipt.phase=$data.stage
    foreach ($field in @('distIndexSha256','distModuleGraphSha256','distInventorySha256')) {
        $value=$data[$field]
        if ($null -ne $value -and $value -cnotmatch '^[0-9a-f]{64}$') { throw 'Invalid dist digest' }
        $receipt[$field]=$value
    }
    foreach ($field in @('distReferencesResolved','frontendIdentityMatches')) {
        if ($data[$field] -isnot [bool]) { throw 'Invalid dist proof flag' }
        $receipt[$field]=$data[$field]
    }
    foreach ($field in @('moduleScripts','localReferences')) {
        if ($data[$field] -isnot [long] -and $data[$field] -isnot [int]) { throw 'Invalid dist count' }
        if ($data[$field] -lt 0 -or $data[$field] -gt 48) { throw 'Unbounded dist count' }
        $receipt[$field]=$data[$field]
    }
    if ($distExit -ne 0 -or $receipt.phase -cne 'dist-complete' -or !$receipt.distReferencesResolved -or !$receipt.frontendIdentityMatches) { throw 'Frontend integrity proof incomplete' }
    $receipt.phase='bind-nsis'
    $installers=@(Get-ChildItem -LiteralPath 'src-tauri/target/release/bundle/nsis' -Filter '*.exe' -File)
    if ($installers.Count -ne 1 -or ($installers[0].Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected one regular original NSIS' }
    $installer=$installers[0].FullName
    $receipt.phase='bind-pinned-installer'
    $heldInstaller=[IO.File]::Open($installer,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    $receipt.installerBytes=$heldInstaller.Length
    if ($receipt.installerBytes -le 0 -or $receipt.installerBytes -gt 50MB) { throw 'Installer bounds rejected' }
    $receipt.installerSha256=(Get-FileHash -InputStream $heldInstaller).Hash.ToLowerInvariant()
    $parser=Join-Path $PSScriptRoot 'verify_named_nsis.py'
    $binder=Join-Path $PSScriptRoot 'next_gate_binding.py'
    $installHelper=Join-Path $PSScriptRoot 'test-conpty-install-bound.ps1'
    if ((& python --version 2>$null) -cnotmatch '^Python 3\.(1[2-4])\.[0-9]+$') { throw 'Expected hosted Python runtime not found' }



    $receipt.phase='bind-named-nsis-member'
    $checkInvocation={
        param([string]$Invoked)
        $receipt.phase='verify-installer-invocation'
        $local=& python $binder 'installer' $Invoked $installer $receipt.installerSha256 2>$null
        $status=$LASTEXITCODE
        if ($status -ne 0 -or !$local -or "$local".Length -gt 1024) { throw 'Pinned invocation identity verification failed' }
        $p=$local | ConvertFrom-Json -AsHashtable
        if ($p.installerSha256 -cne $receipt.installerSha256 -or $p.installerBytes -ne $receipt.installerBytes) { throw 'Pinned invocation receipt differs' }
        $receipt.installerInvocationsVerified++
    }.GetNewClosure()
    & $checkInvocation $installer
    $receipt.installerInvocationsVerified=0 # The actual two launches increment it, not this pre-parser check.
    $receipt.phase='bind-named-nsis-member'
    $result=& python $parser --installer $installer --sha256 $receipt.installerSha256 2>$null
    $status=$LASTEXITCODE
    if ($status -ne 0 -or !$result -or "$result".Length -gt 1024) { throw 'Named installer member verification failed' }
    $member=$result | ConvertFrom-Json -AsHashtable
    if ($member.installerSha256 -cne $receipt.installerSha256 -or $member.member -cne 'cc-desk.exe' -or
        $member.format -cne 'tauri-nsis-unicode-solid-lzma-x64' -or $member.memberSha256 -cnotmatch '^[0-9a-f]{64}$' -or
        ($member.memberBytes -isnot [int] -and $member.memberBytes -isnot [long]) -or $member.memberBytes -le 0 -or $member.memberBytes -ge 150MB) { throw 'Named member receipt rejected' }
    $receipt.memberName=$member.member
    $receipt.memberSha256=$member.memberSha256
    $receipt.memberBytes=$member.memberBytes
    $checkInstalled={
        param([string]$Selected,[string]$Directory)
        $receipt.phase='verify-installed-image'
        $local=& python $binder 'installed' $Selected $Directory $receipt.memberName $receipt.memberSha256 $receipt.memberBytes 2>$null
        $status=$LASTEXITCODE
        if ($status -ne 0 -or !$local -or "$local".Length -gt 1024) { throw 'Installed image identity verification failed' }
        $p=$local | ConvertFrom-Json -AsHashtable
        if ($p.installedSha256 -cne $receipt.memberSha256 -or $p.installedBytes -ne $receipt.memberBytes) { throw 'Independent installed-image receipt differs' }
        $receipt.installedSha256=$p.installedSha256
        $receipt.installedBytes=$p.installedBytes
        $receipt.installedImagesVerified++
        $receipt.phase='ordinary-installed-probe'
    }.GetNewClosure()
    $tokens=$null
    $errors=$null
    $null=[System.Management.Automation.Language.Parser]::ParseFile($installHelper,[ref]$tokens,[ref]$errors)
    if (@($errors).Count -ne 0) { throw 'Bound installer helper syntax validation failed' }
    $receipt.phase='ordinary-installed-probe'
    if ((Test-Path -LiteralPath (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cc-box')) -or
        (Test-Path -LiteralPath (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'io.github.shawnwu2022.ccdesk')) -or
        @(Get-Process cc-desk -ErrorAction SilentlyContinue).Count -ne 0 -or
        @(Get-ChildItem Env: | Where-Object Name -like 'WEBVIEW2_*').Count -ne 0) { throw 'Existing app state or WebView override; refusing before installation' }
    $diagnostic | Set-Content -LiteralPath 'scripts/test-desktop-webview.ps1' -Encoding utf8
    if ((git diff --name-only HEAD --) -cne 'scripts/test-desktop-webview.ps1') { throw 'Unexpected local test checkout change' }
    $boundHelper=Join-Path (Get-Item -LiteralPath 'scripts').FullName 'scratch-test-conpty-install-bound.ps1'
    # Invoke beside the stock scripts for identical PSScriptRoot and cwd.
    Copy-Item -LiteralPath $installHelper -Destination $boundHelper
    if ((Get-FileHash -LiteralPath $installHelper).Hash -cne (Get-FileHash -LiteralPath $boundHelper).Hash) { throw 'Local helper copy differs' }
    $env:CCDESK_SCRATCH_EXPECTED_EXE=$receipt.memberSha256
    $env:CCDESK_SCRATCH_EXPECTED_BYTES=[string]$receipt.memberBytes
    [Environment]::SetEnvironmentVariable('CCDESK_TEST_HELPERS_ROOT',$null,'Process')
    Get-ChildItem Env: | Where-Object { $_.Name -cne 'GITHUB_ACTIONS' -and $_.Name -match '^(ACTIONS_|GITHUB_|GH_)|TOKEN|SECRET|API_KEY|PASSWORD|CREDENTIAL' } | ForEach-Object {
        [Environment]::SetEnvironmentVariable($_.Name, $null, 'Process')
    }
    $privateLog=Join-Path $env:RUNNER_TEMP 'ordinary-startup.local.log'
    $passed=$false
    try {
        & $boundHelper -Installer $installer -VerifyInvocation $checkInvocation -VerifyInstalled $checkInstalled *> $privateLog
        $passed=$true
    } catch { $passed=$false }
    if (Test-Path -LiteralPath $privateLog -PathType Leaf) {
        $receipt.originalInstall1=[bool](Select-String -LiteralPath $privateLog -Pattern '^PASS install-1$' -Quiet)
        $receipt.originalInstall2=[bool](Select-String -LiteralPath $privateLog -Pattern '^PASS install-2$' -Quiet)
    }
    $receipt.relocationAndNegativeRuntimeChecks=$passed
    $heldInstaller.Seek(0,[IO.SeekOrigin]::Begin) | Out-Null
    $receipt.installerUnchanged=(Get-FileHash -InputStream $heldInstaller).Hash -ieq $receipt.installerSha256
    $record='safe-webview-probe/result.json'
    if (Test-Path -LiteralPath $record -PathType Leaf) {
        $safe=Get-Item -LiteralPath $record
        if ($safe.Length -lt 4096 -and !($safe.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            $receipt.smoke=Get-Content -LiteralPath $record -Raw | ConvertFrom-Json -AsHashtable
        }
    }
    if ($passed -and $receipt.originalInstall1 -and $receipt.originalInstall2 -and $receipt.installerUnchanged -and
        $receipt.installerInvocationsVerified -eq 2 -and $receipt.installedImagesVerified -eq 2 -and $receipt.smoke -and
        $receipt.smoke.status -ceq 'passed' -and $receipt.smoke.normalExit -and
        $receipt.smoke.sha256 -ieq $receipt.memberSha256 -and $receipt.smoke.imageBytes -eq $receipt.memberBytes) {
        $receipt.phase='complete'
        $success=$true
    }
} catch {
    # Even local build or installation errors never enter the uploaded data.
    $success=$false
} finally {
    if ($heldInstaller) { $heldInstaller.Dispose() }
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $receiptPath -Encoding utf8
}
if (!$success) { throw 'Cold exact-source comparison did not pass; see sanitized phase receipt.' }
