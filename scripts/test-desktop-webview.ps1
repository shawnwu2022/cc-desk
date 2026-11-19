# Ordinary installed product, disposable hosted Windows only. Never a fake IPC page.
param([Parameter(Mandatory=$true)][string]$Exe, [string]$NativeCodex, [string]$ExpectedSha256, [switch]$CanonicalDiagnostic)
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
    $control = Wait-Observed { $c = Find-Control $Names ([System.Windows.Automation.ControlType]::Button); if ($c -and $c.Current.IsEnabled) { return $c }; return $null } ($Names -join '/')
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

    if ($ExpectedSha256 -and (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash -ine $ExpectedSha256) { throw 'Pinned product digest mismatch' }
    if ($NativeCodex) {
        foreach ($key in @('GITHUB_TOKEN','GH_TOKEN','OPENAI_API_KEY','CODEX_API_KEY','AZURE_OPENAI_API_KEY')) {
            if ([Environment]::GetEnvironmentVariable($key)) { throw 'Credential environment must not reach product' }
        }
        $NativeCodex = (Resolve-Path -LiteralPath $NativeCodex).Path
        $codexSha = 'f76f94d08b0793bfa129ae62c09341ee32ae96d57e69968ae6b8d7c71a20d8c4'
        if (!$NativeCodex.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -or
            (Get-FileHash -LiteralPath $NativeCodex -Algorithm SHA256).Hash -ine $codexSha) { throw 'Pinned official Codex required' }
        $nativeRoot = Join-Path $env:RUNNER_TEMP ('packaged-native-' + [guid]::NewGuid())
        $nativeProject = Join-Path $nativeRoot 'empty-project'
        $env:CODEX_HOME = Join-Path $nativeRoot 'empty-codex-home'
        $env:CODEX_SQLITE_HOME = Join-Path $nativeRoot 'empty-codex-sqlite'
        New-Item -ItemType Directory -Path $nativeProject,$env:CODEX_HOME,$env:CODEX_SQLITE_HOME | Out-Null
        $env:PATH = (Split-Path $NativeCodex) + ';' + $env:PATH
        $env:TERM = 'xterm-256color'
        $env:COLORTERM = 'truecolor'
        . (Join-Path $PSScriptRoot 'test-packaged-native-scope.ps1')
    }

    $result.image = $image
    $result.sha256 = (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
    if ($NativeCodex) {
        # Process-local only. Hosted automation tokens must not be inherited by App/Codex.
        foreach ($variable in @(Get-ChildItem Env: | Where-Object { $_.Name -match '(^ACTIONS_|^GITHUB_|^GH_|TOKEN|SECRET|PASSWORD|API_KEY)' })) {
            [Environment]::SetEnvironmentVariable($variable.Name, $null, 'Process')
        }
    }
    # Visible UI is the explicit purpose of this hosted-only acceptance step.
    $startArgs = @{FilePath=$image; WorkingDirectory=(Split-Path $image); PassThru=$true}
    if ($CanonicalDiagnostic) {
        $diagnosticRaw = Join-Path $env:RUNNER_TEMP ('canonical-stderr-' + [guid]::NewGuid() + '.txt')
        $startArgs.RedirectStandardError = $diagnosticRaw
    }
    if ($NativeCodex) { $result.nativeStage = 'initial-product-ui' }
    $app = Start-Process @startArgs
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
    if ($NativeCodex) {
        # Fresh product defaults hide Codex. Enable it through the observed General control.
        $codexToggle = Wait-Observed { Find-Control @('Codex CLI disabled · click to enable') ([System.Windows.Automation.ControlType]::Button) } 'Codex support default off'
        $pattern = $null
        if (!$codexToggle.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) { throw 'Codex support toggle unavailable' }
        $pattern = [System.Windows.Automation.TogglePattern]$pattern
        if ($pattern.Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off) { throw 'Codex support was not default off' }
        $pattern.Toggle()
        $null = Wait-Observed {
            $control = Find-Control @('Codex CLI enabled · click to disable') ([System.Windows.Automation.ControlType]::Button)
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
    if ($NativeCodex) { Test-PackagedNativeScope }
    Observe-Children
    $result.children = @($children.Values | ForEach-Object { [ordered]@{ pid=$_.Id; name=$_.ProcessName } })
    if (@($children.Values | Where-Object ProcessName -eq 'msedgewebview2').Count -eq 0) { throw 'No actual WebView2 child observed' }
    $close = $null
    if (!$window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$close)) { throw 'BLOCKED_UI_CLOSE' }
    ([System.Windows.Automation.WindowPattern]$close).Close()
    if (!$app.WaitForExit(30000) -or $app.ExitCode -ne 0) { throw 'Ordinary App did not close normally' }
    foreach ($child in $children.Values) { if (!$child.WaitForExit(10000)) { throw 'Observed WebView child did not exit' } }
    $result.normalExit = $true
    $result.status = 'passed'
} catch {
    if ($app) { $result.status = 'failed' }
    $result.error = $_.Exception.Message
    if ($NativeCodex -and $app -and $result.Contains('nativeStage') -and $result.nativeStage -eq 'initial-product-ui') {
        try {
            $result.initialAppExited = $app.HasExited
            if ($app.HasExited) { $result.initialAppExitCode = $app.ExitCode }
            else {
                Observe-Children
                @($children.Values | Select-Object -First 128 | ForEach-Object { [ordered]@{pid=$_.Id; imageName=[IO.Path]::GetFileName($_.MainModule.FileName); exited=$_.HasExited} }) | ConvertTo-Json -Depth 3 | Set-Content -Encoding utf8 (Join-Path $out 'initial-child-metadata.json')
            }
        } catch { $result.startupCaptureError = 'Startup process observation unavailable' }
    }
    if ($NativeCodex -and $window -and $result.Contains('nativeStage') -and $result.nativeStage -in @('initial-product-ui','add-empty-project','safe-new-session-dialog')) {
        # No CLI has been created. Desktop window metadata only; contents stay scoped to this App.
        try {
            Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class PackagedWindowOwners { [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hwnd, uint command); [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid); }'
            $roots = @([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.Condition]::TrueCondition))
            if ($roots.Count -gt 128) { throw 'Desktop window metadata bound exceeded' }
            @($roots | ForEach-Object {
                $handle = [IntPtr]::new($_.Current.NativeWindowHandle)
                $owner = [PackagedWindowOwners]::GetWindow($handle,4)
                [uint32]$ownerPid = 0
                if ($owner -ne [IntPtr]::Zero) { $null = [PackagedWindowOwners]::GetWindowThreadProcessId($owner,[ref]$ownerPid) }
                $name = $_.Current.Name
                if ($name.Length -gt 256) { $name = $name.Substring(0,256) }
                [ordered]@{name=$name; class=$_.Current.ClassName; pid=$_.Current.ProcessId; handle=$_.Current.NativeWindowHandle; owner=$owner.ToInt64(); ownerPid=$ownerPid; type=$_.Current.ControlType.ProgrammaticName; offscreen=$_.Current.IsOffscreen}
            }) | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $out 'desktop-project-window-metadata.json')
            $elements = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition))
            if ($elements.Count -gt 10000) { throw 'Empty App UI bound exceeded' }
            @($elements | ForEach-Object { [ordered]@{name=$_.Current.Name; id=$_.Current.AutomationId; class=$_.Current.ClassName; type=$_.Current.ControlType.ProgrammaticName; pid=$_.Current.ProcessId; handle=$_.Current.NativeWindowHandle; enabled=$_.Current.IsEnabled; offscreen=$_.Current.IsOffscreen; patterns=@($_.GetSupportedPatterns() | ForEach-Object {$_.ProgrammaticName})} }) | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'empty-project-failure-tree.json')
        } catch { $result.captureError = $_.Exception.Message }
    }

    if ($NativeCodex -and $window -and $result.Contains('nativeStage') -and $result.nativeStage -eq 'read-public-scope') {
        # Only the formally labelled Host panel; never enumerate terminal descendants into evidence.
        try {
            $panels = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, '单次远程会话')))
            if ($panels.Count -ne 1) { throw 'Host panel not unique' }
            $elements = @($panels[0].FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition))
            if ($elements.Count -gt 1000) { throw 'Host panel bound exceeded' }
            @($elements | ForEach-Object { [ordered]@{name=$_.Current.Name; id=$_.Current.AutomationId; type=$_.Current.ControlType.ProgrammaticName; enabled=$_.Current.IsEnabled; offscreen=$_.Current.IsOffscreen; patterns=@($_.GetSupportedPatterns() | ForEach-Object {$_.ProgrammaticName})} }) | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'host-panel-failure-tree.json')
        } catch { $result.captureError = $_.Exception.Message }
    }
    if ($CanonicalDiagnostic -and $result.Contains('nativeScopeError') -and $app -and !$app.HasExited) {
        # A failed scope does not skip the independent normal desktop exit observation.
        try {
            Observe-Children
            $close = $null
            if (!$window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$close)) { throw 'BLOCKED_UI_CLOSE' }
            ([System.Windows.Automation.WindowPattern]$close).Close()
            if (!$app.WaitForExit(30000) -or $app.ExitCode -ne 0) { throw 'Diagnostic normal exit failed' }
            $result.normalAppExit = $true
            foreach ($child in $children.Values) { if (!$child.WaitForExit(10000)) { throw 'Observed diagnostic child did not exit' } }
            $result.normalExit = $true
        } catch { $result.normalExitError = $_.Exception.Message }
    }
    if ($window -and !$NativeCodex) { try { Capture-Evidence 'failure' } catch { $result.captureError = $_.Exception.Message } }
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
        if ($CanonicalDiagnostic -and (Get-Variable diagnosticRaw -ErrorAction SilentlyContinue) -and (Test-Path -LiteralPath $diagnosticRaw)) {
            # Raw stderr is never uploaded. Only the fixed numeric grammar is retained.
            $safe = @(Get-Content -LiteralPath $diagnosticRaw | Where-Object { $_ -cmatch '^CC_CANONICAL_DIAG stage=[0-9]{1,3} code=[0-9]{1,3} a=[0-9]{1,20} b=[0-9]{1,20} elapsed_ms=[0-9]{1,20}$' } | Select-Object -First 64)
            $safe | Set-Content -Encoding utf8 (Join-Path $out 'canonical-diagnostic.txt')
            $result.diagnosticEventCount = $safe.Count
        }
        $result | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'result.json')
        if ($app) { $app.Dispose() }
    }
}
