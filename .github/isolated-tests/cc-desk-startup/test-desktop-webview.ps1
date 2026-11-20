# Ordinary installed product, disposable hosted Windows only. Never a fake IPC page.
param([Parameter(Mandatory=$true)][string]$Exe)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$expectedExe=$env:CCDESK_SCRATCH_EXPECTED_EXE
$sizeText=$env:CCDESK_SCRATCH_EXPECTED_BYTES
[Environment]::SetEnvironmentVariable('CCDESK_SCRATCH_EXPECTED_BYTES',$null,'Process')
if ($sizeText -cnotmatch '^[1-9][0-9]{0,8}$' -or [int64]$sizeText -gt 157286400) { throw 'Missing validated expected member size' }
$expectedSize=[int64]$sizeText
[Environment]::SetEnvironmentVariable('CCDESK_SCRATCH_EXPECTED_EXE',$null,'Process')
if ($expectedExe -cnotmatch '^[0-9a-f]{64}$') { throw 'Missing validated expected application digest' }
$out = Join-Path (Get-Location) 'safe-webview-probe'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$result = [ordered]@{ status = 'blocked'; normalExit = $false; stage = 'preflight'; sample = $null; cleanupFailed = $false }
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
    if (!$control.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { throw 'BLOCKED_UI_INVOKE' }
    ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
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
function Read-StartupMilestones {
    $s=[ordered]@{loggerStarted=$false; ptyManagerInitialized=$false; webviewSettingsApplied=$false; buildIdentityMatches=$false}
    if (!$app) { return $s }
    try {
        $appHome=Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cc-box'
        $logs=Join-Path $appHome 'logs'
        foreach ($dir in @($appHome,$logs)) {
            $item=Get-Item -LiteralPath $dir -ErrorAction Stop
            if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return $s }
        }
        $files=@(Get-ChildItem -LiteralPath $logs -File -Filter '*.log' | Where-Object Name -cmatch '^\d{4}-\d{2}-\d{2}\.log$')
        if ($files.Count -ne 1) { return $s }
        $log=$files[0]
        if ($log.Length -gt 262144 -or ($log.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return $s }
        $lines=@(Get-Content -LiteralPath $log.FullName -TotalCount 4096)
        $s.loggerStarted=@($lines | Where-Object { $_ -cmatch '\[INFO\] \[cc_desk::logger\] === CC Desk started ===$' }).Count -eq 1
        $s.ptyManagerInitialized=@($lines | Where-Object { $_ -cmatch '\[INFO\] \[cc_desk\] PTY manager initialized$' }).Count -eq 1
        $s.webviewSettingsApplied=@($lines | Where-Object { $_ -cmatch '\[INFO\] \[cc_desk\] WebView2 browser accelerator keys disabled$' }).Count -eq 1
        $s.buildIdentityMatches=@($lines | Where-Object { $_ -cmatch '\[INFO\] \[cc_desk::logger\] Build identity: version=0\.28\.0-rc\.1 sha=04a2f0cedc8e7acc080e8736723c6e8799127f58$' }).Count -eq 1
    } catch { return $s }
    return $s
}
function Measure-SafeStartup {
    $snapshot=[ordered]@{rootAlive=$false; shellObserved=$false; uiState='not-observed'; webviewObservation='not-observed'; webviewCount=0; webviewVersions=@(); titleBarCount=0; paneCount=0; workspaceCount=0; settingsCount=0; generalCount=0; terminalCount=0; lockedCount=0}
    $snapshot.milestones=Read-StartupMilestones
    if (!$app -or $app.HasExited) { return $snapshot }
    $snapshot.rootAlive=$true
    try {
        # Original owned-process identity capture: handle, parent lifetime, session, image, creation time.
        Observe-Children
        foreach ($child in $children.Values) {
            if ($child.HasExited -or $child.ProcessName -ine 'msedgewebview2') { continue }
            $module=$child.MainModule
            if ([IO.Path]::GetFileName($module.FileName) -ine 'msedgewebview2.exe') { continue }
            $version=$module.FileVersionInfo
            if ($child.HasExited -or $child.MainModule.FileName -ine $module.FileName) { throw 'Observed engine identity became unavailable' }
            $snapshot.webviewCount++
            $actual='{0}.{1}.{2}.{3}' -f $version.FileMajorPart,$version.FileMinorPart,$version.FileBuildPart,$version.FilePrivatePart
            if ($actual -ne '0.0.0.0' -and $actual -cmatch '^\d{1,5}(\.\d{1,5}){3}$' -and $actual -cnotin $snapshot.webviewVersions) {
                $snapshot.webviewVersions+= $actual
            }
        }
        $snapshot.webviewObservation='complete'
    } catch { $snapshot.webviewObservation='partial-or-unavailable' }
    if (!$window) { return $snapshot }
    try {
        if ($window.Current.ProcessId -ne $app.Id -or $window.Current.IsOffscreen -or $app.HasExited) { return $snapshot }
        $snapshot.shellObserved=$true
        $elements=$window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
        if ($elements.Count -gt 10000) { $snapshot.uiState='bounded-out'; return $snapshot }
        foreach ($e in $elements) {
            if ($e.Current.IsOffscreen) { continue }
            $type=$e.Current.ControlType
            $name=$e.Current.Name
            if ($type -eq [System.Windows.Automation.ControlType]::TitleBar) { $snapshot.titleBarCount++ }
            if ($type -eq [System.Windows.Automation.ControlType]::Pane) { $snapshot.paneCount++ }
            if ($type -eq [System.Windows.Automation.ControlType]::Button) {
                if ($name -cin @('Workspace','工作区')) { $snapshot.workspaceCount++ }
                if ($name -cin @('Settings','设置')) { $snapshot.settingsCount++ }
                if ($name -cin @('General','通用')) { $snapshot.generalCount++ }
                if ($name -cin @('Terminal','终端')) { $snapshot.terminalCount++ }
            }
            if ($type -eq [System.Windows.Automation.ControlType]::Text -and
                $name -cin @('Locked — no native connection is installed.','已锁定：尚未安装原生连接。')) { $snapshot.lockedCount++ }
        }
        $snapshot.uiState='counted'
    } catch { $snapshot.uiState='partial-or-unavailable' }
    return $snapshot
}
function Observe-Children {
    $rows = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate,ExecutablePath,SessionId)
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
    $result.sha256 = (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash
    $result.imageBytes=(Get-Item -LiteralPath $image).Length
    if ($result.sha256 -ine $expectedExe -or $result.imageBytes -ne $expectedSize) { throw 'Installed image differs from verified named NSIS member' }
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $result.stage = 'native-shell'
    # Visible UI is the explicit purpose of this hosted-only acceptance step.
    $app = Start-Process -FilePath $image -WorkingDirectory (Split-Path $image) -PassThru
    $null = $app.Handle
    if ($app.MainModule.FileName -ine $image) { throw 'Started image identity mismatch' }
    $window = Wait-Observed {
        $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $app.Id)
        $all = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
        $visible = @($all | Where-Object { !$_.Current.IsOffscreen -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window })
        if ($visible.Count -gt 1) { throw 'Ambiguous original top-level window' }
        if ($visible.Count -eq 1) { return $visible[0] }
    } 'ordinary main window'
    $result.stage = 'settings'
    Invoke-Button @('Settings', '设置')
    $result.stage = 'general-defaults'
    Invoke-Button @('General', '通用')
    $agy = Wait-Observed { Find-Control @('Antigravity CLI disabled · click to enable', 'Antigravity CLI 已停用 · 点击启用') ([System.Windows.Automation.ControlType]::Button) } 'AGY disabled'
    $toggle = $null
    if (!$agy.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle) -or ([System.Windows.Automation.TogglePattern]$toggle).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off) { throw 'BLOCKED_UI_OBSERVATION: AGY pressed state unavailable or enabled' }
    $result.stage = 'terminal-defaults'
    Invoke-Button @('Terminal', '终端')
    $null = Wait-Observed { Reveal-RemoteSection } 'remote section native scroll'
    $null = Wait-Observed { Find-Control @('Locked — no native connection is installed.', '已锁定：尚未安装原生连接。') ([System.Windows.Automation.ControlType]::Text) } 'remote locked'
    $disconnect = Wait-Observed { Find-Control @('Disconnect', '断开连接') ([System.Windows.Automation.ControlType]::Button) } 'remote disconnect'
    if ($disconnect.Current.IsEnabled) { throw 'Remote disconnect unexpectedly enabled' }
    if (Find-Control @('Literal UTF-8 text (maximum 8192 bytes)', '原样 UTF-8 文本（最多 8192 字节）') $null) { throw 'Remote input unexpectedly visible' }
    $result.stage = 'process-identity'
    $result.sample=Measure-SafeStartup
    if ($result.sample.webviewObservation -cne 'complete') { throw 'Owned WebView observation incomplete' }
    if (@($children.Values | Where-Object ProcessName -eq 'msedgewebview2').Count -eq 0) { throw 'No actual WebView2 child observed' }
    $result.stage = 'normal-exit'
    $close = $null
    if (!$window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$close)) { throw 'BLOCKED_UI_CLOSE' }
    ([System.Windows.Automation.WindowPattern]$close).Close()
    if (!$app.WaitForExit(30000) -or $app.ExitCode -ne 0) { throw 'Ordinary App did not close normally' }
    foreach ($child in $children.Values) { if (!$child.WaitForExit(10000)) { throw 'Observed WebView child did not exit' } }
    $result.normalExit = $true
    $result.stage = 'complete'
    $result.status = 'passed'
} catch {
    if ($app) { $result.status = 'failed' }
    if ($null -eq $result.sample) { $result.sample=Measure-SafeStartup }
    throw 'Ordinary installed WebView acceptance blocked; see safe receipt.'
} finally {
    # Failure cleanup is never retirement evidence or a passed UI acceptance.
    try {
        if ($app -and !$app.HasExited) { $app.Kill(); $null = $app.WaitForExit(10000) }
        foreach ($child in $children.Values) { if (!$child.HasExited) { $child.Kill(); $null = $child.WaitForExit(10000) }; $child.Dispose() }
    } catch {
        $result.cleanupFailed = $true
        $result.status = 'failed'
        throw 'Owned startup cleanup failed; see safe receipt.'
    } finally {
        $result | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'result.json')
        if ($app) { $app.Dispose() }
    }
}
