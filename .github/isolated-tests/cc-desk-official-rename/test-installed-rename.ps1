# Internal helper; called with a pinned, read-held install and Codex by test-fresh-official-rename.ps1.
param([Parameter(Mandatory=$true)][string]$Exe, [Parameter(Mandatory=$true)][string]$FixtureCodex)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$out = Join-Path (Get-Location) 'official-rename-evidence'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$result = [ordered]@{ status = 'blocked'; normalExit = $false; evidence = 'non-archived record only; no open/running session claim'; stopCode = $null }
$app = $null
$window = $null
$children = @{}
function Wait-Observed([scriptblock]$Observe, [string]$Label) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        if ($app -and $app.HasExited) { throw "App exited before $Label" }
        $value = & $Observe
        if ($null -ne $value -and $value -ne $false) { return $value }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "BLOCKED_UI_OBSERVATION: $Label"
}
function Find-Control([string[]]$Names, $Type) {
    $matches = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
        !$_.Current.IsOffscreen -and $_.Current.Name -cin $Names -and (!$Type -or $_.Current.ControlType -eq $Type)
    })
    if ($matches.Count -gt 1) { throw "Ambiguous UI selector: $($Names -join ', ')" }
    if ($matches.Count -eq 1) { return $matches[0] }
    return $null
}
function Invoke-Button([string[]]$Names) {
    $control = Wait-Observed { Find-Control $Names ([System.Windows.Automation.ControlType]::Button) } ($Names -join '/')
    $pattern = $null
    if (!$control.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { throw ("BLOCKED_UI_INVOKE: $($Names -join '/')") }
    ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
}
function Reveal-FixtureControl([string[]]$Names, $Type) {
    $seen = @{ reported=$false; revealed=$false }
    $null = Wait-Observed {
        $found = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
            $_.Current.Name -cin $Names -and (!$Type -or $_.Current.ControlType -eq $Type)
        })
        if ($found.Count -gt 1) { throw "Ambiguous fixture control: $($Names -join '/')" }
        if ($found.Count -eq 0) { return $null }
        $target = $found[0]
        if (!$seen.reported) {
            $result.codexHistory.controls += [ordered]@{
                expectedLabel=($Names -join '/')
                enabled=$target.Current.IsEnabled
                offscreen=$target.Current.IsOffscreen
                patterns=@($target.GetSupportedPatterns() | ForEach-Object ProgrammaticName)
            }
            $seen.reported = $true
        }
        if (!$target.Current.IsOffscreen) { return $target }
        if (!$seen.revealed) {
            $scroll = $null
            if (!$target.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern, [ref]$scroll)) {
                throw "BLOCKED_UI_SCROLL: $($Names -join '/')"
            }
            ([System.Windows.Automation.ScrollItemPattern]$scroll).ScrollIntoView()
            $seen.revealed = $true
        }
        return $null
    } "fixture fixed control reveal: $($Names -join '/')"
}
function Observe-Children {
    $owned = @{ ([int]$app.Id) = $app }
    $pending=[Collections.Generic.Queue[int]]::new()
    $pending.Enqueue([int]$app.Id)
    while ($pending.Count) {
        $parentId=$pending.Dequeue()
        if ($owned[$parentId].HasExited) { throw 'Owned parent exited during observation' }
        $rows=@(Get-CimInstance -ClassName Win32_Process -Filter "ParentProcessId = $parentId" -Property ParentProcessId,ProcessId,CreationDate,ExecutablePath,SessionId -ErrorAction Stop)
        foreach ($row in $rows) {
            if ([int]$row.ParentProcessId -ne $parentId -or $owned.ContainsKey([int]$row.ProcessId)) { throw 'Unverified parent traversal' }
            $id = [int]$row.ProcessId
            $process = $null
            try {
                $parent = $owned[[int]$row.ParentProcessId]
                $process = [Diagnostics.Process]::GetProcessById($id)
                $null = $process.Handle
                # CIM timestamps have microsecond precision; compare the captured handle,
                # not a second PID lookup. Any exit/reuse ambiguity blocks acceptance.
                $created = ([DateTime]$row.CreationDate).ToUniversalTime()
                $started = $process.StartTime.ToUniversalTime()
                if ($parent.HasExited -or $process.HasExited -or
                    ($created.Ticks - ($created.Ticks % 10)) -ne ($started.Ticks - ($started.Ticks % 10)) -or
                    !$row.ExecutablePath -or $process.MainModule.FileName -ine $row.ExecutablePath -or
                    $process.SessionId -ne $app.SessionId -or [int]$row.SessionId -ne $app.SessionId -or
                    $started -lt $parent.StartTime.ToUniversalTime()) { throw 'Unverified child identity or parent lifetime' }
                if ($owned.Count -ge 32) { throw 'Owned observation exceeded safe count' }
                $children[$id] = $process
                $owned[$id] = $process
                $pending.Enqueue($id)
                $process = $null
            } finally {
                # Never register or kill a handle whose ancestry/identity was not verified.
                if ($process) { $process.Dispose() }
            }
        }
    }
}

try {
    if (!$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or !$env:RUNNER_TEMP -or ![Environment]::UserInteractive -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'BLOCKED_EXTERNAL_TARGET: disposable interactive hosted Windows required' }
    if (@(Get-ChildItem Env: | Where-Object { $_.Name -like 'WEBVIEW2_*' }).Count -ne 0) { throw 'BLOCKED_EXTERNAL_TARGET: WebView2 environment override' }
    # Locked Tauri 2.11.1 manager/webview.rs defaults to LocalData/identifier.
    $webviewRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'io.github.shawnwu2022.ccdesk'
    if (Test-Path -LiteralPath $webviewRoot) { throw 'BLOCKED_EXTERNAL_TARGET: existing default WebView data; never inspect or reset it' }
    if (@(Get-Process cc-desk -ErrorAction SilentlyContinue).Count -ne 0) { throw 'BLOCKED_EXTERNAL_TARGET: existing App' }
    $homeRoot = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cc-box'
    if (Test-Path -LiteralPath $homeRoot) { throw 'BLOCKED_EXTERNAL_TARGET: existing application data; never inspect or reset it' }
    $image = (Resolve-Path -LiteralPath $Exe).Path
    $temp = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
    if (!$image.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase)) { throw 'Installed image must be inside RUNNER_TEMP' }
    $result.sha256 = (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash
    $result.imageBytes=(Get-Item -LiteralPath $image).Length
    if ($result.sha256 -ine 'cd55c6504093a7aa884cc8936bc51b58a133a512d8d496ea0530d64e9999b6f5' -or $result.imageBytes -ne 56135680) {
        throw 'Official installed image mismatch'
    }

    if ($FixtureCodex) {
        $result.codexHistory=[ordered]@{stage='fixture-read'; cli='0.169.1'; preflight=$false; rename='not-attempted'; controls=@(); archivedMenu='not-tested'; delete='held'; optionsControls=@(); profileSaved=$false; selectedProjectExact=$false; recordedCanonicalExact=$false; metaCwdExact=$false; profilePinMatches=$false; sessionsTreeObserved=$false; projectTreeObserved=$false; expansionInvoked=$false; projectExpanded=$false; nativePreflight=$false; activeRowObserved=$false; activeRowUuidMatched=$false; oldEditorMatched=$false; inputReadbackMatched=$false; refreshedTitleMatched=$false; postRenameUuidMatched=$false; nativeReadbackMatched=$false; saveInvocationCount=0; uiSessionOpenedByHarness=$false; runningCliSessionProven=$false; nonArchivedNativeRecord=$false; appServerExecutableProven=$false; productObserverArmed=$false; productCodexChildImageObserved=$false; productCodeHomeInputBound=$false; productCodexPinnedVersion=$null; productChildEnvironmentInspected=$false}
        $fixture=& (Join-Path $PSScriptRoot 'pr101-codex-history-fixture.ps1') -Codex $FixtureCodex
        if (!$fixture.AppServerReadVerified) { throw 'Unverified synthetic history inputs' }
        $result.codexHistory.preflight=$true
        $env:CODEX_HOME=$fixture.Home
        $env:PATH=(Split-Path $FixtureCodex -Parent) + ';' + $env:PATH
        . (Join-Path $PSScriptRoot 'pr101-codex-history-gui.ps1')
        . (Join-Path $PSScriptRoot 'observe-product-codex.ps1')
    }
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
    # Visible UI is the explicit purpose of this hosted-only acceptance step.
    # This disposable process does not forward service or proxy credentials to the installed app.
    $inherited=@(Get-ChildItem Env: | Where-Object {
        $_.Name -ne 'GITHUB_ACTIONS' -and $_.Name -match '^(ACTIONS_|GITHUB_|GH_|OPENAI_|CHATGPT_|CODEX_(?!HOME$))|TOKEN|SECRET|API_KEY|PASSWORD|CREDENTIAL|^(HTTPS?|ALL)_PROXY$'
    })
    if ($inherited.Count) { throw 'Run wrapper did not scrub auth/proxy environment before product launch' }
    $spec=[Diagnostics.ProcessStartInfo]::new($image)
    $spec.UseShellExecute=$false
    $spec.WorkingDirectory=Split-Path $image
    $spec.Environment.Clear()
    foreach ($key in @('SYSTEMROOT','WINDIR','SYSTEMDRIVE','OS','COMSPEC','PATHEXT','PROCESSOR_ARCHITECTURE',
                        'USERPROFILE','HOME','HOMEDRIVE','HOMEPATH','LOCALAPPDATA','APPDATA','PROGRAMDATA','PROGRAMFILES','PROGRAMFILES(X86)','TEMP','TMP','PATH','CODEX_HOME')) {
        $value=[Environment]::GetEnvironmentVariable($key)
        if ($value) { $spec.Environment[$key]=$value }
    }
    if ($spec.Environment['CODEX_HOME'] -cne $fixture.Home) { throw 'Product synthetic CODEX_HOME mismatch' }
    Assert-ExactFixtureConfig
    $app=[Diagnostics.Process]::Start($spec)
    $result.codexHistory.productCodeHomeInputBound=$true
    $null = $app.Handle
    if ($app.MainModule.FileName -ine $image) { throw 'Started image identity mismatch' }
    $window = Wait-Observed {
        $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $app.Id)
        $all = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
        $visible = @($all | Where-Object { !$_.Current.IsOffscreen -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window })
        if ($visible.Count -gt 1) { throw 'Ambiguous original top-level window' }
        if ($visible.Count -eq 1) { return $visible[0] }
    } 'ordinary main window'
    if ($FixtureCodex) {
        $result.codexHistory.stage='initial-settings-observation'
        $settingsControl=Wait-Observed {
            $same=@($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
                $_.Current.Name -ceq 'Settings' -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button
            })
            if ($same.Count -gt 1) { throw 'Ambiguous initial Settings controls' }
            if (!$same.Count) { return $null }
            $observed=$same[0]
            $result.initialSettings=[ordered]@{
                enabled=$observed.Current.IsEnabled
                offscreen=$observed.Current.IsOffscreen
                patterns=@($observed.GetSupportedPatterns() | ForEach-Object ProgrammaticName)
            }
            if ($observed.Current.IsOffscreen) {
                $scroll=$null
                if (!$observed.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)) { throw 'Initial Settings is offscreen without an observed scroll pattern' }
                ([System.Windows.Automation.ScrollItemPattern]$scroll).ScrollIntoView()
                return $null
            }
            if ($observed.Current.IsEnabled) { return $observed }
        } 'initial fixed Settings control'
        $click=$null
        if (!$settingsControl.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$click)) { throw 'Initial Settings lacks an observed InvokePattern' }
        ([System.Windows.Automation.InvokePattern]$click).Invoke()
    }
    Invoke-Button @('General')
    if ($FixtureCodex) {
        # Fresh product defaults hide Codex. Enable it through the observed General control.
        $codexOffLabel = "Codex CLI disabled $([char]0x00B7) click to enable"
        $codexOnLabel = "Codex CLI enabled $([char]0x00B7) click to disable"
        $codexToggle = Wait-Observed { Find-Control @($codexOffLabel) ([System.Windows.Automation.ControlType]::Button) } 'Codex support default off'
        $pattern = $null
        if (!$codexToggle.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) { throw 'Codex support toggle unavailable' }
        $pattern = [System.Windows.Automation.TogglePattern]$pattern
        if ($pattern.Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off) { throw 'Codex support was not default off' }
        $pattern.Toggle()
        $null = Wait-Observed {
            $control = Find-Control @($codexOnLabel) ([System.Windows.Automation.ControlType]::Button)
            $state = $null
            if ($control -and $control.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$state) -and ([System.Windows.Automation.TogglePattern]$state).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On) { return $control }
            return $null
        } 'Codex support enabled through GUI'
    }
    $testFailure=$null
    try { Test-CodexHistoryGui } catch { $testFailure=$_.Exception.Message }
    Observe-Children
    $result.observedOwnedChildren = @($children.Values).Count
    if (@($children.Values | Where-Object ProcessName -eq 'msedgewebview2').Count -eq 0) { throw 'No actual WebView2 child observed' }
    $close = $null
    if (!$window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$close)) { throw 'BLOCKED_UI_CLOSE' }
    ([System.Windows.Automation.WindowPattern]$close).Close()
    if (!$app.WaitForExit(30000) -or $app.ExitCode -ne 0) { throw 'Ordinary App did not close normally' }
    foreach ($child in $children.Values) { if (!$child.WaitForExit(10000)) { throw 'Observed WebView child did not exit' } }
    $result.normalExit = $true
    if ($testFailure) { $result.status='partial'; throw $testFailure }
    if (!$result.codexHistory.nativeReadbackMatched -or !$result.codexHistory.productCodexChildImageObserved -or $result.codexHistory.saveInvocationCount -ne 1) { throw 'Scoped real rename evidence was not completed' }
    $result.status = 'passed'
} catch {
    if ($app -and $result.status -cne 'partial') { $result.status = 'failed' }
    $result.stopCode='RENAMING_ACCEPTANCE_BLOCKED'
    throw 'Isolated ordinary rename acceptance blocked; see safe stages.'
} finally {
    # Failure cleanup is never retirement evidence or a passed UI acceptance.
    try {
        if ($app -and !$app.HasExited) { $app.Kill(); $null = $app.WaitForExit(10000) }
        foreach ($child in $children.Values) { if (!$child.HasExited) { $child.Kill(); $null = $child.WaitForExit(10000) }; $child.Dispose() }
    } catch {
        $result.stopCode='OWNED_CLEANUP_BLOCKED'
        $result.status = 'failed'
        throw 'Owned cleanup blocked.'
    } finally {
        $result | ConvertTo-Json -Depth 7 | Set-Content -Encoding utf8 (Join-Path $out 'result.json')
        if ($app) { $app.Dispose() }
    }
}
