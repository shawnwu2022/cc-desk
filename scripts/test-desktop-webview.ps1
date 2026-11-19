# Ordinary installed product, disposable hosted Windows only. Never a fake IPC page.
param([Parameter(Mandatory=$true)][string]$Exe, [switch]$ActivityDiagnosticsAcceptance, [string]$FixtureCodex)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$out = Join-Path (Get-Location) 'desktop-webview-smoke'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$result = [ordered]@{ status = 'blocked'; normalExit = $false; evidence = 'visible UI state, not authentication or zero network'; error = $null }
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
function Reveal-DiagnosticsControl([string[]]$Names, $Type) {
    $seen = @{ reported=$false; revealed=$false }
    $null = Wait-Observed {
        $found = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
            $_.Current.Name -cin $Names -and (!$Type -or $_.Current.ControlType -eq $Type)
        })
        if ($found.Count -gt 1) { throw "Ambiguous diagnostics control: $($Names -join '/')" }
        if ($found.Count -eq 0) { return $null }
        $target = $found[0]
        if (!$seen.reported) {
            $result.activityDiagnostics.controls += [ordered]@{
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
    } "diagnostics fixed control reveal: $($Names -join '/')"
}
function Toggle-DiagnosticsButton([string[]]$Names) {
    $ctl = Wait-Observed { Find-Control $Names ([System.Windows.Automation.ControlType]::Button) } ($Names -join '/')
    $pattern = $null
    $result.activityDiagnostics.controls += [ordered]@{
        expectedLabel=($Names -join '/')
        enabled=$ctl.Current.IsEnabled
        patterns=@($ctl.GetSupportedPatterns() | ForEach-Object ProgrammaticName)
    }
    if (!$ctl.Current.IsEnabled -or !$ctl.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
        throw "BLOCKED_UI_TOGGLE: $($Names -join '/')"
    }
    ([System.Windows.Automation.TogglePattern]$pattern).Toggle()
}
function Reveal-RemoteSection {
    $matches = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
        $_.Current.Name -cin @('Disconnect', '断开连接') -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button
    })
    if ($matches.Count -ne 1) { return $false }
    $target = $matches[0]
    if (!$target.Current.IsOffscreen) { return $true }
    $pattern = $null
    if ($target.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern, [ref]$pattern)) {
        ([System.Windows.Automation.ScrollItemPattern]$pattern).ScrollIntoView()
        return $true
    }
    $ancestor = $target
    for ($depth=0; $depth -lt 16; $depth++) {
        $ancestor = [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($ancestor)
        if (!$ancestor -or [System.Windows.Automation.AutomationElement]::Compare($ancestor, $window)) { break }
        $pattern = $null
        if ($ancestor.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern, [ref]$pattern)) {
            $scroll = [System.Windows.Automation.ScrollPattern]$pattern
            if ($scroll.Current.VerticallyScrollable) {
                $scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll, 100)
                return $true
            }
        }
    }
    throw 'BLOCKED_UI_OBSERVATION: native remote section scrolling unavailable'
}
function Capture-Evidence([string]$Name) {
    $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    if ($elements.Count -gt 10000) { throw 'UI tree exceeds observation bound' }
    @($elements | ForEach-Object { [ordered]@{ name=$_.Current.Name; type=$_.Current.ControlType.ProgrammaticName; pid=$_.Current.ProcessId; enabled=$_.Current.IsEnabled; offscreen=$_.Current.IsOffscreen } }) | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $out "$Name-tree.json")
    $rect = $window.Current.BoundingRectangle
    if ($rect.Width -le 0 -or $rect.Height -le 0 -or $rect.Width -gt 8192 -or $rect.Height -gt 8192) { throw 'Invalid window screenshot bounds' }
    $bitmap = [Drawing.Bitmap]::new([int]$rect.Width, [int]$rect.Height)
    try {
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try { $graphics.CopyFromScreen([int]$rect.X, [int]$rect.Y, 0, 0, $bitmap.Size) } finally { $graphics.Dispose() }
        $bitmap.Save((Join-Path $out "$Name.png"), [Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
}
function Observe-Children {
    $rows = @(Get-CimInstance Win32_Process)
    $owned = @{ ([int]$app.Id) = $app }
    do {
        $next = @($rows | Where-Object { $owned.ContainsKey([int]$_.ParentProcessId) -and !$owned.ContainsKey([int]$_.ProcessId) })
        foreach ($row in $next) {
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
                $children[$id] = $process
                $owned[$id] = $process
                $process = $null
            } finally {
                # Never register or kill a handle whose ancestry/identity was not verified.
                if ($process) { $process.Dispose() }
            }
        }
    } while ($next.Count -gt 0)
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
    $result.image = $image
    $result.sha256 = (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash
    if ($FixtureCodex) {
        if (!$ActivityDiagnosticsAcceptance) { throw 'Fixture mode requires diagnostics acceptance' }
        $result.codexHistory=[ordered]@{stage='fixture-read'; cli='0.169.1'; preflight=$false; archive='not-attempted'; restore='not-attempted'; rename='not-attempted'; delete='held'; backendTraceCorrelated=$false; privacyCanariesAbsent=$false}
        $fixture=& (Join-Path $PSScriptRoot 'pr101-codex-history-fixture.ps1') -Codex $FixtureCodex
        if (!$fixture.AppServerReadVerified) { throw 'Unverified synthetic history inputs' }
        $result.codexHistory.preflight=$true
        $env:CODEX_HOME=$fixture.Home
        $env:PATH=(Split-Path $FixtureCodex -Parent) + ';' + $env:PATH
        . (Join-Path $PSScriptRoot 'pr101-codex-history-gui.ps1')
    }
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
    # Visible UI is the explicit purpose of this hosted-only acceptance step.
    $app = Start-Process -FilePath $image -WorkingDirectory (Split-Path $image) -PassThru
    $null = $app.Handle
    $result.pid = $app.Id
    if ($app.MainModule.FileName -ine $image) { throw 'Started image identity mismatch' }
    $window = Wait-Observed {
        $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $app.Id)
        $all = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
        $visible = @($all | Where-Object { !$_.Current.IsOffscreen -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window })
        if ($visible.Count -gt 1) { throw 'Ambiguous original top-level window' }
        if ($visible.Count -eq 1) { return $visible[0] }
    } 'ordinary main window'
    Invoke-Button @('Settings', '设置')
    Invoke-Button @('General', '通用')
    $agy = Wait-Observed { Find-Control @('Antigravity CLI disabled · click to enable', 'Antigravity CLI 已停用 · 点击启用') ([System.Windows.Automation.ControlType]::Button) } 'AGY disabled'
    $toggle = $null
    if (!$agy.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle) -or ([System.Windows.Automation.TogglePattern]$toggle).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off) { throw 'BLOCKED_UI_OBSERVATION: AGY pressed state unavailable or enabled' }
    Capture-Evidence 'general'
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
    Invoke-Button @('Terminal', '终端')
    $null = Wait-Observed { Reveal-RemoteSection } 'remote section native scroll'
    $null = Wait-Observed { Find-Control @('Locked — no native connection is installed.', '已锁定：尚未安装原生连接。') ([System.Windows.Automation.ControlType]::Text) } 'remote locked'
    $disconnect = Wait-Observed { Find-Control @('Disconnect', '断开连接') ([System.Windows.Automation.ControlType]::Button) } 'remote disconnect'
    if ($disconnect.Current.IsEnabled) { throw 'Remote disconnect unexpectedly enabled' }
    if (Find-Control @('Literal UTF-8 text (maximum 8192 bytes)', '原样 UTF-8 文本（最多 8192 字节）') $null) { throw 'Remote input unexpectedly visible' }
    Capture-Evidence 'terminal'
    if ($ActivityDiagnosticsAcceptance) {
        $result.activityDiagnostics = [ordered]@{ defaultOff=$false; startAcknowledged=$false; stopAcknowledged=$false; emptyPreviewCopied=$false; restartWithoutAppRestart=$false; controls=@() }
        Invoke-Button @('About')
        $button = [System.Windows.Automation.ControlType]::Button
        $start = @('Start collecting')
        $stop = @('Stop collecting')
        $preview = @('Preview activity report')
        $copy = @('Copy this preview')
        $null = Wait-Observed { Find-Control $start $button } 'diagnostics default off'
        if (Find-Control $stop $button) { throw 'Diagnostics appears enabled by default' }
        $result.activityDiagnostics.defaultOff = $true
        $isOn = {
            $ctl = Find-Control $stop $button
            $pressed = $null
            if ($ctl -and $ctl.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pressed) -and
                ([System.Windows.Automation.TogglePattern]$pressed).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On) { $ctl }
        }
        Toggle-DiagnosticsButton $start
        $null = Wait-Observed $isOn 'diagnostics started acknowledgement'
        $result.activityDiagnostics.startAcknowledged = $true
        Reveal-DiagnosticsControl $preview $button
        Invoke-Button $preview
        Reveal-DiagnosticsControl $copy $button
        Invoke-Button $copy
        Reveal-DiagnosticsControl @('Diagnostic summary copied') $null
        $null = Wait-Observed { Find-Control @('Diagnostic summary copied') $null } 'explicit diagnostic copy'
        $raw = Get-Clipboard -Raw
        if (!$raw -or $raw.Length -gt 131072) { throw 'Diagnostic export exceeds expected bound or absent' }
        $report = $raw | ConvertFrom-Json -AsHashtable
        if ($report.schemaVersion -ne 1 -or $report.events.Count -ne 0 -or !$report.backend.available -or $report.backend.events.Count -ne 0) { throw 'Unexpected initial diagnostic report' }
        $result.activityDiagnostics.emptyPreviewCopied = $true
        Reveal-DiagnosticsControl $stop $button
        Toggle-DiagnosticsButton $stop
        $null = Wait-Observed { Find-Control $start $button } 'diagnostics stop confirmed'
        $result.activityDiagnostics.stopAcknowledged = $true
        Toggle-DiagnosticsButton $start
        $null = Wait-Observed $isOn 'diagnostics second start'
        Reveal-DiagnosticsControl $preview $button
        Invoke-Button $preview
        Reveal-DiagnosticsControl $copy $button
        Invoke-Button $copy
        Reveal-DiagnosticsControl @('Diagnostic summary copied') $null
        $null = Wait-Observed { Find-Control @('Diagnostic summary copied') $null } 'second explicit diagnostic copy'
        $rawAgain = Get-Clipboard -Raw
        if (!$rawAgain -or $rawAgain.Length -gt 131072) { throw 'Second diagnostic export exceeds expected bound or absent' }
        $again = $rawAgain | ConvertFrom-Json -AsHashtable
        if (!$again.backend.available -or $again.events.Count -ne 0) { throw 'Fresh diagnostic capture was not observed' }
        Reveal-DiagnosticsControl $stop $button
        Toggle-DiagnosticsButton $stop
        $null = Wait-Observed { Find-Control $start $button } 'second diagnostics stop confirmed'
        $result.activityDiagnostics.restartWithoutAppRestart = $true
        if ($FixtureCodex) {
            Reveal-DiagnosticsControl $start $button
            Toggle-DiagnosticsButton $start
            $null=Wait-Observed $isOn 'known fixture capture starts'
            Test-CodexHistoryGui
            Invoke-Button @('Settings')
            Invoke-Button @('About')
            Reveal-DiagnosticsControl $preview $button
            Invoke-Button $preview
            Reveal-DiagnosticsControl $copy $button
            Invoke-Button $copy
            Reveal-DiagnosticsControl @('Diagnostic summary copied') $null
            $observed=Get-Clipboard -Raw
            if (!$observed -or $observed.Length -gt 131072) { throw 'Missing bounded fixture diagnosis' }
            foreach ($canary in @($fixture.ActiveCanary,$fixture.ArchivedCanary,$fixture.SessionId,$fixture.ArchivedId,$fixture.Home,$fixture.Project)) {
                if ($observed.Contains($canary) -or $observed.Contains($canary.Replace('\','\\'))) { throw 'Unexpected fixture canary in diagnosis' }
            }
            $result.codexHistory.privacyCanariesAbsent=$true
            $activity=$observed | ConvertFrom-Json -AsHashtable
            if (!$activity.backend.available) { throw 'Backend diagnosis was not available' }
            $operations=@('restore')
            if ($result.codexHistory.archive -eq 'physical-archive') { $operations += 'archive' }
            foreach ($operation in $operations) {
                $starts = @($activity.events | Where-Object { $_.operation -ceq $operation -and $_.cli -ceq 'codex' -and $_.stage -ceq 'ui' -and $_.result -ceq 'started' })
                if ($starts.Count -ne 1 -or $starts[0].id -cnotmatch '^[a-f0-9]{32}$') { throw "Expected exactly one synthetic $operation trace" }
                $id=$starts[0].id
                $events=@($activity.backend.events | Where-Object id -CEQ $id)
                if (!($events | Where-Object { $_.stage -ceq 'backend' -and $_.status -ceq 'returned' }) -or
                    !($events | Where-Object { $_.stage -ceq 'provider' -and $_.status -ceq 'ok' }) -or
                    !($activity.events | Where-Object { $_.id -ceq $id -and $_.stage -ceq 'list' -and $_.result -cin @('applied','unchanged') })) {
                    throw "Missing correlated backend, provider or target list evidence: $operation"
                }
            }
            $result.codexHistory.backendTraceCorrelated=$true
            Reveal-DiagnosticsControl $stop $button
            Toggle-DiagnosticsButton $stop
            $null=Wait-Observed { Find-Control $start $button } 'fixture capture stop confirmed'
            $result.codexHistory.stage='archive-and-restore-observed'
            if ($result.codexHistory.archive -ne 'physical-archive') { $result.codexHistory.stage='restore-observed-archive-entry-unavailable' }
        }
    }
    Observe-Children
    $result.children = @($children.Values | ForEach-Object { [ordered]@{ pid=$_.Id; name=$_.ProcessName } })
    if (@($children.Values | Where-Object ProcessName -eq 'msedgewebview2').Count -eq 0) { throw 'No actual WebView2 child observed' }
    $close = $null
    if (!$window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$close)) { throw 'BLOCKED_UI_CLOSE' }
    ([System.Windows.Automation.WindowPattern]$close).Close()
    if (!$app.WaitForExit(30000) -or $app.ExitCode -ne 0) { throw 'Ordinary App did not close normally' }
    foreach ($child in $children.Values) { if (!$child.WaitForExit(10000)) { throw 'Observed WebView child did not exit' } }
    $result.normalExit = $true
    if ($FixtureCodex -and $result.codexHistory.archive -ne 'physical-archive') {
        $result.status = 'partial'
        throw 'BLOCKED_UI_ACTION: exact synthetic history archive entry unavailable'
    }
    $result.status = 'passed'
} catch {
    if ($app -and $result.status -cne 'partial') { $result.status = 'failed' }
    $result.error = $_.Exception.Message
    if ($window) { try { Capture-Evidence 'failure' } catch { $result.captureError = $_.Exception.Message } }
    throw
} finally {
    # Failure cleanup is never retirement evidence or a passed UI acceptance.
    try {
        if ($app -and !$app.HasExited) { $app.Kill(); $null = $app.WaitForExit(10000) }
        foreach ($child in $children.Values) { if (!$child.HasExited) { $child.Kill(); $null = $child.WaitForExit(10000) }; $child.Dispose() }
    } catch {
        $result.cleanupError = $_.Exception.Message
        $result.status = 'failed'
        throw
    } finally {
        $result | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'result.json')
        if ($app) { $app.Dispose() }
    }
}
