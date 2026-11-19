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
function Test-ExactLocalImageAlias([string]$Left, [string]$Right) {
    $paths = @($Left, $Right)
    $extended = @($false, $false)
    for ($i=0; $i -lt 2; $i++) {
        if ($paths[$i].StartsWith('\\?\', [StringComparison]::Ordinal)) { $extended[$i]=$true; $paths[$i]=$paths[$i].Substring(4) }
        if ($paths[$i] -cnotmatch '^[A-Za-z]:\\[^/\x00-\x1f:]+$') { return $false }
        foreach ($part in $paths[$i].Substring(3).Split('\')) {
            if (!$part -or $part -in @('.','..') -or $part.EndsWith('.') -or $part.EndsWith(' ')) { return $false }
        }
    }
    return (($extended[0] -ne $extended[1]) -and [StringComparer]::OrdinalIgnoreCase.Equals($paths[0],$paths[1]))
}
function Test-ImageFileIdentity([string]$Left, [string]$Right) {
    if (!(Test-ExactLocalImageAlias $Left $Right)) { return $false }
    if (!('PackagedImageIdentity' -as [type])) {
        Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class PackagedImageIdentity {
  [StructLayout(LayoutKind.Sequential)] struct Info {
    public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh;
    public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
  }
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle file, out Info info);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern SafeFileHandle CreateFileW(string path, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
  public static bool Same(string a, string b) {
    using (var x = CreateFileW(a, 0, 7, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero))
    using (var y = CreateFileW(b, 0, 7, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero)) {
      if (x.IsInvalid || y.IsInvalid) return false;
      Info i,j;
      if (!GetFileInformationByHandle(x,out i) || !GetFileInformationByHandle(y,out j)) return false;
      if (((i.Attributes | j.Attributes) & 0x410) != 0) return false;
      return i.Volume == j.Volume && i.IndexHigh == j.IndexHigh && i.IndexLow == j.IndexLow;
    }
  }
}
"@
    }
    return [PackagedImageIdentity]::Same($Left,$Right)
}
function Observe-Children {
    $rows = @(Get-CimInstance Win32_Process)
    $owned = @{ ([int]$app.Id) = $app }
    $unobserved = @{}
    do {
        $next = @($rows | Where-Object { $owned.ContainsKey([int]$_.ParentProcessId) -and !$owned.ContainsKey([int]$_.ProcessId) -and !$unobserved.ContainsKey([int]$_.ProcessId) })
        foreach ($row in $next) {
            $id = [int]$row.ProcessId
            $process = $null
            try {
                $parent = $owned[[int]$row.ParentProcessId]
                $inventoryStage = "child-handle"
                try { $process = [Diagnostics.Process]::GetProcessById($id) }
                catch {
                    if (!$CanonicalDiagnostic -or $_.Exception.InnerException -isnot [ArgumentException]) { throw }
                    # A vanished enumerated PID has no held identity. Never register/kill it,
                    # or use it as a trusted parent. Diagnostic scope may still be observed,
                    # but this gap permanently prevents whole-run acceptance.
                    $unobserved[$id] = $true
                    $result.childObservationUnknown = 'enumerated-child-exited-before-handle'
                    $result.lastUnheldChildKind = switch ($row.Name) {
                        'codex.exe' { 'codex'; break }
                        'node.exe' { 'node-wrapper'; break }
                        'conhost.exe' { 'conhost'; break }
                        'OpenConsole.exe' { 'openconsole'; break }
                        'msedgewebview2.exe' { 'webview'; break }
                        default { 'other-or-unknown' }
                    }
                    if (!$result.Contains('firstUnheldChild')) {
                        $result.firstUnheldChild = [ordered]@{
                            pid=$id; parentPid=[int]$row.ParentProcessId
                            kind=$result.lastUnheldChildKind
                            phase=$(if($result.Contains('nativeStage')){$result.nativeStage}else{'pre-native'})
                            mayBeCodex=($row.Name -ieq 'codex.exe' -or $result.lastUnheldChildKind -eq 'other-or-unknown')
                        }
                    }
                    continue
                }
                $inventoryStage = 'held-handle'
                $null = $process.Handle
                $inventoryStage = 'parent-live'
                if ($parent.HasExited) { throw 'Unverified child identity or parent lifetime' }
                $inventoryStage = 'child-live'
                if ($process.HasExited) { throw 'Unverified child identity or parent lifetime' }
                $inventoryStage = 'creation-token-read'
                $created = ([DateTime]$row.CreationDate).ToUniversalTime()
                $started = $process.StartTime.ToUniversalTime()
                $inventoryStage = 'start-time-match'
                if (($created.Ticks - ($created.Ticks % 10)) -ne ($started.Ticks - ($started.Ticks % 10))) {
                    if ($CanonicalDiagnostic -and !$result.Contains('childInventoryFailure')) { $result.childCreationPrecision = [ordered]@{absoluteDeltaTicks=[Math]::Abs($created.Ticks-$started.Ticks); cimSubMicrosecondTicks=($created.Ticks%10); handleSubMicrosecondTicks=($started.Ticks%10)} }
                    throw 'Unverified child identity or parent lifetime'
                }
                $inventoryStage = 'image-present'
                if (!$row.ExecutablePath) { throw 'Unverified child identity or parent lifetime' }
                $inventoryStage = 'image-read-error'
                $heldImage = $process.MainModule.FileName
                $inventoryStage = 'image-empty'
                if (!$heldImage) { throw 'Unverified child identity or parent lifetime' }
                $inventoryStage = 'image-path-not-equal'
                if ($heldImage -ine $row.ExecutablePath) {
                    if ($CanonicalDiagnostic -and !$result.Contains('childInventoryFailure')) {
                        $result.childImageShape = [ordered]@{
                            cimExtendedPrefix=([string]$row.ExecutablePath).StartsWith('\\?\')
                            heldExtendedPrefix=$heldImage.StartsWith('\\?\')
                            cimSystem32=([string]$row.ExecutablePath -match '(?i)\\System32\\')
                            heldSystem32=($heldImage -match '(?i)\\System32\\')
                            cimSysWow64=([string]$row.ExecutablePath -match '(?i)\\SysWOW64\\')
                            heldSysWow64=($heldImage -match '(?i)\\SysWOW64\\')
                        }
                    }
                    $inventoryStage = 'image-alias-file-identity'
                    if (!$CanonicalDiagnostic -or !(Test-ImageFileIdentity ([string]$row.ExecutablePath) $heldImage)) { throw 'Unverified child identity or parent lifetime' }
                    if ($process.HasExited -or $parent.HasExited -or $process.StartTime.ToUniversalTime() -ne $started) { throw 'Unverified child identity or parent lifetime' }
                    $result.imagePrefixAliasFileIdentityMatched = $true
                }
                $inventoryStage = 'session-match'
                if ($process.SessionId -ne $app.SessionId -or [int]$row.SessionId -ne $app.SessionId) { throw 'Unverified child identity or parent lifetime' }
                $inventoryStage = 'parent-before-child'
                if ($started -lt $parent.StartTime.ToUniversalTime()) { throw 'Unverified child identity or parent lifetime' }
                $children[$id] = $process
                $owned[$id] = $process
                $process = $null
            } catch {
                if ($CanonicalDiagnostic -and !$result.Contains('childInventoryFailure')) {
                    $kind = switch ($row.Name) {
                        'codex.exe' { 'codex'; break }
                        'node.exe' { 'node-wrapper'; break }
                        'conhost.exe' { 'conhost'; break }
                        'OpenConsole.exe' { 'openconsole'; break }
                        'msedgewebview2.exe' { 'webview'; break }
                        default { 'other-or-unknown' }
                    }
                    $result.childInventoryFailure = [ordered]@{stage=$inventoryStage; pid=$id; parentPid=[int]$row.ParentProcessId; kind=$kind; registered=$false}
                }
                throw
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
    if ($CanonicalDiagnostic -and $NativeCodex) { $env:CC_DESK_DIAGNOSTIC_EXPECTED_PROGRAM = $NativeCodex }
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
    try { Observe-Children } catch {
        if (!$CanonicalDiagnostic) { throw }
        $result.childObservationUnknown = 'child-inventory-not-proven'
    }
    $result.children = @($children.Values | ForEach-Object { [ordered]@{ pid=$_.Id; name=$_.ProcessName } })
    if (@($children.Values | Where-Object ProcessName -eq 'msedgewebview2').Count -eq 0) {
        if (!$CanonicalDiagnostic) { throw 'No actual WebView2 child observed' }
        $result.childObservationUnknown = 'webview-child-not-proven'
    }
    $close = $null
    if (!$window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$close)) { throw 'BLOCKED_UI_CLOSE' }
    ([System.Windows.Automation.WindowPattern]$close).Close()
    if (!$app.WaitForExit(30000) -or $app.ExitCode -ne 0) { throw 'Ordinary App did not close normally' }
    $result.normalAppExit = $true
    foreach ($child in $children.Values) { if (!$child.WaitForExit(10000)) { throw 'Observed WebView child did not exit' } }
    if ($result.Contains('childObservationUnknown')) { throw 'CHILD_OBSERVATION_UNKNOWN' }
    if ($result.Contains('runningCodexUnknown')) { throw 'RUNNING_CODEX_IDENTITY_UNKNOWN' }
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

    if ($CanonicalDiagnostic -and $window -and $result.Contains('nativeStage') -and $result.nativeStage -eq 'create-actual-run') {
        # Fixed public UI labels only. A CLI may have started: never capture the App/terminal tree or arbitrary text.
        try {
            $labels = @(
                'Finding the installed CLI…', '正在自动查找已安装的命令行程序…',
                'Found the CLI. Confirm this program once; future sessions will use the saved selection.', '已找到命令行程序。首次使用时确认一次，以后新建会话会复用保存的选择。',
                'No CLI program was found. Check that it is installed, or edit the configuration for a custom location.', '未找到命令行程序。请确认已经安装；自定义安装位置可通过编辑配置指定。',
                'Automatic CLI detection could not finish. View details or edit the launch configuration.', '自动识别未能完成。请查看详情，或编辑启动配置。',
                'Detected program', '检测到的程序', 'Use and start session', '使用并启动会话',
                'Create session', '创建会话', '单次远程连接', 'Toggle details', '切换详情',
                'PROGRAM_TRUST_REQUIRED', 'PROGRAM_UNAVAILABLE', 'ENV_SOURCE_MISSING', 'PROFILE_NOT_FOUND',
                'PROFILE_CLI_MISMATCH', 'PROFILE_SELECTION_CHANGED', 'PROJECT_IDENTITY_CHANGED',
                'REVISION_CONFLICT', 'FORBIDDEN', 'DOCUMENT_BRIDGE_UNAVAILABLE', 'INVALID_REQUEST', 'GENERIC_UNAVAILABLE'
            )
            $observations = @($labels | ForEach-Object {
                $label = $_
                $matches = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $label)))
                if ($matches.Count -gt 32) { throw 'Fixed launch label match bound exceeded' }
                [ordered]@{label=$label; count=$matches.Count; controls=@($matches | ForEach-Object { [ordered]@{type=$_.Current.ControlType.ProgrammaticName; enabled=$_.Current.IsEnabled; offscreen=$_.Current.IsOffscreen} })}
            })
            $observations | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'launch-fixed-state.json')
        } catch { $result.launchStateObservation = 'fixed-label-observation-unavailable' }
    }
    if ($NativeCodex -and $window -and $result.Contains('nativeStage') -and $result.nativeStage -in @('observe-host-state','read-public-scope')) {
        # Only the formally labelled Host panel; never enumerate terminal descendants into evidence.
        try {
            $panels = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, '单次远程会话')))
            if ($panels.Count -ne 1) { throw 'Host panel not unique' }
            $elements = @($panels[0].FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition))
            if ($elements.Count -gt 1000) { throw 'Host panel bound exceeded' }
            @($elements | ForEach-Object { [ordered]@{name=$_.Current.Name; id=$_.Current.AutomationId; type=$_.Current.ControlType.ProgrammaticName; enabled=$_.Current.IsEnabled; offscreen=$_.Current.IsOffscreen; patterns=@($_.GetSupportedPatterns() | ForEach-Object {$_.ProgrammaticName})} }) | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'host-panel-failure-tree.json')
        } catch { $result.captureError = $_.Exception.Message }
    }
    if ($CanonicalDiagnostic -and $window -and $app -and !$app.HasExited) {
        # Any diagnostic failure keeps its original result and still observes normal App exit.
        try {
            try { Observe-Children } catch {
                $result.childObservationUnknown = 'child-inventory-not-proven'
            }
            $close = $null
            if (!$window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$close)) { throw 'BLOCKED_UI_CLOSE' }
            ([System.Windows.Automation.WindowPattern]$close).Close()
            if (!$app.WaitForExit(30000) -or $app.ExitCode -ne 0) { throw 'Diagnostic normal exit failed' }
            $result.normalAppExit = $true
            foreach ($child in $children.Values) { if (!$child.WaitForExit(10000)) { throw 'Observed diagnostic child did not exit' } }
            $result.normalExit = !$result.Contains('childObservationUnknown') -and !$result.Contains('runningCodexUnknown')
        } catch { $result.normalExitError = $_.Exception.Message }
    }
    if ($window -and !$NativeCodex) { try { Capture-Evidence 'failure' } catch { $result.captureError = $_.Exception.Message } }
    throw
} finally {
    # Failure cleanup is never retirement evidence or a passed UI acceptance.
    try {
        if (!$CanonicalDiagnostic -and $app -and !$app.HasExited) { $app.Kill(); $null = $app.WaitForExit(10000) }
        foreach ($child in $children.Values) { if (!$CanonicalDiagnostic -and !$child.HasExited) { $child.Kill(); $null = $child.WaitForExit(10000) }; $child.Dispose() }
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
